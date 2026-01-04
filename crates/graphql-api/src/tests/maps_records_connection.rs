use mkenv::prelude::*;
use std::time::Duration;

use async_graphql::{ID, connection::CursorType};
use chrono::SubsecRound;
use entity::{maps, players, records};
use itertools::Itertools;
use sea_orm::{ActiveValue::Set, EntityTrait};

use crate::{
    config::InitError,
    cursors::{ConnectionParameters, RecordDateCursor, RecordRankCursor},
    objects::{
        map::get_map_records_connection, sort::MapRecordSort, sort_order::SortOrder,
        sortable_fields::MapRecordSortableField,
    },
};

fn setup() {
    match crate::init_config() {
        Ok(_) | Err(InitError::ConfigAlreadySet) => (),
        Err(InitError::Config(e)) => {
            panic!("error during test setup: {e}");
        }
    }
}

#[derive(Debug, PartialEq)]
struct Record {
    record_id: u32,
    cursor: String,
    rank: i32,
    map_id: u32,
    player_id: u32,
    time: i32,
    record_date: chrono::DateTime<chrono::Utc>,
    flags: u32,
    respawn_count: i32,
}

#[tokio::test]
async fn default_page() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            (0..default_limit).map(|i| {
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordRankCursor {
                        time: (i as i32 + 1) * 1000,
                        record_date,
                        data: i as u32 + 1,
                    }
                    .encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            }),
        );

        assert!(result.has_next_page);
        assert!(!result.has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn default_page_desc() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            Default::default(),
            Some(MapRecordSort {
                field: MapRecordSortableField::Rank,
                order: Some(SortOrder::Descending),
            }),
            Default::default(),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            (0..default_limit).map(|i| {
                let i = record_amount - 1 - i;
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordRankCursor {
                        time: (i as i32 + 1) * 1000,
                        record_date,
                        data: i as u32 + 1,
                    }
                    .encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            }),
        );

        assert!(result.has_next_page);
        assert!(!result.has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn default_page_date() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            Default::default(),
            Some(MapRecordSort {
                field: MapRecordSortableField::Date,
                order: Default::default(),
            }),
            Default::default(),
        )
        .await?;

        let mut expected = (0..default_limit)
            .map(|i| {
                let i = record_amount - 1 - i;
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordDateCursor(record_date, i as u32 + 1).encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            })
            .collect_vec();

        expected.sort_by_key(|record| record.record_date);

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            expected,
        );

        assert!(result.has_next_page);
        assert!(!result.has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn default_page_date_desc() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            Default::default(),
            Some(MapRecordSort {
                field: MapRecordSortableField::Date,
                order: Some(SortOrder::Descending),
            }),
            Default::default(),
        )
        .await?;

        let mut expected = (0..default_limit)
            .map(|i| {
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordDateCursor(record_date, i as u32 + 1).encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            })
            .collect_vec();

        expected.sort_by(|a, b| a.record_date.cmp(&b.record_date).reverse());

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            expected,
        );

        assert!(result.has_next_page);
        assert!(!result.has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tracing::instrument]
async fn test_first_x_after_y(
    record_amount: usize,
    is_desc: bool,
    first: Option<usize>,
    after_idx: usize,
    expected_len: usize,
    has_next_page: bool,
) -> anyhow::Result<()> {
    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            ConnectionParameters {
                first,
                after: Some(ID({
                    let idx = if is_desc {
                        record_amount.saturating_sub(1).saturating_sub(after_idx)
                    } else {
                        after_idx
                    };
                    RecordRankCursor {
                        time: (idx as i32 + 1) * 1000,
                        record_date: record_dates[idx].and_utc(),
                        data: idx as u32 + 1,
                    }
                    .encode_cursor()
                })),
                ..Default::default()
            },
            is_desc.then_some(MapRecordSort {
                field: MapRecordSortableField::Rank,
                order: Some(SortOrder::Descending),
            }),
            Default::default(),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            (0..expected_len).map(|i| {
                let i = if is_desc {
                    record_amount
                        .saturating_sub(1)
                        .saturating_sub(after_idx)
                        .saturating_sub(1)
                        .saturating_sub(i)
                } else {
                    after_idx + 1 + i
                };
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordRankCursor {
                        time: (i as i32 + 1) * 1000,
                        record_date,
                        data: i as u32 + 1,
                    }
                    .encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            }),
        );

        assert_eq!(result.has_next_page, has_next_page);
        assert!(result.has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tracing::instrument]
