use std::borrow::Cow;

use async_graphql::{
    ID,
    connection::{self, CursorType},
};
use deadpool_redis::redis::{AsyncCommands, ToRedisArgs};
use entity::{
    event as event_entity, event_edition, event_edition_records, functions, global_records, maps,
    players, records,
};
use records_lib::{
    Database, RedisConnection, RedisPool, internal, must,
    opt_event::OptEvent,
    ranks,
    redis_key::{MapRanking, PlayerRanking, map_ranking, player_ranking},
    sync,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbConn, EntityTrait, FromQueryResult, Identity, QueryFilter as _,
    QueryOrder as _, QuerySelect, RelationTrait as _, Select, SelectModel, StreamTrait,
    TransactionTrait,
    prelude::Expr,
    sea_query::{Asterisk, ExprTrait as _, Func, IntoIden, IntoValueTuple, SelectStatement},
};

use crate::{
    cursors::{
        ConnectionParameters, F64Cursor, RecordDateCursor, TextCursor, expr_tuple::IntoExprTuple,
        query_builder::CursorQueryBuilder, query_trait::CursorPaginable,
    },
    error::{self, ApiGqlError, CursorDecodeError, CursorDecodeErrorKind, GqlResult},
    objects::{
        event::Event,
        event_edition::EventEdition,
        map::Map,
        map_filter::MapsFilter,
        map_with_score::MapWithScore,
        mappack::{self, Mappack},
        player::Player,
        player_filter::PlayersFilter,
        player_with_score::PlayerWithScore,
        ranked_record::RankedRecord,
        records_filter::RecordsFilter,
        sort::{PlayerMapRankingSort, UnorderedRecordSort},
        sort_order::SortOrder,
        sort_state::SortState,
        sortable_fields::{PlayerMapRankingSortableField, UnorderedRecordSortableField},
    },
    utils::{
        connection_input::{ConnectionInput, ConnectionInputBuilder},
        page_input::{PaginationInput, apply_cursor_input},
        pagination_result::{PaginationResult, get_paginated},
        records_filter::apply_filter,
    },
};

pub struct QueryRoot;

async fn get_record<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    record_id: u32,
    event: OptEvent<'_>,
) -> GqlResult<RankedRecord> {
    let record = records::Entity::find_by_id(record_id).one(conn).await?;

    let Some(record) = record else {
        return Err(ApiGqlError::from_record_not_found_error(record_id));
    };

    let mut redis_conn = redis_pool.get().await?;

    let out = records::RankedRecord {
        rank: ranks::get_rank(&mut redis_conn, record.map_id, record.time, event).await?,
        record,
    }
    .into();

    Ok(out)
}

async fn get_records<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    date_sort_by: Option<SortState>,
    event: OptEvent<'_>,
) -> GqlResult<Vec<RankedRecord>> {
    let records = global_records::Entity::find()
        .order_by(
            global_records::Column::RecordDate,
            match date_sort_by {
                Some(SortState::Reverse) => sea_orm::Order::Asc,
                _ => sea_orm::Order::Desc,
            },
        )
        .limit(100)
        .all(conn)
        .await?;

    let mut ranked_records = Vec::with_capacity(records.len());

    let mut redis_conn = redis_pool.get().await?;

    for record in records {
        let rank = ranks::get_rank(&mut redis_conn, record.map_id, record.time, event).await?;

        ranked_records.push(
            records::RankedRecord {
                rank,
                record: record.into(),
            }
            .into(),
        );
    }

    Ok(ranked_records)
}

pub(crate) async fn get_records_connection_impl<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    connection_parameters: ConnectionParameters<RecordDateCursor>,
    event: OptEvent<'_>,
    sort: Option<UnorderedRecordSort>,
    base_query: Select<global_records::Entity>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let pagination_input = PaginationInput::try_from_input(connection_parameters)?;

    let mut query = base_query.paginate_cursor_by((
        global_records::Column::RecordDate,
        global_records::Column::RecordId,
    ));

    apply_cursor_input(&mut query, &pagination_input);

    // Record dates are ordered by desc by default
    let is_sort_asc = matches!(
        sort,
        Some(UnorderedRecordSort {
            field: UnorderedRecordSortableField::Date,
            order: Some(SortOrder::Descending),
        })
    );

    if is_sort_asc {
        query.asc();
    } else {
        query.desc();
    }

    // inline get_paginated
    let PaginationResult {
        mut connection,
        iter: records,
    } = get_paginated(conn, query, &pagination_input).await?;

    connection.edges.reserve(records.len());

    let mut redis_conn = redis_pool.get().await?;

    for record in records {
        let rank = ranks::get_rank(&mut redis_conn, record.map_id, record.time, event).await?;

        connection.edges.push(connection::Edge::new(
            ID(RecordDateCursor {
                record_date: record.record_date.and_utc(),
                data: record.record_id,
            }
            .encode_cursor()),
            records::RankedRecord {
                rank,
                record: record.into(),
            }
            .into(),
        ));
    }

    Ok(connection)
}

