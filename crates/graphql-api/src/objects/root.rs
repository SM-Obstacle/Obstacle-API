use std::{collections::HashMap, fmt};

use async_graphql::{
    ID, OutputType,
    connection::{self, CursorType},
};
use deadpool_redis::redis::{AsyncCommands, ToRedisArgs};
use entity::{event as event_entity, event_edition, global_records, maps, players, records};
use records_lib::{
    Database, RedisConnection, RedisPool, internal, must,
    opt_event::OptEvent,
    ranks,
    redis_key::{map_ranking, player_ranking},
    sync,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbConn, EntityTrait, Order, QueryFilter as _, QueryOrder as _,
    QuerySelect as _, QueryTrait, Select, StreamTrait, TransactionTrait,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func},
};

use crate::{
    cursors::{ConnectionParameters, F64Cursor, RecordDateCursor, TextCursor},
    error::{self, ApiGqlError, GqlResult},
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
        sort::UnorderedRecordSort,
        sort_order::SortOrder,
        sort_state::SortState,
        sortable_fields::UnorderedRecordSortableField,
    },
    utils::{
        page_input::{PaginationDirection, PaginationInput, apply_cursor_input},
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

pub(crate) async fn get_connection<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    connection_parameters: ConnectionParameters,
    event: OptEvent<'_>,
    sort: Option<UnorderedRecordSort>,
    base_query: Select<global_records::Entity>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let pagination_input = <PaginationInput>::try_from_input(connection_parameters)?;

    let mut query = base_query
        .clone()
        .cursor_by(global_records::Column::RecordDate);

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

    let (mut connection, records) = match pagination_input.dir {
        PaginationDirection::After { cursor } => {
            let records = query
                .first(pagination_input.limit as u64 + 1)
                .all(conn)
                .await?;
            (
                connection::Connection::new(
                    cursor.is_some(),
                    records.len() > pagination_input.limit,
                ),
                itertools::Either::Left(records.into_iter().take(pagination_input.limit)),
            )
        }
        PaginationDirection::Before { .. } => {
            let records = query
                .last(pagination_input.limit as u64 + 1)
                .all(conn)
                .await?;
            let amount_to_skip = records.len().saturating_sub(pagination_input.limit);
            (
                connection::Connection::new(records.len() > pagination_input.limit, true),
                itertools::Either::Right(records.into_iter().skip(amount_to_skip)),
            )
        }
    };

    connection.edges.reserve(records.len());

    let mut redis_conn = redis_pool.get().await?;

    for record in records {
        let rank = ranks::get_rank(&mut redis_conn, record.map_id, record.time, event).await?;

        connection.edges.push(connection::Edge::new(
            ID(RecordDateCursor(record.record_date.and_utc()).encode_cursor()),
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
    connection_parameters: ConnectionParameters,
    event: OptEvent<'_>,
    sort: Option<UnorderedRecordSort>,
    filter: Option<RecordsFilter>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let base_query = apply_filter(global_records::Entity::find(), filter.as_ref());

    get_connection(
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

    async fn players(
        &self,
        ctx: &async_graphql::Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
        filter: Option<PlayersFilter>,
    ) -> GqlResult<connection::Connection<ID, PlayerWithScore>> {
        let db = ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        connection::query(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                get_players_connection(
                    &db.sql_conn,
                    &mut redis_conn,
                    ConnectionParameters {
                        after,
                        before,
                        first,
                        last,
                    },
                    filter,
                )
                .await
            },
        )
        .await
        .map_err(error::map_gql_err)
    }

    async fn maps(
        &self,
        ctx: &async_graphql::Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
        filter: Option<MapsFilter>,
    ) -> GqlResult<connection::Connection<ID, MapWithScore>> {
        let db = ctx.data_unchecked::<Database>();
        let mut redis_conn = db.redis_pool.get().await?;

        connection::query(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                get_maps_connection(
                    &db.sql_conn,
                    &mut redis_conn,
                    ConnectionParameters {
                        after,
                        before,
                        first,
                        last,
                    },
                    filter,
                )
                .await
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

        connection::query(
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

/// If a filter is provided, the result is ordered by the login of the players, so the cursors become
/// based on them. Otherwise, the result is ordered by the score of the players.
async fn get_players_connection<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    ConnectionParameters {
        after,
        before,
        first,
        last,
    }: ConnectionParameters,
    filter: Option<PlayersFilter>,
) -> GqlResult<connection::Connection<ID, PlayerWithScore>> {
    match filter {
        Some(filter) => {
            let after = after
                .map(|after| {
                    TextCursor::decode_cursor(&after.0)
                        .map_err(|e| ApiGqlError::from_cursor_decode_error("after", after.0, e))
                })
                .transpose()?;

            let before = before
                .map(|before| {
                    TextCursor::decode_cursor(&before.0)
                        .map_err(|e| ApiGqlError::from_cursor_decode_error("before", before.0, e))
                })
                .transpose()?;

            let limit = first
                .map(|f| f as u64)
                .or(last.map(|l| l as _))
                .map(|l| l.min(100))
                .unwrap_or(50);

            let has_previous_page = after.is_some();

            let query = players::Entity::find()
                .apply_if(filter.player_login, |query, login| {
                    query.filter(players::Column::Login.like(format!("%{login}%")))
                })
                .apply_if(filter.player_name, |query, name| {
                    query.filter(
                        Func::cust("rm_mp_style")
                            .arg(Expr::col((players::Entity, players::Column::Name)))
                            .like(format!("%{name}%")),
                    )
                })
                .apply_if(after, |query, after| {
                    query.filter(players::Column::Login.gt(after.0))
                })
                .apply_if(before, |query, before| {
                    query.filter(players::Column::Login.lt(before.0))
                })
                .order_by(
                    players::Column::Login,
                    if first.is_some() {
                        Order::Asc
                    } else {
                        Order::Desc
                    },
                )
                .limit(limit + 1)
                .all(conn)
                .await?;

            let mut connection =
                connection::Connection::new(has_previous_page, query.len() > limit as _);
            connection.edges.reserve(limit as _);

            for player in query.into_iter().take(limit as _) {
                let score = redis_conn.zscore(player_ranking(), player.id).await?;
                let rank: i32 = redis_conn.zrevrank(player_ranking(), player.id).await?;
                connection.edges.push(connection::Edge::new(
                    ID(TextCursor(player.login.clone()).encode_cursor()),
                    PlayerWithScore {
                        score,
                        rank: rank + 1,
                        player: player.into(),
                    },
                ));
            }

            Ok(connection)
        }
        None => {
            let has_previous_page = after.is_some();

            let player_with_scores =
                build_scores(redis_conn, player_ranking(), after, before, first, last).await?;

            let players = players::Entity::find()
                .filter(players::Column::Id.is_in(player_with_scores.keys().copied()))
                .all(conn)
                .await?;

            let limit = first.or(last).unwrap_or(50);
            let mut connection = connection::Connection::<_, PlayerWithScore>::new(
                has_previous_page,
                players.len() > limit,
            );

            build_connections(
                redis_conn,
                player_ranking(),
                &mut connection,
                players,
                limit,
                &player_with_scores,
            )
            .await?;

            if last.is_some() {
                connection.edges.sort_by_key(|edge| -edge.node.rank);
            } else {
                connection.edges.sort_by_key(|edge| edge.node.rank);
            }

            Ok(connection)
        }
    }
}

/// If a filter is provided, the result is ordered by the UID of the maps, so the cursors become
/// based on them. Otherwise, the result is ordered by the score of the maps.
async fn get_maps_connection<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    ConnectionParameters {
        after,
        before,
        first,
        last,
    }: ConnectionParameters,
    filter: Option<MapsFilter>,
) -> GqlResult<connection::Connection<ID, MapWithScore>> {
    match filter {
        Some(filter) => {
            let after = after
                .map(|after| {
                    TextCursor::decode_cursor(&after.0)
                        .map_err(|e| ApiGqlError::from_cursor_decode_error("after", after.0, e))
                })
                .transpose()?;

            let before = before
                .map(|before| {
                    TextCursor::decode_cursor(&before.0)
                        .map_err(|e| ApiGqlError::from_cursor_decode_error("before", before.0, e))
                })
                .transpose()?;

            let limit = first
                .map(|f| f as u64)
                .or(last.map(|l| l as _))
                .map(|l| l.min(100))
                .unwrap_or(50);

            let has_previous_page = after.is_some();

            let query = maps::Entity::find()
                .apply_if(filter.author, |query, filter| {
                    query
                        .inner_join(players::Entity)
                        .apply_if(filter.player_login, |query, login| {
                            query.filter(players::Column::Login.like(format!("%{login}%")))
                        })
                        .apply_if(filter.player_name, |query, name| {
                            query.filter(
                                Func::cust("rm_mp_style")
                                    .arg(Expr::col((players::Entity, players::Column::Name)))
                                    .like(format!("%{name}%")),
                            )
                        })
                })
                .apply_if(filter.map_uid, |query, uid| {
                    query.filter(maps::Column::GameId.like(format!("%{uid}%")))
                })
                .apply_if(filter.map_name, |query, name| {
                    query.filter(
                        Func::cust("rm_mp_style")
                            .arg(Expr::col((maps::Entity, maps::Column::Name)))
                            .like(format!("%{name}%")),
                    )
                })
                .apply_if(after, |query, after| {
                    query.filter(maps::Column::GameId.gt(after.0))
                })
                .apply_if(before, |query, before| {
                    query.filter(maps::Column::GameId.lt(before.0))
                })
                .order_by(
                    maps::Column::GameId,
                    if first.is_some() {
                        Order::Asc
                    } else {
                        Order::Desc
                    },
                )
                .limit(limit + 1)
                .all(conn)
                .await?;

            let mut connection =
                connection::Connection::new(has_previous_page, query.len() > limit as _);
            connection.edges.reserve(limit as _);

            for map in query.into_iter().take(limit as _) {
                let score = redis_conn.zscore(map_ranking(), map.id).await?;
                let rank: i32 = redis_conn.zrevrank(map_ranking(), map.id).await?;
                connection.edges.push(connection::Edge::new(
                    ID(TextCursor(map.game_id.clone()).encode_cursor()),
                    MapWithScore {
                        score,
                        rank: rank + 1,
                        map: map.into(),
                    },
                ));
            }

            Ok(connection)
        }
        None => {
            let has_previous_page = after.is_some();

            let map_with_scores =
                build_scores(redis_conn, map_ranking(), after, before, first, last).await?;

            let maps = maps::Entity::find()
                .filter(maps::Column::Id.is_in(map_with_scores.keys().copied()))
                .all(conn)
                .await?;

            let limit = first.or(last).unwrap_or(50);
            let mut connection = connection::Connection::<_, MapWithScore>::new(
                has_previous_page,
                maps.len() > limit,
            );

            build_connections(
                redis_conn,
                map_ranking(),
                &mut connection,
                maps,
                limit,
                &map_with_scores,
            )
            .await?;

            if last.is_some() {
                connection.edges.sort_by_key(|edge| -edge.node.rank);
            } else {
                connection.edges.sort_by_key(|edge| edge.node.rank);
            }

            Ok(connection)
        }
    }
}

async fn build_connections<K, I, T, U>(
    redis_conn: &mut RedisConnection,
    redis_key: K,
    connection: &mut connection::Connection<ID, T>,
    items: I,
    limit: usize,
    scores: &HashMap<u32, f64>,
) -> GqlResult<()>
where
    K: ToRedisArgs + Sync,
    I: IntoIterator<Item = U>,
    U: HasId,
    T: Ranking<Item = U> + OutputType,
{
    connection.edges.reserve(limit);

    for map in items.into_iter().take(limit) {
        let score = scores
            .get(&map.get_id())
            .copied()
            .ok_or_else(|| internal!("missing score entry for ID {}", map.get_id()))?;
        let rank: i32 = redis_conn.zrevrank(&redis_key, map.get_id()).await?;
        connection.edges.push(connection::Edge::new(
            ID(F64Cursor(score).encode_cursor()),
            T::from_node(rank + 1, score, map),
        ));
    }

    Ok(())
}

trait Ranking {
    type Item;

    fn from_node(rank: i32, score: f64, node: Self::Item) -> Self;
}

trait HasId {
    fn get_id(&self) -> u32;
}

impl HasId for maps::Model {
    fn get_id(&self) -> u32 {
        self.id
    }
}

impl HasId for players::Model {
    fn get_id(&self) -> u32 {
        self.id
    }
}

impl Ranking for MapWithScore {
    type Item = maps::Model;

    fn from_node(rank: i32, score: f64, node: Self::Item) -> Self {
        Self {
            rank,
            score,
            map: node.into(),
        }
    }
}

impl Ranking for PlayerWithScore {
    type Item = players::Model;

    fn from_node(rank: i32, score: f64, node: Self::Item) -> Self {
        Self {
            rank,
            score,
            player: node.into(),
        }
    }
}

async fn build_scores<K>(
    redis_conn: &mut deadpool_redis::Connection,
    redis_key: K,
    after: Option<ID>,
    before: Option<ID>,
    first: Option<usize>,
    last: Option<usize>,
) -> Result<HashMap<u32, f64>, ApiGqlError>
where
    K: ToRedisArgs + Sync + fmt::Display,
{
    let ids: Vec<String> = match after {
        Some(after) => {
            let decoded = F64Cursor::decode_cursor(&after.0)
                .map_err(|e| ApiGqlError::from_cursor_decode_error("after", after.0, e))?;
            let first = first.map(|f| f.min(100)).unwrap_or(50);
            redis_conn
                .zrevrangebyscore_limit_withscores(&redis_key, decoded.0, "-inf", 1, first as _)
                .await?
        }
        None => match before {
            Some(before) => {
                let decoded = F64Cursor::decode_cursor(&before.0)
                    .map_err(|e| ApiGqlError::from_cursor_decode_error("before", before.0, e))?;
                let last = last.map(|l| l.min(100)).unwrap_or(50);
                redis_conn
                    .zrangebyscore_limit_withscores(&redis_key, decoded.0, "+inf", 1, last as _)
                    .await?
            }
            None => match last {
                Some(last) => {
                    redis_conn
                        .zrange_withscores(&redis_key, 0, last.min(100) as isize - 1)
                        .await?
                }
                None => {
                    let first = first.map(|f| f.min(100)).unwrap_or(50);
                    redis_conn
                        .zrevrange_withscores(&redis_key, 0, first as isize - 1)
                        .await?
                }
            },
        },
    };
    let (ids, _) = ids.as_chunks::<2>();
    let with_scores = ids
        .iter()
        .map(|[id, score]| {
            let id = id
                .parse::<u32>()
                .map_err(|e| internal!("got invalid ID `{id}` in {redis_key} ZSET: {e}"))?;
            let score = score
                .parse::<f64>()
                .map_err(|e| internal!("got invalid score `{score}` in {redis_key} ZSET: {e}"))?;
            GqlResult::Ok((id, score))
        })
        .collect::<GqlResult<HashMap<_, _>>>()?;
    Ok(with_scores)
}
