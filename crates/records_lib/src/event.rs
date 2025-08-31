//! This module contains anything related to ShootMania Obstacle events in this library.

use entity::{
    event, event_category, event_edition, event_edition_admins, event_edition_categories,
    event_edition_maps, maps, players,
};
use futures::{Stream, TryStreamExt as _};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, Order, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, RelationTrait as _, StreamTrait,
    prelude::Expr,
    sea_query::{Asterisk, ExprTrait as _, Func, IntoCondition, Query},
};

use crate::error::RecordsResult;

/// Represents an item in the event list.
///
/// In general, when we want a list of events, we return a list of this type.
#[derive(serde::Serialize)]
pub struct EventListItem {
    /// The event handle.
    pub handle: String,
    /// The ID of the last edition of the event.
    pub last_edition_id: i64,
    /// The concrete event model.
    #[serde(skip_serializing)]
    pub event: event::Model,
}

#[derive(FromQueryResult)]
struct RawSqlEventListItem {
    handle: String,
    last_edition_id: u32,
    #[sea_orm(nested)]
    event: event::Model,
}

impl From<RawSqlEventListItem> for EventListItem {
    fn from(value: RawSqlEventListItem) -> Self {
        Self {
            handle: value.handle,
            last_edition_id: value.last_edition_id as _,
            event: value.event,
        }
    }
}

/// Returns the list of events from the database.
// FIXME: this function doesn't list events with no map on purpose, why?
pub async fn event_list<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    ignore_expired: bool,
) -> RecordsResult<Vec<EventListItem>> {
    event::Entity::find()
        .reverse_join(event_edition::Entity)
        .join(
            sea_orm::JoinType::InnerJoin,
            event_edition::Entity::belongs_to(event_edition_maps::Entity)
                .from((event_edition::Column::EventId, event_edition::Column::Id))
                .to((
                    event_edition_maps::Column::EventId,
                    event_edition_maps::Column::EditionId,
                ))
                .into(),
        )
        .filter(Expr::col(event_edition::Column::StartDate).lt(Func::cust("SYSDATE")))
        .group_by(event::Column::Id)
        .group_by(event::Column::Handle)
        .order_by(event::Column::Id, Order::Asc)
        .column_as(event::Column::Handle, "handle")
        .column_as(event_edition::Column::Id.max(), "last_edition_id")
        .apply_if(ignore_expired.then_some(()), |query, _| {
            query.filter(
                event_edition::Column::Ttl
                    .is_null()
                    .or(Func::cust("TIMESTAMPADD")
                        .arg(Expr::custom_keyword("SECOND"))
                        .arg(Expr::col(event_edition::Column::Ttl))
                        .arg(Expr::col(event_edition::Column::StartDate))
                        .gt(Func::cust("SYSDATE"))),
            )
        })
        .into_model::<RawSqlEventListItem>()
        .stream(conn)
        .await?
        .map_ok(From::from)
        .map_err(From::from)
        .try_collect()
        .await
}

/// Returns the list of event editions bound to the provided event handle.
pub async fn event_editions_list<C: ConnectionTrait>(
    conn: &C,
    handle: &str,
) -> RecordsResult<Vec<event_edition::Model>> {
    let result = event_edition::Entity::find()
        .inner_join(event::Entity)
        .filter(
            event::Column::Handle
                .eq(handle)
                .and(Expr::col(event_edition::Column::StartDate).lt(Func::cust("SYSDATE")))
                .and(
                    event_edition::Column::Ttl
                        .is_null()
                        .or(Func::cust("TIMESTAMPADD")
                            .arg(Expr::custom_keyword("SECOND"))
                            .arg(Expr::col(event_edition::Column::Ttl))
                            .arg(Expr::col(event_edition::Column::StartDate))
                            .gt(Func::cust("SYSDATE"))),
                ),
        )
        .all(conn)
        .await?;

    Ok(result)
}