pub(crate) async fn get_records_connection<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    connection_parameters: ConnectionParameters<RecordDateCursor>,
    event: OptEvent<'_>,
    sort: Option<UnorderedRecordSort>,
    filter: Option<RecordsFilter>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let base_query = apply_filter(global_records::Entity::find(), filter.as_ref());

    get_records_connection_impl(
        conn,
        redis_pool,
        connection_parameters,
        event,
        sort,
        base_query,
    )
    .await
}

#[async_graphql::Object]
impl QueryRoot {
    async fn event_edition_from_mx_id(
        &self,
        ctx: &async_graphql::Context<'_>,
        mx_id: i64,
    ) -> GqlResult<Option<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let edition = event_edition::Entity::find()
            .filter(event_edition::Column::MxId.eq(mx_id))
            .one(conn)
            .await?;

        Ok(match edition {
            Some(edition) => Some(EventEdition::from_inner(conn, edition).await?),
            None => None,
        })
    }

    async fn mappack(
        &self,
        ctx: &async_graphql::Context<'_>,
        mappack_id: String,
    ) -> GqlResult<Mappack> {
        let res = mappack::get_mappack(ctx, mappack_id).await?;
        Ok(res)
    }

    async fn event(&self, ctx: &async_graphql::Context<'_>, handle: String) -> GqlResult<Event> {
        let conn = ctx.data_unchecked::<DbConn>();
        let event = must::have_event_handle(conn, &handle).await?;

        Ok(event.into())
    }

    async fn events(&self, ctx: &async_graphql::Context<'_>) -> GqlResult<Vec<Event>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let r = event_entity::Entity::find()
            .reverse_join(event_edition::Entity)
            .filter(
                Expr::col(event_edition::Column::StartDate)
                    .lt(Func::cust("SYSDATE"))
                    .and(
                        event_edition::Column::Ttl
                            .is_null()
                            .or(Func::cust("SYSDATE").lt(Func::cust("TIMESTAMPADD")
                                .arg(Expr::custom_keyword("SECOND"))
                                .arg(Expr::col(event_edition::Column::Ttl))
                                .arg(Expr::col(event_edition::Column::StartDate)))),
                    ),
            )
            .group_by(event_entity::Column::Id)
            .into_model()
            .all(conn)
            .await?;

        Ok(r)
    }

    async fn trending_event_editions(
        &self,
        ctx: &async_graphql::Context<'_>,
        last_days: Option<u8>,
        limit: Option<u64>,
    ) -> GqlResult<Vec<EventEdition<'_>>> {
        let conn = ctx.data_unchecked::<DbConn>();

        let limit = limit.unwrap_or(3).min(5);
        let last_days = last_days.unwrap_or(7).min(30);

        let editions = event_edition::Entity::find()
            .reverse_join(event_edition_records::Entity)
            .join(
                sea_orm::JoinType::InnerJoin,
                event_edition_records::Relation::Records.def(),
            )
            .filter(
                Expr::current_timestamp().lt(Func::cust("TIMESTAMPADD")
                    .arg(Expr::custom_keyword("DAY"))
                    .arg(last_days)
                    .arg(Expr::col((records::Entity, records::Column::RecordDate)))),
            )
            .find_also_related(event_entity::Entity)
            .expr_as(Expr::col(Asterisk).count(), "records_count")
            .group_by(event_edition::Column::EventId)
            .group_by(event_edition::Column::Id)
            .order_by_desc(Expr::col("records_count"))
            .limit(limit)
            .all(conn)
            .await?
            .into_iter()
            .map(|(edition, event)| {
                GqlResult::Ok(EventEdition {
                    event: Cow::Owned(
                        event
                            .ok_or_else(|| {
                                internal!("event {} should be present", edition.event_id)
                            })?
                            .into(),
                    ),
                    inner: edition,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(editions)
    }

    async fn record(
        &self,
        ctx: &async_graphql::Context<'_>,
        record_id: u32,
    ) -> GqlResult<RankedRecord> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();

        sync::transaction(conn, async |txn| {
            get_record(txn, &db.redis_pool, record_id, Default::default()).await
        })
        .await
    }

    async fn map(&self, ctx: &async_graphql::Context<'_>, game_id: String) -> GqlResult<Map> {
        let conn = ctx.data_unchecked::<DbConn>();

        let opt_map = records_lib::map::get_map_from_uid(conn, &game_id).await?;

        opt_map
            .ok_or_else(|| ApiGqlError::from_map_not_found_error(game_id))
            .map(From::from)
    }

    async fn player(&self, ctx: &async_graphql::Context<'_>, login: String) -> GqlResult<Player> {
        let conn = ctx.data_unchecked::<DbConn>();

        let opt_player = players::Entity::find()
            .filter(players::Column::Login.eq(&login))
            .one(conn)
            .await?;

        opt_player
            .ok_or_else(|| ApiGqlError::from_player_not_found_error(login))
            .map(From::from)
    }

    async fn records(
        &self,
        ctx: &async_graphql::Context<'_>,
        date_sort_by: Option<SortState>,
    ) -> GqlResult<Vec<RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();
        let conn = ctx.data_unchecked::<DbConn>();

        sync::transaction(conn, async |txn| {
            get_records(txn, &db.redis_pool, date_sort_by, Default::default()).await
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn players(
        &self,
        ctx: &async_graphql::Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
        filter: Option<PlayersFilter>,
        sort: Option<PlayerMapRankingSort>,
    ) -> GqlResult<connection::Connection<ID, PlayerWithScore>> {
        let db = ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        connection::query_with(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                let input = ConnectionInputBuilder::new(ConnectionParameters {
                    after,
                    before,
                    first,
                    last,
                })
                .with_filter(filter)
                .with_sort(sort)
                .build(player_ranking());

                get_players_connection(&db.sql_conn, &mut redis_conn, input).await
            },
        )
        .await
        .map_err(error::map_gql_err)
    }

    #[allow(clippy::too_many_arguments)]
    async fn maps(
        &self,
        ctx: &async_graphql::Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
        filter: Option<MapsFilter>,
        sort: Option<PlayerMapRankingSort>,
    ) -> GqlResult<connection::Connection<ID, MapWithScore>> {
        let db = ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        connection::query_with(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                let input = ConnectionInputBuilder::new(ConnectionParameters {
                    after,
                    before,
                    first,
                    last,
                })
                .with_filter(filter)
                .with_sort(sort)
                .build(map_ranking());

                get_maps_connection(&db.sql_conn, &mut redis_conn, input).await
            },
        )
        .await
        .map_err(error::map_gql_err)
    }

    #[allow(clippy::too_many_arguments)]
    async fn records_connection(
        &self,
        ctx: &async_graphql::Context<'_>,
        #[graphql(desc = "Cursor to fetch records after (for forward pagination)")] after: Option<
            String,
        >,
        #[graphql(desc = "Cursor to fetch records before (for backward pagination)")]
        before: Option<String>,
        #[graphql(desc = "Number of records to fetch (default: 50, max: 100)")] first: Option<i32>,
        #[graphql(desc = "Number of records to fetch from the end (for backward pagination)")] last: Option<i32>,
        sort: Option<UnorderedRecordSort>,
        filter: Option<RecordsFilter>,
    ) -> GqlResult<connection::Connection<ID, RankedRecord>> {
        let db = ctx.data_unchecked::<Database>();

        connection::query_with(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                get_records_connection(
                    &db.sql_conn,
                    &db.redis_pool,
                    ConnectionParameters {
                        after,
                        before,
                        first,
                        last,
                    },
                    Default::default(),
                    sort,
                    filter,
                )
                .await
            },
        )
        .await
        .map_err(error::map_gql_err)
    }
}

