use async_graphql::{ID, connection::CursorType};
use deadpool_redis::redis::{self, ToRedisArgs};
use entity::players;
use mkenv::Layer as _;
use rand::Rng;
use records_lib::RedisConnection;
use sea_orm::{ActiveValue::Set, EntityTrait};

use crate::{
    config::InitError,
    cursors::{ConnectionParameters, F64Cursor},
    objects::root::{PlayersConnectionInput, get_players_connection},
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
struct Player {
    cursor: String,
    rank: i32,
    id: u32,
    login: String,
    name: String,
    score: f64,
}

impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        const ALLOWED_ERROR: f64 = 0.05;
        self.cursor == other.cursor
            && self.rank == other.rank
            && self.id == other.id
            && self.login == other.login
            && self.name == other.name
            && (self.score - other.score).abs() < ALLOWED_ERROR
    }
}

fn gen_player_ranking_key() -> String {
    "__test_player_ranking_"
        .chars()
        .chain(
            rand::rng()
                .sample_iter(rand::distr::Alphabetic)
                .take(20)
                .map(char::from),
        )
        .collect()
}

async fn fill_redis_lb<I, P, S>(
    redis_conn: &mut RedisConnection,
    key: impl ToRedisArgs,
    iter: I,
) -> anyhow::Result<()>
where
    I: IntoIterator<Item = (P, S)>,
    P: ToRedisArgs,
    S: ToRedisArgs,
{
    let mut pipe = redis::pipe();
    pipe.atomic();
    pipe.del(&key);
    for (player_id, score) in iter {
        pipe.zadd(&key, player_id, score);
    }
    pipe.exec_async(redis_conn).await?;
    Ok(())
}

#[tokio::test]
async fn default_page() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let player_amount = default_limit * 2;

    let players = (0..player_amount).map(|i| players::ActiveModel {
        id: Set((i + 1) as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        score: Set(i as _),
        ..Default::default()
    });

    let source = gen_player_ranking_key();

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;

        let mut redis_conn = db.redis_pool.get().await?;
        fill_redis_lb(
            &mut redis_conn,
            &source,
            (1..=player_amount).zip(0..player_amount),
        )
        .await?;

        let result = get_players_connection(
            &db.sql_conn,
            &mut redis_conn,
            <PlayersConnectionInput>::default().with_source(source),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Player {
                cursor: edge.cursor.0,
                rank: edge.node.rank,
                id: edge.node.player.inner.id,
                login: edge.node.player.inner.login,
                name: edge.node.player.inner.name,
                score: edge.node.player.inner.score,
            }),
            (0..default_limit).map(|i| Player {
                cursor: F64Cursor {
                    score: i as _,
                    data: (i + 1),
                }
                .encode_cursor(),
                id: (i + 1) as _,
                login: format!("player_{i}_login"),
                name: format!("player_{i}_name"),
                rank: (player_amount - i) as _,
                score: i as _,
            }),
        );

        anyhow::Ok(())
    })
    .await
}

struct FirstXAfterYParams {
    player_amount: usize,
    first: Option<usize>,
    after_idx: usize,
}

#[tracing::instrument(
    skip(params),
    fields(
        player_amount = params.player_amount,
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

    let computed_has_next_page = params.after_idx + 1 + limit < params.player_amount;
    if has_next_page != computed_has_next_page {
        tracing::warn!(
            computed_has_next_page = computed_has_next_page,
            "wrong has_next_page",
        );
    }
    let computed_expected_len = limit.min(
        params
            .player_amount
            .saturating_sub(params.after_idx)
            .saturating_sub(1),
    );
    if expected_len != computed_expected_len {
        tracing::warn!(
            computed_expected_len = computed_expected_len,
            "wrong expected_len",
        );
    }

    let players = (0..params.player_amount).map(|i| players::ActiveModel {
        id: Set((i + 1) as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        score: Set(i as _),
        ..Default::default()
    });

    let source = gen_player_ranking_key();

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;

        let mut redis_conn = db.redis_pool.get().await?;
        fill_redis_lb(
            &mut redis_conn,
            &source,
            (1..=params.player_amount).zip(0..params.player_amount),
        )
        .await?;

        let result = get_players_connection(
            &db.sql_conn,
            &mut redis_conn,
            <PlayersConnectionInput>::new(ConnectionParameters {
                first: params.first,
                after: Some(ID(F64Cursor {
                    score: params.after_idx as _,
                    data: params.after_idx + 1,
                }
                .encode_cursor())),
                ..Default::default()
            })
            .with_source(source),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Player {
                cursor: edge.cursor.0,
                rank: edge.node.rank,
                id: edge.node.player.inner.id,
                login: edge.node.player.inner.login,
                name: edge.node.player.inner.name,
                score: edge.node.player.inner.score,
            }),
            (0..expected_len).map(|i| {
                let i = params.after_idx + 1 + i;
                Player {
                    cursor: F64Cursor {
                        score: i as _,
                        data: (i + 1),
                    }
                    .encode_cursor(),
                    id: (i + 1) as _,
                    login: format!("player_{i}_login"),
                    name: format!("player_{i}_name"),
                    rank: (params.player_amount - i) as _,
                    score: i as _,
                }
            }),
        );

        anyhow::Ok(())
    })
    .await
}