/// Returns the list of the maps of the provided event edition.
pub async fn event_edition_maps<C: ConnectionTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<maps::Model>> {
    let result = maps::Entity::find()
        .join(
            sea_orm::JoinType::InnerJoin,
            event_edition_maps::Relation::OriginalMaps.def(),
        )
        .filter(
            event_edition_maps::Column::EventId
                .eq(event_id)
                .and(event_edition_maps::Column::EditionId.eq(edition_id)),
        )
        .all(conn)
        .await?;
    Ok(result)
}

/// Returns the optional event bound to the provided handle.
// TODO: remove useless wrapper
pub async fn get_event_by_handle<C: ConnectionTrait>(
    conn: &C,
    handle: &str,
) -> RecordsResult<Option<event::Model>> {
    let r = event::Entity::find()
        .filter(event::Column::Handle.eq(handle))
        .one(conn)
        .await?;
    Ok(r)
}

/// Returns the optional edition bound to the provided event.
///
/// ## Parameters
///
/// * `event_id`: the database ID of the event.
/// * `edition_id` the ID of the edition bound to this event.
// TODO: remove useless wrapper
pub async fn get_edition_by_id<C: ConnectionTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Option<event_edition::Model>> {
    let r = event_edition::Entity::find()
        .filter(
            event_edition::Column::EventId
                .eq(event_id)
                .and(event_edition::Column::Id.eq(edition_id)),
        )
        .one(conn)
        .await?;
    Ok(r)
}

/// Returns the list of the categories of the provided event edition.
///
/// ## Parameters
///
/// * `event_id`: the database ID of the event.
/// * `edition_id` the ID of the edition bound to this event.
pub async fn get_categories_by_edition_id<C: ConnectionTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<event_category::Model>> {
    let r = event_category::Entity::find()
        .reverse_join(event_edition_categories::Entity)
        .filter(
            event_edition_categories::Column::EventId
                .eq(event_id)
                .and(event_edition_categories::Column::EditionId.eq(edition_id)),
        )
        .order_by_asc(event_category::Column::Id)
        .all(conn)
        .await?;

    Ok(r)
}

/// Represents the medal times, in milliseconds.
#[derive(async_graphql::SimpleObject, Clone, Copy)]
pub struct MedalTimes {
    /// The time of the bronze medal.
    pub bronze_time: i32,
    /// The time of the silver medal.
    pub silver_time: i32,
    /// The time of the gold medal.
    pub gold_time: i32,
    /// The time of the champion/author medal.
    pub champion_time: i32,
}

/// Returns the medal times of the provided map bound to the event edition.
///
/// ## Parameters
///
/// * `event_id`: the database ID of the event.
/// * `edition_id` the ID of the edition bound to this event.
/// * `map_id`: the database ID of the map.
pub async fn get_medal_times_of<C: ConnectionTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
    map_id: u32,
) -> RecordsResult<Option<MedalTimes>> {
    let (bronze_time, silver_time, gold_time, champion_time) = event_edition_maps::Entity::find()
        .filter(
            event_edition_maps::Column::EventId
                .eq(event_id)
                .and(event_edition_maps::Column::EditionId.eq(edition_id))
                .and(event_edition_maps::Column::MapId.eq(map_id)),
        )
        .select_only()
        .columns([
            event_edition_maps::Column::BronzeTime,
            event_edition_maps::Column::SilverTime,
            event_edition_maps::Column::GoldTime,
            event_edition_maps::Column::AuthorTime,
        ])
        .into_tuple()
        .one(conn)
        .await?
        .unwrap_or_default();

    let Some(bronze_time) = bronze_time else {
        return Ok(None);
    };
    let Some(silver_time) = silver_time else {
        return Ok(None);
    };
    let Some(gold_time) = gold_time else {
        return Ok(None);
    };
    let Some(champion_time) = champion_time else {
        return Ok(None);
    };

    Ok(Some(MedalTimes {
        bronze_time,
        silver_time,
        gold_time,
        champion_time,
    }))
}