pub(crate) enum PlayerMapRankingCursor {
    Name(TextCursor),
    Score(F64Cursor),
}

impl From<TextCursor> for PlayerMapRankingCursor {
    #[inline]
    fn from(value: TextCursor) -> Self {
        Self::Name(value)
    }
}

impl From<F64Cursor> for PlayerMapRankingCursor {
    #[inline]
    fn from(value: F64Cursor) -> Self {
        Self::Score(value)
    }
}

impl CursorType for PlayerMapRankingCursor {
    type Error = CursorDecodeError;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        match F64Cursor::decode_cursor(s) {
            Ok(score) => Ok(score.into()),
            Err(CursorDecodeError {
                kind: CursorDecodeErrorKind::InvalidPrefix,
            }) => match TextCursor::decode_cursor(s) {
                Ok(text) => Ok(text.into()),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    fn encode_cursor(&self) -> String {
        match self {
            PlayerMapRankingCursor::Name(text_cursor) => text_cursor.encode_cursor(),
            PlayerMapRankingCursor::Score(f64_cursor) => f64_cursor.encode_cursor(),
        }
    }
}

impl IntoExprTuple for &PlayerMapRankingCursor {
    fn into_expr_tuple(self) -> crate::cursors::expr_tuple::ExprTuple {
        match self {
            PlayerMapRankingCursor::Name(name) => IntoExprTuple::into_expr_tuple(name),
            PlayerMapRankingCursor::Score(score) => IntoExprTuple::into_expr_tuple(score),
        }
    }
}

impl IntoValueTuple for &PlayerMapRankingCursor {
    fn into_value_tuple(self) -> sea_orm::sea_query::ValueTuple {
        match self {
            PlayerMapRankingCursor::Name(name) => IntoValueTuple::into_value_tuple(name),
            PlayerMapRankingCursor::Score(score) => IntoValueTuple::into_value_tuple(score),
        }
    }
}

pub(crate) type PlayersConnectionInput<S = PlayerRanking> =
    ConnectionInput<PlayerMapRankingCursor, PlayersFilter, PlayerMapRankingSort, S>;

#[derive(FromQueryResult)]
struct PlayerWithUnstyledName {
    #[sea_orm(nested)]
    player: players::Model,
    unstyled_player_name: String,
}

pub(crate) async fn get_players_connection<C, S>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    input: PlayersConnectionInput<S>,
) -> GqlResult<connection::Connection<ID, PlayerWithScore>>
where
    C: ConnectionTrait,
    S: ToRedisArgs + Send + Sync,
{
    let pagination_input = PaginationInput::try_from_input(input.connection_parameters)?;
    let cursor_encoder = match input.sort.map(|s| s.field) {
        Some(PlayerMapRankingSortableField::Name) => |player: &PlayerWithUnstyledName| {
            TextCursor {
                text: player.unstyled_player_name.clone(),
                data: player.player.id,
            }
            .encode_cursor()
        },
        _ => |player: &PlayerWithUnstyledName| {
            F64Cursor {
                score: player.player.score,
                data: player.player.id,
            }
            .encode_cursor()
        },
    };

    let mut query = players::Entity::find().expr_as(
        functions::unstyled(players::Column::Name),
        "unstyled_player_name",
    );
    let query = SelectStatement::new()
        .expr(Expr::col(("player", Asterisk)))
        .from_subquery(QuerySelect::query(&mut query).take(), "player")
        .apply_if(input.filter, |query, filter| {
            query
                .apply_if(filter.player_login, |query, login| {
                    query.and_where(
                        Expr::col(("player", players::Column::Login)).like(format!("%{login}%")),
                    );
                })
                .apply_if(filter.player_name, |query, name| {
                    query.and_where(
                        Expr::col(("player", "unstyled_player_name")).like(format!("%{name}%")),
                    );
                });
        })
        .take();

    let mut query = match (
        pagination_input.get_cursor(),
        input.sort.as_ref().map(|s| s.field),
    ) {
        (Some(PlayerMapRankingCursor::Name(_)), _)
        | (None, Some(PlayerMapRankingSortableField::Name)) => {
            CursorQueryBuilder::<SelectModel<players::Model>>::new(
                query,
                "player".into_iden(),
                Identity::Binary(
                    "unstyled_player_name".into_iden(),
                    players::Column::Id.into_iden(),
                ),
            )
        }
        _ => CursorQueryBuilder::new(
            query,
            "player".into_iden(),
            (players::Column::Score, players::Column::Id),
        ),
    }
    .into_model::<PlayerWithUnstyledName>();

    apply_cursor_input(&mut query, &pagination_input);

    match input.sort {
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Rank,
            order: Some(SortOrder::Descending),
        }) => query.asc(),
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Rank,
            ..
        })
        | None => query.desc(),
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Name,
            order: Some(SortOrder::Descending),
        }) => query.desc(),
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Name,
            ..
        }) => query.asc(),
    };

    let PaginationResult {
        mut connection,
        iter: players,
    } = get_paginated(conn, query, &pagination_input).await?;

    connection.edges.reserve(players.len());

    for player in players {
        let rank: i32 = redis_conn.zrevrank(&input.source, player.player.id).await?;
        connection.edges.push(connection::Edge::new(
            ID((cursor_encoder)(&player)),
            PlayerWithScore {
                rank: rank + 1,
                player: player.player.into(),
            },
        ));
    }

    Ok(connection)
}