struct LastXBeforeYParams {
    player_amount: usize,
    last: Option<usize>,
    before_idx: usize,
}

#[tracing::instrument(
    skip(params),
    fields(
        player_amount = params.player_amount,
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
    let computed_expected_len = limit.min(params.before_idx).min(params.player_amount);
    if expected_len != computed_expected_len {
        tracing::warn!(
            computed_expected_len = computed_expected_len,
            "wrong expected_len",
        );
    }

    let players = (0..params.player_amount).map(|i| players::ActiveModel {
        id: Set((i + 1) as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        score: Set(i as _),
        ..Default::default()
    });

    let source = gen_player_ranking_key();

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        let mut redis_conn = db.redis_pool.get().await?;
        fill_redis_lb(
            &mut redis_conn,
            &source,
            (1..=params.player_amount).zip(0..params.player_amount),
        )
        .await?;

        let result = get_players_connection(
            &db.sql_conn,
            &mut redis_conn,
            <PlayersConnectionInput>::new(ConnectionParameters {
                last: params.last,
                before: Some(ID(F64Cursor {
                    score: params.before_idx as _,
                    data: params.before_idx + 1,
                }
                .encode_cursor())),
                ..Default::default()
            })
            .with_source(source),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Player {
                cursor: edge.cursor.0,
                rank: edge.node.rank,
                id: edge.node.player.inner.id,
                login: edge.node.player.inner.login,
                name: edge.node.player.inner.name,
                score: edge.node.player.inner.score,
            }),
            (0..expected_len).map(|i| {
                let i = i + params.before_idx - expected_len;
                Player {
                    cursor: F64Cursor {
                        score: i as _,
                        data: (i + 1),
                    }
                    .encode_cursor(),
                    id: (i + 1) as _,
                    login: format!("player_{i}_login"),
                    name: format!("player_{i}_name"),
                    rank: (params.player_amount - i) as _,
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
            player_amount: 10,
            first: Some(3),
            after_idx: 2,
        },
        true,
        3,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            player_amount: 10,
            first: Some(10),
            after_idx: 2,
        },
        false,
        7,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            player_amount: 10,
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
    let player_amount = default_limit * 2;

    test_first_x_after_y(
        FirstXAfterYParams {
            player_amount,
            first: None,
            after_idx: 2,
        },
        true,
        default_limit,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            player_amount,
            first: None,
            after_idx: default_limit + 7,
        },
        false,
        default_limit - 8,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            player_amount,
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
            player_amount: 10,
            last: Some(3),
            before_idx: 6,
        },
        true,
        3,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            player_amount: 10,
            last: Some(6),
            before_idx: 6,
        },
        false,
        6,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            player_amount: 10,
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
    let player_amount = default_limit * 2;

    test_last_x_before_y(
        LastXBeforeYParams {
            player_amount,
            last: None,
            before_idx: default_limit + 1,
        },
        true,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            player_amount,
            last: None,
            before_idx: default_limit,
        },
        false,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            player_amount,
            last: None,
            before_idx: default_limit - 3,
        },
        false,
        default_limit - 3,
    )
    .await?;

    Ok(())
}
