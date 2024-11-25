//! This module contains anything related to ShootMania Obstacle events in this library.

use futures::{Stream, TryStreamExt as _};
use sqlx::{pool::PoolConnection, MySql, MySqlConnection};

use crate::{
    context::{HasEventIds, HasMySqlConnection},
    error::RecordsResult,
    models,
};

/// Represents an optional event in a certain context.
///
/// This is used by the various items of the API, because we use a different source to retrieve
/// the player records for an event.
///
/// This type isn't used as is, but rather with either [`OptEvent`] or [`OptEventIds`].
#[derive(Clone, Copy)]
pub struct OptEventImpl<Ev, Ed>(
    /// The inner type, which is simply an optional couple of references to an event and its edition.
    pub Option<(Ev, Ed)>,
);

impl<Ev, Ed> Default for OptEventImpl<Ev, Ed> {
    fn default() -> Self {
        Self(None)
    }
}

/// A specialization of the [`OptEventImpl`] type with references to the event models.
pub type OptEvent<'ev, 'ed> = OptEventImpl<&'ev models::Event, &'ed models::EventEdition>;

/// A specialization of the [`OptEventImpl`] type with the event ID and its edition ID.
pub type OptEventIds = OptEventImpl<u32, u32>;

impl<Ev, Ed> OptEventImpl<Ev, Ed> {
    /// Constructs a new [`OptEvent`] with the provided event and edition.
    ///
    /// If there are no event, use the [`Default`] implementation.
    pub fn new(event: Ev, edition: Ed) -> Self {
        Self(Some((event, edition)))
    }

    /// Returns the fragments used to build an SQL query to retrieve records bound
    /// to a specific event.
    ///
    /// The first output is the name of the SQL view which contains the records, and the second one
    /// is the condition fragment (in `WHERE`) which filters the records for the specific event.
    ///
    /// If there are no event, the fragments result in the default behavior.
    ///
    /// ## Example usage
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # use records_lib::event::OptEvent;
    /// let event = OptEvent::default();
    /// let (view_name, and_event) = event.get_view();
    /// let query = format!("select * from {view_name} r where ... {and_event}");
    /// let query = sqlx::query(&query);
    /// let query = if let Some((event, edition)) = event.0 {
    ///     query.bind(event.id).bind(edition.id)
    /// } else {
    ///     query
    /// };
    /// # Ok(())
    /// # }
    /// ```
    pub fn get_view(&self) -> (&'static str, &'static str) {
        if self.0.is_some() {
            (
                "global_event_records",
                "and r.event_id = ? and r.edition_id = ?",
            )
        } else {
            ("global_records", "")
        }
    }

    /// Returns the fragments used to build an SQL query to retrieve records bound
    /// to a specific event.
    ///
    /// The first output is the inner join to the SQL table which contains the records in events,
    /// and the second one is the condition fragment (in `WHERE`) which filters the records
    /// for the specific event.
    ///
    /// If there are no event, the fragments are empty.
    ///
    /// ## Example usage
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # use records_lib::event::OptEvent;
    /// let event = OptEvent::default();
    /// let (join_event, and_event) = event.get_join();
    /// let query = format!("select * from records r {join_event} where ... {and_event}");
    /// let query = sqlx::query(&query);
    /// let query = if let Some((event, edition)) = event.0 {
    ///     query.bind(event.id).bind(edition.id)
    /// } else {
    ///     query
    /// };
    /// # Ok(())
    /// # }
    /// ```
    pub fn get_join(&self) -> (&'static str, &'static str) {
        self.0
            .is_some()
            .then_some((
                "inner join event_edition_records eer on eer.record_id = r.record_id",
                "and eer.event_id = ? and eer.edition_id = ?",
            ))
            .unwrap_or_default()
    }
}

impl OptEvent<'_, '_> {
    /// Returns an instance of [`OptEventIds`] with the IDs of the referenced event edition.
    pub fn to_ids(&self) -> OptEventIds {
        OptEventImpl(self.0.map(|(ev, ed)| (ev.id, ed.id)))
    }
}

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
    pub event: models::Event,
}

