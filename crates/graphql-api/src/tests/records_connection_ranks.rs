//! Tests that the records connections report the right ranks on maps whose leaderboard isn't
//! cached in Redis yet.
//!
//! Redis answers `ZCOUNT` with `0` on a key that doesn't exist, which is indistinguishable from
//! the player being first, so a rank read without syncing the leaderboard beforehand silently
//! comes back as `1`. The connections rooted at `QueryRoot` and `Player` span many maps at once,
//! unlike `Map.recordsConnection` which only ever deals with the one map it syncs.
//!
//! These tests deliberately never populate Redis: `test_env::get_map_id` draws random map IDs,
//! so the leaderboards start out missing, and the code under test is the only thing that may
//! bring them in.

use std::time::Duration;

use chrono::SubsecRound;
use deadpool_redis::redis::AsyncCommands as _;
use entity::{maps, players, records};
use itertools::Itertools;
use records_lib::redis_key::map_key;
use sea_orm::{ActiveValue::Set, EntityTrait};

use crate::{
    config::InitError,
    cursors::ConnectionParameters,
    objects::{player::get_player_records_connection, root::get_records_connection},
};

fn setup() {
    match crate::init_config() {
        Ok(_) | Err(InitError::ConfigAlreadySet) => (),
        Err(InitError::Config(e)) => {
            panic!("error during test setup: {e}");
        }
    }
}

const MAP_AMOUNT: usize = 3;
const PLAYERS_PER_MAP: usize = 5;
const RECORD_AMOUNT: usize = MAP_AMOUNT * PLAYERS_PER_MAP;

/// The map a record belongs to, from its index.
fn map_of(i: usize) -> usize {
    i / PLAYERS_PER_MAP
}

/// The expected rank of a record, from its index.
///
/// Each map holds one record per player, all with a distinct time, the fastest one first. So
/// the rank of a record is its position within its own map.
fn expected_rank(i: usize) -> i32 {
    (i % PLAYERS_PER_MAP) as i32 + 1
}

struct Fixture {
    map_ids: Vec<u32>,
    record_dates: Vec<chrono::NaiveDateTime>,
}

/// Builds `MAP_AMOUNT` maps holding `PLAYERS_PER_MAP` records each, and inserts them.
///
/// Record dates strictly decrease with the record index, so that a connection ordered by date
/// descending yields the records in index order.
async fn insert_fixture(db: &records_lib::Database) -> anyhow::Result<Fixture> {
    let map_ids = (0..MAP_AMOUNT)
        .map(|_| test_env::get_map_id())
        .collect_vec();

    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..RECORD_AMOUNT)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let players = (0..RECORD_AMOUNT).map(|i| players::ActiveModel {
        id: Set((i + 1) as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let maps = map_ids
        .iter()
        .enumerate()
        .map(|(i, map_id)| maps::ActiveModel {
            id: Set(*map_id),
            game_id: Set(format!("map_uid_{i}")),
            name: Set(format!("map_name_{i}")),
            player_id: Set(1),
            ..Default::default()
        });

    let records = (0..RECORD_AMOUNT).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_ids[map_of(i)]),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        // The slower the record, the higher its rank within its map.
        time: Set((1000 * expected_rank(i)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    players::Entity::insert_many(players)
        .exec(&db.sql_conn)
        .await?;
    maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
    records::Entity::insert_many(records)
        .exec(&db.sql_conn)
        .await?;

    Ok(Fixture {
        map_ids,
        record_dates,
    })
}

/// Asserts every map ended up with a leaderboard holding all of its records.
async fn assert_leaderboards_synced(
    db: &records_lib::Database,
    map_ids: &[u32],
) -> anyhow::Result<()> {
    let mut redis_conn = db.redis_pool.get().await?;

    for map_id in map_ids {
        let cached: u64 = redis_conn
            .zcard(map_key(*map_id, Default::default()))
            .await?;
        assert_eq!(
            cached, PLAYERS_PER_MAP as u64,
            "map {map_id} should have had its leaderboard built while reading the ranks"
        );
    }

    Ok(())
}

/// `QueryRoot.recordsConnection` spans several maps, none of them cached beforehand.
#[tokio::test]
async fn queryroot_ranks_without_cached_leaderboards() -> anyhow::Result<()> {
    setup();

    test_env::wrap(async |db| {
        let fixture = insert_fixture(&db).await?;

        let result = get_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            ConnectionParameters {
                before: None,
                after: None,
                first: Some(RECORD_AMOUNT),
                last: None,
            },
            Default::default(),
            None,
            None,
        )
        .await?;

        let ranks = result
            .edges
            .iter()
            .map(|edge| edge.node.inner.rank)
            .collect_vec();

        // Records come back ordered by date descending, so in index order.
        itertools::assert_equal(ranks.iter().copied(), (0..RECORD_AMOUNT).map(expected_rank));

        assert_leaderboards_synced(&db, &fixture.map_ids).await?;

        anyhow::Ok(())
    })
    .await
}

/// `Player.recordsConnection` reads the ranks of one player across several uncached maps.
///
/// This is the reported symptom: a player who is first on none of them was shown as first on
/// all of them.
#[tokio::test]
async fn player_ranks_without_cached_leaderboards() -> anyhow::Result<()> {
    setup();

    test_env::wrap(async |db| {
        let fixture = insert_fixture(&db).await?;

        // The last player of each map is its slowest, so they rank last everywhere. Give them
        // one record per map, keeping the times that already earned the last rank.
        let player_id = RECORD_AMOUNT as u32 + 1;
        let slowest_time = (1000 * PLAYERS_PER_MAP) as i32;

        players::Entity::insert(players::ActiveModel {
            id: Set(player_id),
            login: Set("lonely_player_login".to_owned()),
            name: Set("lonely_player_name".to_owned()),
            role: Set(0),
            ..Default::default()
        })
        .exec(&db.sql_conn)
        .await?;

        let extra =
            fixture
                .map_ids
                .iter()
                .enumerate()
                .map(|(i, map_id)| records::ActiveModel {
                    record_id: Set((RECORD_AMOUNT + i + 1) as _),
                    map_id: Set(*map_id),
                    record_player_id: Set(player_id),
                    flags: Set(682),
                    // Slower than every other record of the map, so last by one.
                    time: Set(slowest_time + 1),
                    respawn_count: Set(0),
                    record_date: Set(
                        fixture.record_dates[0] - Duration::from_secs(60 * (i as u64 + 1))
                    ),
                    ..Default::default()
                });

        records::Entity::insert_many(extra)
            .exec(&db.sql_conn)
            .await?;

        let result = get_player_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            player_id,
            Default::default(),
            ConnectionParameters {
                before: None,
                after: None,
                first: Some(MAP_AMOUNT),
                last: None,
            },
            None,
            None,
        )
        .await?;

        assert_eq!(result.edges.len(), MAP_AMOUNT);

        // They are behind every other player of every map, never first.
        for edge in &result.edges {
            assert_eq!(
                edge.node.inner.rank,
                PLAYERS_PER_MAP as i32 + 1,
                "player should rank last on map {}, not first",
                edge.node.inner.record.map_id
            );
        }

        anyhow::Ok(())
    })
    .await
}
