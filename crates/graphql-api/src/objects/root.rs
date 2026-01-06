use async_graphql::{
    ID,
    connection::{self, CursorType},
};
use deadpool_redis::redis::{AsyncCommands, ToRedisArgs};
use entity::{event as event_entity, event_edition, global_records, maps, players, records};
use records_lib::{
    Database, RedisConnection, RedisPool, must,
    opt_event::OptEvent,
    ranks,
    redis_key::{MapRanking, PlayerRanking, map_ranking, player_ranking},
    sync,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbConn, EntityTrait, FromQueryResult, Identity, QueryFilter as _,
    QueryOrder as _, QuerySelect as _, QueryTrait, Select, StreamTrait, TransactionTrait,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func, IntoIden, IntoValueTuple},
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
        sort::{PlayerMapRankingSort, UnorderedRecordSort},
        sort_order::SortOrder,
        sort_state::SortState,
        sortable_fields::{PlayerMapRankingSortableField, UnorderedRecordSortableField},
    },
    utils::{
        page_input::{
            PaginationDirection, PaginationInput, ParsedPaginationInput, apply_cursor_input,
        },
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

pub(crate) async fn get_connection<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    redis_pool: &RedisPool,
    connection_parameters: ConnectionParameters,
    event: OptEvent<'_>,
    sort: Option<UnorderedRecordSort>,
    base_query: Select<global_records::Entity>,
) -> GqlResult<connection::Connection<ID, RankedRecord>> {
    let pagination_input = <PaginationInput>::try_from_input(connection_parameters)?;

    let mut query = base_query.cursor_by((
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

    let PaginationResult {
        mut connection,
        iter: records,
    } = get_paginated(conn, query, &pagination_input).await?;

    connection.edges.reserve(records.len());

    let mut redis_conn = redis_pool.get().await?;

    for record in records {
        let rank = ranks::get_rank(&mut redis_conn, record.map_id, record.time, event).await?;

        connection.edges.push(connection::Edge::new(
            ID(RecordDateCursor(record.record_date.and_utc(), record.record_id).encode_cursor()),
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

        connection::query(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                let input = <PlayersConnectionInput>::new(ConnectionParameters {
                    after,
                    before,
                    first,
                    last,
                });
                let input = match filter {
                    Some(filter) => input.with_filter(filter),
                    None => input,
                };
                let input = match sort {
                    Some(sort) => input.with_sort(sort),
                    None => input,
                };

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

        connection::query(
            after,
            before,
            first,
            last,
            |after, before, first, last| async move {
                let input = <MapsConnectionInput>::new(ConnectionParameters {
                    after,
                    before,
                    first,
                    last,
                });
                let input = match filter {
                    Some(filter) => input.with_filter(filter),
                    None => input,
                };
                let input = match sort {
                    Some(sort) => input.with_sort(sort),
                    None => input,
                };

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

enum EitherRedisKey<A, B> {
    A(A),
    B(B),
}

impl<A, B> ToRedisArgs for EitherRedisKey<A, B>
where
    A: ToRedisArgs,
    B: ToRedisArgs,
{
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + deadpool_redis::redis::RedisWrite,
    {
        match self {
            EitherRedisKey::A(a) => ToRedisArgs::write_redis_args(a, out),
            EitherRedisKey::B(b) => ToRedisArgs::write_redis_args(b, out),
        }
    }
}

fn custom_source_or<S, F, D>(source: Option<S>, default: F) -> EitherRedisKey<D, S>
where
    F: FnOnce() -> D,
{
    source
        .map(EitherRedisKey::B)
        .unwrap_or_else(|| EitherRedisKey::A(default()))
}

pub(crate) struct ConnectionInput<F, O, S> {
    connection_parameters: ConnectionParameters,
    filter: Option<F>,
    sort: Option<O>,
    source: Option<S>,
}

// derive(Default) adds Default: bound to the generics
impl<F, O, S> Default for ConnectionInput<F, O, S> {
    fn default() -> Self {
        Self {
            connection_parameters: Default::default(),
            filter: Default::default(),
            sort: Default::default(),
            source: Default::default(),
        }
    }
}

impl<F, O, S> ConnectionInput<F, O, S> {
    pub(crate) fn new(connection_parameters: ConnectionParameters) -> Self {
        Self {
            connection_parameters,
            ..Default::default()
        }
    }

    pub(crate) fn with_filter(mut self, filter: F) -> Self {
        self.filter = Some(filter);
        self
    }

    pub(crate) fn with_sort(mut self, sort: O) -> Self {
        self.sort = Some(sort);
        self
    }

    #[allow(unused)] // used for testing
    pub(crate) fn with_source<U>(self, source: U) -> ConnectionInput<F, O, U> {
        ConnectionInput {
            connection_parameters: self.connection_parameters,
            filter: self.filter,
            sort: self.sort,
            source: Some(source),
        }
    }
}

enum PlayerMapRankingCursor {
    Name(TextCursor),
    Score(F64Cursor),
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
    ConnectionInput<PlayersFilter, PlayerMapRankingSort, S>;

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
    let sort_input = match input.sort.map(|s| s.field) {
        Some(PlayerMapRankingSortableField::Name) => ParsedPaginationInput::new(
            PaginationInput::<TextCursor>::try_from_input(input.connection_parameters)?
                .map_cursor(PlayerMapRankingCursor::Name),
            |player: &PlayerWithUnstyledName| {
                TextCursor(player.unstyled_player_name.clone(), player.player.id).encode_cursor()
            },
        ),
        _ => ParsedPaginationInput::new(
            PaginationInput::<F64Cursor>::try_from_input(input.connection_parameters)?
                .map_cursor(PlayerMapRankingCursor::Score),
            |player: &PlayerWithUnstyledName| {
                F64Cursor(player.player.score, player.player.id).encode_cursor()
            },
        ),
    };

    let query = players::Entity::find()
        .expr_as(
            Func::cust("rm_mp_style").arg(Expr::col((players::Entity, players::Column::Name))),
            "unstyled_player_name",
        )
        .apply_if(input.filter, |query, filter| {
            query
                .apply_if(filter.player_login, |query, login| {
                    query.filter(players::Column::Login.like(format!("%{login}%")))
                })
                .apply_if(filter.player_name, |query, name| {
                    query.filter(Expr::col("unstyled_player_name").like(format!("%{name}%")))
                })
        });

    let mut query = match (
        &sort_input.page_input().dir,
        input.sort.as_ref().map(|s| s.field),
    ) {
        (
            PaginationDirection::After {
                cursor: Some(PlayerMapRankingCursor::Name(_)),
            }
            | PaginationDirection::Before {
                cursor: PlayerMapRankingCursor::Name(_),
            },
            _,
        )
        | (
            PaginationDirection::After { cursor: None },
            Some(PlayerMapRankingSortableField::Name),
        ) => query.cursor_by(Identity::Binary(
            "unstyled_player_name".into_iden(),
            players::Column::Id.into_iden(),
        )),
        _ => query.cursor_by((players::Column::Score, players::Column::Id)),
    }
    .into_model::<PlayerWithUnstyledName>();

    apply_cursor_input(&mut query, sort_input.page_input());

    match input.sort.and_then(|s| s.order) {
        Some(SortOrder::Descending) => query.desc(),
        _ => query.asc(),
    };

    let PaginationResult {
        mut connection,
        iter: players,
    } = get_paginated(conn, query, sort_input.page_input()).await?;

    connection.edges.reserve(players.len());
    let source = custom_source_or(input.source, player_ranking);

    for player in players {
        let rank: i32 = redis_conn.zrevrank(&source, player.player.id).await?;
        connection.edges.push(connection::Edge::new(
            ID(sort_input.encode_cursor(&player)),
            PlayerWithScore {
                rank: rank + 1,
                player: player.player.into(),
            },
        ));
    }

    Ok(connection)
}

pub(crate) type MapsConnectionInput<S = MapRanking> =
    ConnectionInput<MapsFilter, PlayerMapRankingSort, S>;

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
    let sort_input = match input.sort.map(|s| s.field) {
        Some(PlayerMapRankingSortableField::Name) => ParsedPaginationInput::new(
            PaginationInput::<TextCursor>::try_from_input(input.connection_parameters)?
                .map_cursor(PlayerMapRankingCursor::Name),
            |map: &MapWithUnstyledName| {
                TextCursor(map.unstyled_map_name.clone(), map.map.id).encode_cursor()
            },
        ),
        _ => ParsedPaginationInput::new(
            PaginationInput::<F64Cursor>::try_from_input(input.connection_parameters)?
                .map_cursor(PlayerMapRankingCursor::Score),
            |map: &MapWithUnstyledName| F64Cursor(map.map.score, map.map.id).encode_cursor(),
        ),
    };

    let query = maps::Entity::find()
        .expr_as(
            Func::cust("rm_mp_style").arg(Expr::col((maps::Entity, maps::Column::Name))),
            "unstyled_map_name",
        )
        .apply_if(input.filter, |query, filter| {
            query
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
                    query.filter(Expr::col("unstyled_map_name").like(format!("%{name}%")))
                })
        });

    let mut query = match (
        &sort_input.page_input().dir,
        input.sort.as_ref().map(|s| s.field),
    ) {
        (
            PaginationDirection::After {
                cursor: Some(PlayerMapRankingCursor::Name(_)),
            }
            | PaginationDirection::Before {
                cursor: PlayerMapRankingCursor::Name(_),
            },
            _,
        )
        | (
            PaginationDirection::After { cursor: None },
            Some(PlayerMapRankingSortableField::Name),
        ) => query.cursor_by(Identity::Binary(
            "unstyled_map_name".into_iden(),
            maps::Column::Id.into_iden(),
        )),
        _ => query.cursor_by((maps::Column::Score, maps::Column::Id)),
    }
    .into_model::<MapWithUnstyledName>();

    apply_cursor_input(&mut query, sort_input.page_input());

    match input.sort.and_then(|s| s.order) {
        Some(SortOrder::Descending) => query.desc(),
        _ => query.asc(),
    };

    let PaginationResult {
        mut connection,
        iter: maps,
    } = get_paginated(conn, query, sort_input.page_input()).await?;

    connection.edges.reserve(maps.len());
    let source = custom_source_or(input.source, map_ranking);

    for map in maps {
        let rank: i32 = redis_conn.zrevrank(&source, map.map.id).await?;
        connection.edges.push(connection::Edge::new(
            ID(sort_input.encode_cursor(&map)),
            MapWithScore {
                rank: rank + 1,
                map: map.map.into(),
            },
        ));
    }

    Ok(connection)
}