pub(crate) type MapsConnectionInput<S = MapRanking> =
    ConnectionInput<PlayerMapRankingCursor, MapsFilter, PlayerMapRankingSort, S>;

#[derive(FromQueryResult)]
struct MapWithUnstyledName {
    #[sea_orm(nested)]
    map: maps::Model,
    unstyled_map_name: String,
}

pub(crate) async fn get_maps_connection<C, S>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    input: MapsConnectionInput<S>,
) -> GqlResult<connection::Connection<ID, MapWithScore>>
where
    C: ConnectionTrait,
    S: ToRedisArgs + Send + Sync,
{
    let pagination_input = PaginationInput::try_from_input(input.connection_parameters)?;
    let cursor_encoder = match input.sort.map(|s| s.field) {
        Some(PlayerMapRankingSortableField::Name) => |map: &MapWithUnstyledName| {
            TextCursor {
                text: map.unstyled_map_name.clone(),
                data: map.map.id,
            }
            .encode_cursor()
        },
        _ => |map: &MapWithUnstyledName| {
            F64Cursor {
                score: map.map.score,
                data: map.map.id,
            }
            .encode_cursor()
        },
    };

    let mut query =
        maps::Entity::find().expr_as(functions::unstyled(maps::Column::Name), "unstyled_map_name");
    let query = SelectStatement::new()
        .expr(Expr::col(("map", Asterisk)))
        .from_subquery(QuerySelect::query(&mut query).take(), "map")
        .apply_if(input.filter, |query, filter| {
            query
                .apply_if(filter.author, |query, filter| {
                    query
                        .join_as(
                            sea_orm::JoinType::InnerJoin,
                            players::Entity,
                            "author",
                            Expr::col(("author", players::Column::Id))
                                .eq(Expr::col(("map", maps::Column::PlayerId))),
                        )
                        .apply_if(filter.player_login, |query, login| {
                            query.and_where(
                                Expr::col(("author", players::Column::Login))
                                    .like(format!("%{login}%")),
                            );
                        })
                        .apply_if(filter.player_name, |query, name| {
                            query.and_where(
                                functions::unstyled(Expr::col(("author", players::Column::Name)))
                                    .like(format!("%{name}%")),
                            );
                        });
                })
                .apply_if(filter.map_uid, |query, uid| {
                    query.and_where(
                        Expr::col(("map", maps::Column::GameId)).like(format!("%{uid}%")),
                    );
                })
                .apply_if(filter.map_name, |query, name| {
                    query.and_where(
                        Expr::col(("map", "unstyled_map_name")).like(format!("%{name}%")),
                    );
                });
        })
        .take();

    let mut query = match (
        pagination_input.get_cursor(),
        input.sort.as_ref().map(|s| s.field),
    ) {
        (Some(PlayerMapRankingCursor::Name(_)), _)
        | (_, Some(PlayerMapRankingSortableField::Name)) => {
            CursorQueryBuilder::<SelectModel<maps::Model>>::new(
                query,
                "map".into_iden(),
                Identity::Binary(
                    "unstyled_map_name".into_iden(),
                    maps::Column::Id.into_iden(),
                ),
            )
        }
        _ => CursorQueryBuilder::new(
            query,
            "map".into_iden(),
            (maps::Column::Score, maps::Column::Id),
        ),
    }
    .into_model::<MapWithUnstyledName>();

    apply_cursor_input(&mut query, &pagination_input);

    match input.sort {
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Rank,
            order: Some(SortOrder::Descending),
        }) => query.asc(),
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Rank,
            ..
        })
        | None => query.desc(),
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Name,
            order: Some(SortOrder::Descending),
        }) => query.desc(),
        Some(PlayerMapRankingSort {
            field: PlayerMapRankingSortableField::Name,
            ..
        }) => query.asc(),
    };

    let PaginationResult {
        mut connection,
        iter: maps,
    } = get_paginated(conn, query, &pagination_input).await?;

    connection.edges.reserve(maps.len());

    for map in maps {
        let rank: i32 = redis_conn.zrevrank(&input.source, map.map.id).await?;
        connection.edges.push(connection::Edge::new(
            ID((cursor_encoder)(&map)),
            MapWithScore {
                rank: rank + 1,
                map: map.map.into(),
            },
        ));
    }

    Ok(connection)
}
