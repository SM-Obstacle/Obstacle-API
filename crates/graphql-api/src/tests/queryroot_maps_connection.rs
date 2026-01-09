use async_graphql::connection::CursorType;
use deadpool_redis::redis::{self, ToRedisArgs};
use entity::{maps, players};
use mkenv::Layer as _;
use rand::Rng;
use records_lib::RedisConnection;
use sea_orm::{ActiveValue::Set, EntityTrait};

use crate::{
    config::InitError,
    cursors::{ConnectionParameters, F64Cursor},
    objects::root::{MapsConnectionInput, get_maps_connection},
};

fn setup() {
    match crate::init_config() {
        Ok(_) | Err(InitError::ConfigAlreadySet) => (),
        Err(InitError::Config(e)) => {
            panic!("error during test setup: {e}");
        }
    }
}

#[derive(Debug)]
struct Map {
    cursor: String,
    rank: i32,
    id: u32,
    uid: String,
    name: String,
    score: f64,
}

impl PartialEq for Map {
    fn eq(&self, other: &Self) -> bool {
        const ALLOWED_ERROR: f64 = 0.05;
        self.cursor == other.cursor
            && self.rank == other.rank
            && self.id == other.id
            && self.uid == other.uid
            && self.name == other.name
            && (self.score - other.score).abs() < ALLOWED_ERROR
    }
}

fn gen_map_ranking_key() -> String {
    "__test_map_ranking_"
        .chars()
        .chain(
            rand::rng()
                .sample_iter(rand::distr::Alphabetic)
                .take(20)
                .map(char::from),
        )
        .collect()
}

async fn fill_redis_lb<I, M, S>(
    redis_conn: &mut RedisConnection,
    key: impl ToRedisArgs,
    iter: I,
) -> anyhow::Result<()>
where
    I: IntoIterator<Item = (M, S)>,
    M: ToRedisArgs,
    S: ToRedisArgs,
{
    let mut pipe = redis::pipe();
    pipe.atomic();
    pipe.del(&key);
    for (member, score) in iter {
        pipe.zadd(&key, member, score);
    }
    pipe.exec_async(redis_conn).await?;
    Ok(())
}

#[tokio::test]
async fn default_page() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let map_amount = default_limit * 2;

    let author = players::ActiveModel {
        id: Set(1),
        login: Set("boogalogin".to_owned()),
        name: Set("booganame".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let maps = (0..map_amount).map(|i| maps::ActiveModel {
        id: Set((i + 1) as _),
        game_id: Set(format!("map_{i}_uid")),
        name: Set(format!("map_{i}_name")),
        score: Set(i as _),
        player_id: Set(1),
        ..Default::default()
    });

    let source = gen_map_ranking_key();

    test_env::wrap(async |db| {
        players::Entity::insert(author).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;

        let mut redis_conn = db.redis_pool.get().await?;
        fill_redis_lb(
            &mut redis_conn,
            &source,
            (1..=map_amount).zip(0..map_amount),
        )
        .await?;

        let result = get_maps_connection(
            &db.sql_conn,
            &mut redis_conn,
            <MapsConnectionInput>::default().with_source(source),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Map {
                cursor: edge.cursor.0,
                rank: edge.node.rank,
                id: edge.node.map.inner.id,
                uid: edge.node.map.inner.game_id,
                name: edge.node.map.inner.name,
                score: edge.node.map.inner.score,
            }),
            (0..default_limit).map(|i| Map {
                cursor: F64Cursor {
                    score: i as _,
                    data: (i + 1),
                }
                .encode_cursor(),
                id: (i + 1) as _,
                uid: format!("map_{i}_uid"),
                name: format!("map_{i}_name"),
                rank: (map_amount - i) as _,
                score: i as _,
            }),
        );

        anyhow::Ok(())
    })
    .await
}

struct FirstXAfterYParams {
    map_amount: usize,
    first: Option<usize>,
    after_idx: usize,
}