/// Returns the admins/authors of the provided event edition.
///
/// ## Parameters
///
/// * `event_id`: the database ID of the event.
/// * `edition_id` the ID of the edition bound to this event.
pub async fn get_admins_of<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<impl Stream<Item = Result<players::Model, DbErr>> + '_> {
    let r = players::Entity::find()
        .filter(
            players::Column::Id.in_subquery(
                Query::select()
                    .from(event_edition_admins::Entity)
                    .and_where(
                        event_edition_admins::Column::EventId
                            .eq(event_id)
                            .and(event_edition_admins::Column::EditionId.eq(edition_id)),
                    )
                    .column(event_edition_admins::Column::PlayerId)
                    .take(),
            ),
        )
        .stream(conn)
        .await?;
    Ok(r)
}

/// Returns the event editions which contain the map with the provided ID.
///
/// ## Parameters
///
/// * `map_id`: the ID of the map the event editions should contain.
///
/// ## Return
///
/// This function returns three things:
///
/// 1. The ID of the event
/// 2. The ID of the event edition
/// 3. The ID of the original map bound to the provided map ID. This can be null if the map doesn't
///    have an original map.
pub async fn get_editions_which_contain<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    map_id: u32,
) -> RecordsResult<impl Stream<Item = Result<(u32, u32, Option<u32>), DbErr>> + '_> {
    let r = event_edition_maps::Entity::find()
        .inner_join(event_edition::Entity)
        .filter(
            event_edition::Column::SaveNonEventRecord
                .ne(0)
                .and(event_edition_maps::Column::MapId.eq(map_id)),
        )
        .columns([
            event_edition_maps::Column::EventId,
            event_edition_maps::Column::EditionId,
            event_edition_maps::Column::OriginalMapId,
        ])
        .into_tuple()
        .stream(conn)
        .await?;
    Ok(r)
}

/// The event map retrieved from the [`have_event_edition_with_map`][1] function.
///
/// [1]: crate::must::have_event_edition_with_map
#[derive(FromQueryResult)]
pub struct EventMap {
    /// The map of the event edition.
    ///
    /// For example for the Benchmark, this would be a map with a UID finishing with `_benchmark`.
    #[sea_orm(nested)]
    pub map: maps::Model,
    /// The optional ID of the original map.
    ///
    /// For example for the Benchmark, this would be the ID of the map with a normal UID.
    pub original_map_id: Option<u32>,
}

/// Returns the map bound to an event edition from its UID or its original version UID.
///
/// ## Parameters
///
/// * `map_uid`: the UID of the map.
/// * `event_id`: the ID of the event.
/// * `edition_id`: the ID of its edition.
///
/// ## Return
///
/// For example for the Benchmark, with `map_uid` as `"X"` or `"X_benchmark"`, the function returns
/// the map with the UID `X_benchmark`, and the ID of the map with UID `X`.
pub async fn get_map_in_edition<C: ConnectionTrait>(
    conn: &C,
    map_uid: &str,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Option<EventMap>> {
    let map = event_edition_maps::Entity::find()
        .join_as(
            sea_orm::JoinType::InnerJoin,
            event_edition_maps::Relation::Maps.def(),
            "map",
        )
        .join_as(
            sea_orm::JoinType::InnerJoin,
            event_edition_maps::Relation::OriginalMaps
                .def()
                .condition_type(sea_orm::sea_query::ConditionType::Any)
                .on_condition(|_left, right| {
                    Expr::col((right, maps::Column::Id))
                        .equals(event_edition_maps::Column::MapId)
                        .into_condition()
                }),
            "original_map",
        )
        .filter(
            event_edition_maps::Column::EventId
                .eq(event_id)
                .and(event_edition_maps::Column::EditionId.eq(edition_id))
                .and(Expr::col(("original_map", maps::Column::GameId)).eq(map_uid))
                .and(
                    Expr::col(("map", maps::Column::Id))
                        .equals(("original_map", maps::Column::Id))
                        .or(event_edition_maps::Column::TransitiveSave.ne(0)),
                ),
        )
        .select_only()
        .expr(Expr::col(("map", Asterisk)))
        .column(event_edition_maps::Column::OriginalMapId)
        .into_model()
        .one(conn)
        .await?;

    Ok(map)
}