#[derive(sqlx::FromRow)]
struct RawSqlEventListItem {
    handle: String,
    last_edition_id: u32,
    #[sqlx(flatten)]
    event: models::Event,
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
pub async fn event_list<C: HasMySqlConnection>(mut ctx: C) -> RecordsResult<Vec<EventListItem>> {
    sqlx::query_as::<_, RawSqlEventListItem>(
        "select ev.handle as handle, max(ee.id) as last_edition_id, ev.*
        from event ev
        inner join event_edition ee on ev.id = ee.event_id
        inner join event_edition_maps eem on ee.id = eem.edition_id and ee.event_id = eem.event_id
        where ee.start_date < sysdate()
            and (ee.ttl is null or ee.start_date + interval ee.ttl second > sysdate())
        group by ev.id, ev.handle
        order by ev.id",
    )
    .fetch(ctx.get_mysql_conn())
    .map_ok(From::from)
    .map_err(From::from)
    .try_collect()
    .await
}

/// Returns the list of event editions bound to the provided event handle.
pub async fn event_editions_list<C: HasMySqlConnection>(
    mut ctx: C,
    event_handle: &str,
) -> RecordsResult<Vec<models::EventEdition>> {
    let res = sqlx::query_as(
        "select ee.* from event_edition ee
        inner join event e on ee.event_id = e.id
        where e.handle = ? and ee.start_date < sysdate()
            and (ee.ttl is null or ee.start_date + interval ee.ttl second > sysdate())",
    )
    .bind(event_handle)
    .fetch_all(ctx.get_mysql_conn())
    .await?;
    Ok(res)
}

/// Returns the list of the maps of the provided event edition.
pub async fn event_edition_maps<C: HasMySqlConnection + HasEventIds>(
    mut ctx: C,
) -> RecordsResult<Vec<models::Map>> {
    let (event_id, edition_id) = ctx.get_event_ids();
    sqlx::query_as(
        "select m.* from maps m
        inner join event_edition_maps eem on m.id = eem.map_id
        where eem.event_id = ? and eem.edition_id = ?",
    )
    .bind(event_id)
    .bind(edition_id)
    .fetch_all(ctx.get_mysql_conn())
    .await
    .map_err(From::from)
}

/// Returns the optional event bound to the provided handle.
pub async fn get_event_by_handle(
    db: &mut MySqlConnection,
    handle: &str,
) -> RecordsResult<Option<models::Event>> {
    let r = sqlx::query_as("SELECT * FROM event WHERE handle = ?")
        .bind(handle)
        .fetch_optional(db)
        .await?;
    Ok(r)
}

/// Returns the optional edition bound to the provided event.
///
/// ## Parameters
///
/// * `event_id`: the database ID of the event.
/// * `edition_id` the ID of the edition bound to this event.
pub async fn get_edition_by_id(
    db: &mut MySqlConnection,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Option<models::EventEdition>> {
    let r = sqlx::query_as(
        "SELECT * FROM event_edition
        WHERE event_id = ? AND id = ?",
    )
    .bind(event_id)
    .bind(edition_id)
    .fetch_optional(db)
    .await?;
    Ok(r)
}

/// Returns the list of the categories of the provided event edition.
///
/// ## Parameters
///
/// * `event_id`: the database ID of the event.
/// * `edition_id` the ID of the edition bound to this event.
pub async fn get_categories_by_edition_id(
    db: &mut MySqlConnection,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Vec<models::EventCategory>> {
    let r = sqlx::query_as(
        "select ec.* from event_category ec
        inner join event_edition_categories eec on ec.id = eec.category_id
        where eec.event_id = ? and eec.edition_id = ?
        order by ec.id asc",
    )
    .bind(event_id)
    .bind(edition_id)
    .fetch_all(db)
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
pub async fn get_medal_times_of<E: for<'c> sqlx::Executor<'c, Database = sqlx::MySql>>(
    db: E,
    event_id: u32,
    edition_id: u32,
    map_id: u32,
) -> RecordsResult<Option<MedalTimes>> {
    let (bronze_time, silver_time, gold_time, champion_time) = sqlx::query_as(
        "select eem.bronze_time, eem.silver_time, eem.gold_time, eem.author_time
        from event_edition_maps eem
        where eem.map_id = ? and eem.event_id = ? and eem.edition_id = ?",
    )
    .bind(map_id)
    .bind(event_id)
    .bind(edition_id)
    .fetch_optional(db)
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
pub fn get_admins_of(
    db: &mut PoolConnection<MySql>,
    event_id: u32,
    edition_id: u32,
) -> impl Stream<Item = sqlx::Result<models::Player>> + '_ {
    sqlx::query_as(
        "SELECT * FROM players
            WHERE id IN (
                SELECT player_id FROM event_edition_admins
                WHERE event_id = ? AND edition_id = ?
            )",
    )
    .bind(event_id)
    .bind(edition_id)
    .fetch(&mut **db)
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
pub fn get_editions_which_contain(
    db: &mut MySqlConnection,
    map_id: u32,
) -> impl Stream<Item = sqlx::Result<(u32, u32, Option<u32>)>> + '_ {
    sqlx::query_as(
        "select eem.event_id, eem.edition_id, original_map_id from event_edition_maps eem
            inner join event_edition ee on ee.event_id = eem.event_id and ee.id = eem.edition_id
            where ee.save_non_event_record and map_id = ?",
    )
    .bind(map_id)
    .fetch(db)
}

/// The event map retrieved from the [`have_event_edition_with_map`][1] function.
///
/// [1]: crate::must::have_event_edition_with_map
#[derive(sqlx::FromRow)]
pub struct EventMap {
    /// The map of the event edition.
    ///
    /// For example for the Benchmark, this would be a map with a UID finishing with `_benchmark`.
    #[sqlx(flatten)]
    pub map: models::Map,
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
pub async fn get_map_in_edition(
    db: &mut MySqlConnection,
    map_uid: &str,
    event_id: u32,
    edition_id: u32,
) -> RecordsResult<Option<EventMap>> {
    let map = sqlx::query_as(
        "select m.*, eem.original_map_id from event_edition_maps eem
        inner join maps m on m.id = eem.map_id
        inner join maps om on om.id in (eem.map_id, eem.original_map_id)
        where eem.event_id = ? and eem.edition_id = ? and om.game_id = ?
            and (m.id = om.id or eem.transitive_save)",
    )
    .bind(event_id)
    .bind(edition_id)
    .bind(map_uid)
    .fetch_optional(db)
    .await?;

    Ok(map)
}