#[tracing::instrument(
    skip(params),
    fields(
        map_amount = params.map_amount,
        first = params.first,
        after_idx = params.after_idx,
    )
)]
async fn test_first_x_after_y(
    params: FirstXAfterYParams,
    has_next_page: bool,
    expected_len: usize,
) -> anyhow::Result<()> {
    let limit = params
        .first
        .unwrap_or_else(|| crate::config().cursor_default_limit.get());

    let computed_has_next_page = params.after_idx + 1 + limit < params.map_amount;
    if has_next_page != computed_has_next_page {
        tracing::warn!(
            computed_has_next_page = computed_has_next_page,
            "wrong has_next_page",
        );
    }
    let computed_expected_len = limit.min(
        params
            .map_amount
            .saturating_sub(params.after_idx)
            .saturating_sub(1),
    );
    if expected_len != computed_expected_len {
        tracing::warn!(
            computed_expected_len = computed_expected_len,
            "wrong expected_len",
        );
    }

    let author = players::ActiveModel {
        id: Set(1),
        login: Set("boogalogin".to_owned()),
        name: Set("booganame".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let maps = (0..params.map_amount).map(|i| maps::ActiveModel {
        id: Set((i + 1) as _),
        game_id: Set(format!("map_{i}_uid")),
        name: Set(format!("map_{i}_name")),
        score: Set(i as _),
        player_id: Set(1),
        ..Default::default()
    });

    let source = gen_map_ranking_key();

    test_env::wrap(async |db| {
        players::Entity::insert(author).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;

        let mut redis_conn = db.redis_pool.get().await?;
        fill_redis_lb(
            &mut redis_conn,
            &source,
            (1..=params.map_amount).zip(0..params.map_amount),
        )
        .await?;

        let result = get_maps_connection(
            &db.sql_conn,
            &mut redis_conn,
            <MapsConnectionInput>::new(ConnectionParameters {
                first: params.first,
                after: Some(
                    F64Cursor {
                        score: params.after_idx as _,
                        data: params.after_idx as u32 + 1,
                    }
                    .into(),
                ),
                ..Default::default()
            })
            .with_source(source),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Map {
                cursor: edge.cursor.0,
                rank: edge.node.rank,
                id: edge.node.map.inner.id,
                uid: edge.node.map.inner.game_id,
                name: edge.node.map.inner.name,
                score: edge.node.map.inner.score,
            }),
            (0..expected_len).map(|i| {
                let i = params.after_idx + 1 + i;
                Map {
                    cursor: F64Cursor {
                        score: i as _,
                        data: (i + 1),
                    }
                    .encode_cursor(),
                    id: (i + 1) as _,
                    uid: format!("map_{i}_uid"),
                    name: format!("map_{i}_name"),
                    rank: (params.map_amount - i) as _,
                    score: i as _,
                }
            }),
        );

        anyhow::Ok(())
    })
    .await
}

struct LastXBeforeYParams {
    map_amount: usize,
    last: Option<usize>,
    before_idx: usize,
}

#[tracing::instrument(
    skip(params),
    fields(
        map_amount = params.map_amount,
        last = params.last,
        before_idx = params.before_idx,
    )
)]
async fn test_last_x_before_y(
    params: LastXBeforeYParams,
    has_previous_page: bool,
    expected_len: usize,
) -> anyhow::Result<()> {
    let limit = params
        .last
        .unwrap_or_else(|| crate::config().cursor_default_limit.get());

    let computed_has_previous_page = limit < params.before_idx;
    if has_previous_page != computed_has_previous_page {
        tracing::warn!(
            computed_has_previous_page = computed_has_previous_page,
            "wrong has_previous_page",
        );
    }
    let computed_expected_len = limit.min(params.before_idx).min(params.map_amount);
    if expected_len != computed_expected_len {
        tracing::warn!(
            computed_expected_len = computed_expected_len,
            "wrong expected_len",
        );
    }

    let author = players::ActiveModel {
        id: Set(1),
        login: Set("boogalogin".to_owned()),
        name: Set("booganame".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let maps = (0..params.map_amount).map(|i| maps::ActiveModel {
        id: Set((i + 1) as _),
        game_id: Set(format!("map_{i}_uid")),
        name: Set(format!("map_{i}_name")),
        score: Set(i as _),
        player_id: Set(1),
        ..Default::default()
    });

    let source = gen_map_ranking_key();

    test_env::wrap(async |db| {
        players::Entity::insert(author).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;

        let mut redis_conn = db.redis_pool.get().await?;
        fill_redis_lb(
            &mut redis_conn,
            &source,
            (1..=params.map_amount).zip(0..params.map_amount),
        )
        .await?;

        let result = get_maps_connection(
            &db.sql_conn,
            &mut redis_conn,
            <MapsConnectionInput>::new(ConnectionParameters {
                last: params.last,
                before: Some(
                    F64Cursor {
                        score: params.before_idx as _,
                        data: params.before_idx as u32 + 1,
                    }
                    .into(),
                ),
                ..Default::default()
            })
            .with_source(source),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Map {
                cursor: edge.cursor.0,
                rank: edge.node.rank,
                id: edge.node.map.inner.id,
                uid: edge.node.map.inner.game_id,
                name: edge.node.map.inner.name,
                score: edge.node.map.inner.score,
            }),
            (0..expected_len).map(|i| {
                let i = i + params.before_idx - expected_len;
                Map {
                    cursor: F64Cursor {
                        score: i as _,
                        data: (i + 1),
                    }
                    .encode_cursor(),
                    id: (i + 1) as _,
                    uid: format!("map_{i}_uid"),
                    name: format!("map_{i}_name"),
                    rank: (params.map_amount - i) as _,
                    score: i as _,
                }
            }),
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn first_x_after_y() -> anyhow::Result<()> {
    setup();

    test_first_x_after_y(
        FirstXAfterYParams {
            map_amount: 10,
            first: Some(3),
            after_idx: 2,
        },
        true,
        3,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            map_amount: 10,
            first: Some(10),
            after_idx: 2,
        },
        false,
        7,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            map_amount: 10,
            first: Some(6),
            after_idx: 2,
        },
        true,
        6,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn after_x() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let map_amount = default_limit * 2;

    test_first_x_after_y(
        FirstXAfterYParams {
            map_amount,
            first: None,
            after_idx: 2,
        },
        true,
        default_limit,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            map_amount,
            first: None,
            after_idx: default_limit + 7,
        },
        false,
        default_limit - 8,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            map_amount,
            first: None,
            after_idx: default_limit - 1,
        },
        true,
        default_limit,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn last_x_before_y() -> anyhow::Result<()> {
    setup();

    test_last_x_before_y(
        LastXBeforeYParams {
            map_amount: 10,
            last: Some(3),
            before_idx: 6,
        },
        true,
        3,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            map_amount: 10,
            last: Some(6),
            before_idx: 6,
        },
        false,
        6,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            map_amount: 10,
            last: Some(7),
            before_idx: 6,
        },
        false,
        6,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn last_x() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let map_amount = default_limit * 2;

    test_last_x_before_y(
        LastXBeforeYParams {
            map_amount,
            last: None,
            before_idx: default_limit + 1,
        },
        true,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            map_amount,
            last: None,
            before_idx: default_limit,
        },
        false,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            map_amount,
            last: None,
            before_idx: default_limit - 3,
        },
        false,
        default_limit - 3,
    )
    .await?;

    Ok(())
}