async fn test_first_x_after_y_date(
    record_amount: usize,
    is_desc: bool,
    first: Option<usize>,
    after_idx: usize,
    expected_len: usize,
    has_next_page: bool,
) -> anyhow::Result<()> {
    // Testing the date order is pretty the same as testing the rank order,
    // but the order is reversed (higher rank means an earlier date).
    //
    // So when sorting by date with ascending order, in this test, it's like
    // sorting by rank with descending order.

    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            ConnectionParameters {
                first,
                after: Some(ID({
                    let idx = if is_desc {
                        after_idx
                    } else {
                        record_amount.saturating_sub(1).saturating_sub(after_idx)
                    };
                    RecordDateCursor(record_dates[idx].and_utc(), idx as u32 + 1).encode_cursor()
                })),
                ..Default::default()
            },
            Some(MapRecordSort {
                field: MapRecordSortableField::Date,
                order: is_desc.then_some(SortOrder::Descending),
            }),
            Default::default(),
        )
        .await?;

        let mut expected = (0..expected_len)
            .map(|i| {
                let i = if is_desc {
                    after_idx + 1 + i
                } else {
                    record_amount
                        .saturating_sub(1)
                        .saturating_sub(after_idx)
                        .saturating_sub(1)
                        .saturating_sub(i)
                };
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordDateCursor(record_date, i as u32 + 1).encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            })
            .collect_vec();

        expected.sort_by(if is_desc {
            |a: &Record, b: &Record| a.record_date.cmp(&b.record_date).reverse()
        } else {
            |a: &Record, b: &Record| a.record_date.cmp(&b.record_date)
        });

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            expected,
        );

        assert_eq!(result.has_next_page, has_next_page);
        assert!(result.has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn first_x_after_y() -> anyhow::Result<()> {
    setup();

    test_first_x_after_y(10, false, Some(3), 5, 3, true).await?;
    test_first_x_after_y(10, false, Some(3), 6, 3, false).await?;
    test_first_x_after_y(10, false, Some(3), 7, 2, false).await?;

    Ok(())
}

#[tokio::test]
async fn first_x_after_y_desc() -> anyhow::Result<()> {
    setup();

    test_first_x_after_y(10, true, Some(3), 5, 3, true).await?;
    test_first_x_after_y(10, true, Some(3), 6, 3, false).await?;
    test_first_x_after_y(10, true, Some(3), 7, 2, false).await?;

    Ok(())
}

#[tokio::test]
async fn first_x_after_y_date() -> anyhow::Result<()> {
    setup();

    test_first_x_after_y_date(10, false, Some(3), 5, 3, true).await?;
    test_first_x_after_y_date(10, false, Some(3), 6, 3, false).await?;
    test_first_x_after_y_date(10, false, Some(4), 6, 3, false).await?;

    Ok(())
}

#[tokio::test]
async fn first_x_after_y_date_desc() -> anyhow::Result<()> {
    setup();

    test_first_x_after_y_date(10, true, Some(3), 5, 3, true).await?;
    test_first_x_after_y_date(10, true, Some(3), 6, 3, false).await?;
    test_first_x_after_y_date(10, true, Some(4), 6, 3, false).await?;

    Ok(())
}

#[tokio::test]
async fn after_y() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_first_x_after_y(record_amount, false, None, 5, default_limit, true).await?;
    test_first_x_after_y(
        record_amount,
        false,
        None,
        default_limit - 1,
        default_limit,
        false,
    )
    .await?;
    test_first_x_after_y(
        record_amount,
        false,
        None,
        default_limit + 1,
        default_limit - 2,
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn after_y_desc() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_first_x_after_y(record_amount, true, None, 5, default_limit, true).await?;
    test_first_x_after_y(
        record_amount,
        true,
        None,
        default_limit - 1,
        default_limit,
        false,
    )
    .await?;
    test_first_x_after_y(
        record_amount,
        true,
        None,
        default_limit + 1,
        default_limit - 2,
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn after_y_date() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_first_x_after_y_date(record_amount, false, None, 5, default_limit, true).await?;
    test_first_x_after_y_date(
        record_amount,
        false,
        None,
        default_limit - 1,
        default_limit,
        false,
    )
    .await?;
    test_first_x_after_y_date(
        record_amount,
        false,
        None,
        default_limit,
        default_limit - 1,
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn after_y_date_desc() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_first_x_after_y_date(record_amount, true, None, 5, default_limit, true).await?;
    test_first_x_after_y_date(
        record_amount,
        true,
        None,
        default_limit - 1,
        default_limit,
        false,
    )
    .await?;
    test_first_x_after_y_date(
        record_amount,
        true,
        None,
        default_limit,
        default_limit - 1,
        false,
    )
    .await?;

    Ok(())
}

#[tracing::instrument]
async fn test_last_x_before_y(
    record_amount: usize,
    is_desc: bool,
    last: Option<usize>,
    before_idx: usize,
    expected_len: usize,
    has_previous_page: bool,
) -> anyhow::Result<()> {
    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            ConnectionParameters {
                last,
                before: Some(ID({
                    let idx = if is_desc {
                        record_amount.saturating_sub(1).saturating_sub(before_idx)
                    } else {
                        before_idx
                    };
                    RecordRankCursor {
                        time: (idx as i32 + 1) * 1000,
                        record_date: record_dates[idx].and_utc(),
                        data: idx as u32 + 1,
                    }
                    .encode_cursor()
                })),
                ..Default::default()
            },
            is_desc.then_some(MapRecordSort {
                field: MapRecordSortableField::Rank,
                order: Some(SortOrder::Descending),
            }),
            Default::default(),
        )
        .await?;

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            (0..expected_len).map(|i| {
                let i = if is_desc {
                    record_amount - 1 - (i + before_idx - expected_len)
                } else {
                    i + before_idx - expected_len
                };
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordRankCursor {
                        time: (i as i32 + 1) * 1000,
                        record_date,
                        data: i as u32 + 1,
                    }
                    .encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            }),
        );

        assert!(result.has_next_page);
        assert_eq!(result.has_previous_page, has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tracing::instrument]
async fn test_last_x_before_y_date(
    record_amount: usize,
    is_desc: bool,
    last: Option<usize>,
    before_idx: usize,
    expected_len: usize,
    has_previous_page: bool,
) -> anyhow::Result<()> {
    let players = (1..=record_amount).map(|i| players::ActiveModel {
        id: Set(i as _),
        login: Set(format!("player_{i}_login")),
        name: Set(format!("player_{i}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map_id = test_env::get_map_id();
    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount).map(|i| records::ActiveModel {
        record_id: Set((i + 1) as _),
        map_id: Set(map_id),
        record_player_id: Set((i + 1) as _),
        flags: Set(682),
        time: Set((1000 * (i + 1)) as _),
        respawn_count: Set(0),
        record_date: Set(record_dates[i]),
        ..Default::default()
    });

    test_env::wrap(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_map_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            map_id,
            Default::default(),
            ConnectionParameters {
                last,
                before: Some(ID({
                    let idx = if is_desc {
                        before_idx
                    } else {
                        record_amount.saturating_sub(1).saturating_sub(before_idx)
                    };
                    RecordDateCursor(record_dates[idx].and_utc(), idx as u32 + 1).encode_cursor()
                })),
                ..Default::default()
            },
            Some(MapRecordSort {
                field: MapRecordSortableField::Date,
                order: is_desc.then_some(SortOrder::Descending),
            }),
            Default::default(),
        )
        .await?;

        let mut expected = (0..expected_len)
            .map(|i| {
                let i = if is_desc {
                    i + before_idx - expected_len
                } else {
                    record_amount - 1 - (i + before_idx - expected_len)
                };
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordDateCursor(record_date, i as u32 + 1).encode_cursor(),
                    rank: i as i32 + 1,
                    map_id,
                    flags: 682,
                    player_id: i as u32 + 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000 * (i as i32 + 1),
                }
            })
            .collect_vec();

        expected.sort_by(if is_desc {
            |a: &Record, b: &Record| a.record_date.cmp(&b.record_date).reverse()
        } else {
            |a: &Record, b: &Record| a.record_date.cmp(&b.record_date)
        });

        itertools::assert_equal(
            result.edges.into_iter().map(|edge| Record {
                record_id: edge.node.inner.record.record_id,
                cursor: edge.cursor.0,
                rank: edge.node.inner.rank,
                map_id: edge.node.inner.record.map_id,
                player_id: edge.node.inner.record.record_player_id,
                flags: edge.node.inner.record.flags,
                record_date: edge.node.inner.record.record_date.and_utc(),
                respawn_count: edge.node.inner.record.respawn_count,
                time: edge.node.inner.record.time,
            }),
            expected,
        );

        assert!(result.has_next_page);
        assert_eq!(result.has_previous_page, has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn last_x_before_y() -> anyhow::Result<()> {
    setup();

    test_last_x_before_y(10, false, Some(3), 4, 3, true).await?;
    test_last_x_before_y(10, false, Some(3), 3, 3, false).await?;
    test_last_x_before_y(10, false, Some(3), 2, 2, false).await?;

    Ok(())
}

#[tokio::test]
async fn last_x_before_y_desc() -> anyhow::Result<()> {
    setup();

    test_last_x_before_y(10, true, Some(3), 4, 3, true).await?;
    test_last_x_before_y(10, true, Some(3), 3, 3, false).await?;
    test_last_x_before_y(10, true, Some(3), 2, 2, false).await?;

    Ok(())
}

#[tokio::test]
async fn last_x_before_y_date() -> anyhow::Result<()> {
    setup();

    test_last_x_before_y_date(10, false, Some(3), 4, 3, true).await?;
    test_last_x_before_y_date(10, false, Some(4), 4, 4, false).await?;
    test_last_x_before_y_date(10, false, Some(3), 2, 2, false).await?;

    Ok(())
}

#[tokio::test]
async fn last_x_before_y_date_desc() -> anyhow::Result<()> {
    setup();

    test_last_x_before_y_date(10, true, Some(3), 4, 3, true).await?;
    test_last_x_before_y_date(10, true, Some(4), 4, 4, false).await?;
    test_last_x_before_y_date(10, true, Some(3), 2, 2, false).await?;

    Ok(())
}

#[tokio::test]
async fn before_x() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_last_x_before_y(
        record_amount,
        false,
        None,
        record_amount - 10,
        default_limit,
        true,
    )
    .await?;
    test_last_x_before_y(
        record_amount,
        false,
        None,
        default_limit,
        default_limit,
        false,
    )
    .await?;
    test_last_x_before_y(
        record_amount,
        false,
        None,
        default_limit - 1,
        default_limit - 1,
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn before_x_desc() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_last_x_before_y(
        record_amount,
        true,
        None,
        record_amount - 10,
        default_limit,
        true,
    )
    .await?;
    test_last_x_before_y(
        record_amount,
        true,
        None,
        default_limit,
        default_limit,
        false,
    )
    .await?;
    test_last_x_before_y(
        record_amount,
        true,
        None,
        default_limit - 1,
        default_limit - 1,
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn before_x_date() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_last_x_before_y_date(
        record_amount,
        false,
        None,
        default_limit + 1,
        default_limit,
        true,
    )
    .await?;
    test_last_x_before_y_date(
        record_amount,
        false,
        None,
        default_limit,
        default_limit,
        false,
    )
    .await?;
    test_last_x_before_y_date(
        record_amount,
        false,
        None,
        default_limit - 1,
        default_limit - 1,
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn before_x_date_desc() -> anyhow::Result<()> {
    setup();

    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    test_last_x_before_y_date(
        record_amount,
        true,
        None,
        default_limit + 1,
        default_limit,
        true,
    )
    .await?;
    test_last_x_before_y_date(
        record_amount,
        true,
        None,
        default_limit,
        default_limit,
        false,
    )
    .await?;
    test_last_x_before_y_date(
        record_amount,
        true,
        None,
        default_limit - 1,
        default_limit - 1,
        false,
    )
    .await?;

    Ok(())
}
