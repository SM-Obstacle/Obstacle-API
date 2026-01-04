use mkenv::prelude::*;
use rand::Rng;
use std::time::Duration;

use async_graphql::{ID, connection::CursorType};
use chrono::SubsecRound;
use entity::{maps, players, records};
use itertools::Itertools;
use sea_orm::{ActiveValue::Set, EntityTrait};

use crate::{
    config::InitError,
    cursors::{ConnectionParameters, RecordDateCursor},
    objects::{
        player::get_player_records_connection, sort::UnorderedRecordSort, sort_order::SortOrder,
        sortable_fields::UnorderedRecordSortableField,
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

#[tracing::instrument]
async fn test_default_page(is_desc: bool) -> anyhow::Result<()> {
    let default_limit = crate::config().cursor_default_limit.get();
    let record_amount = default_limit * 2;

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("boogalogin".to_owned()),
        name: Set("booganame".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map_ids = rand::rng()
        .sample_iter(rand::distr::StandardUniform)
        .take(record_amount)
        .collect_vec();

    let maps = map_ids.iter().map(|map_id| maps::ActiveModel {
        id: Set(*map_id),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        player_id: Set(1),
        ..Default::default()
    });

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..record_amount)
        .zip(map_ids.iter())
        .map(|(i, map_id)| records::ActiveModel {
            record_id: Set((i + 1) as _),
            map_id: Set(*map_id),
            record_player_id: Set(1),
            flags: Set(682),
            time: Set(1000),
            respawn_count: Set(0),
            record_date: Set(record_dates[i]),
            ..Default::default()
        });

    test_env::wrap(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_player_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            1,
            Default::default(),
            ConnectionParameters {
                before: None,
                after: None,
                first: None,
                last: None,
            },
            is_desc.then_some(UnorderedRecordSort {
                field: UnorderedRecordSortableField::Date,
                order: Some(SortOrder::Descending),
            }),
            None,
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
                let i = if is_desc { record_amount - 1 - i } else { i };
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordDateCursor(record_date, i + 1).encode_cursor(),
                    rank: 1,
                    map_id: map_ids[i],
                    flags: 682,
                    player_id: 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000,
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
async fn default_page() -> anyhow::Result<()> {
    setup();
    test_default_page(false).await
}

#[tokio::test]
async fn default_page_desc() -> anyhow::Result<()> {
    setup();
    test_default_page(true).await
}

#[tokio::test]
async fn first_x_after_y() -> anyhow::Result<()> {
    setup();
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(6),
            after_idx: 4,
            is_desc: false,
        },
        true,
        6,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(4),
            after_idx: 4,
            is_desc: false,
        },
        true,
        4,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(4),
            after_idx: 14,
            is_desc: false,
        },
        true,
        4,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(5),
            after_idx: 14,
            is_desc: false,
        },
        false,
        5,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(4),
            after_idx: 17,
            is_desc: false,
        },
        false,
        2,
    )
    .await?;

    Ok(())
}

struct FirstXAfterYParams {
    record_amount: usize,
    first: Option<usize>,
    after_idx: usize,
    is_desc: bool,
}

#[tracing::instrument(
    skip(params),
    fields(
        record_amount = params.record_amount,
        first = params.first,
        after_idx = params.after_idx,
        is_desc = params.is_desc,
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

    let computed_has_next_page = params.after_idx + 1 + limit < params.record_amount;
    if has_next_page != computed_has_next_page {
        tracing::warn!(
            computed_has_next_page = computed_has_next_page,
            "wrong has_next_page",
        );
    }
    let computed_expected_len = limit.min(
        params
            .record_amount
            .saturating_sub(params.after_idx)
            .saturating_sub(1),
    );
    if expected_len != computed_expected_len {
        tracing::warn!(
            computed_expected_len = computed_expected_len,
            "wrong expected_len",
        );
    }

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("boogalogin".to_owned()),
        name: Set("booganame".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map_ids = rand::rng()
        .sample_iter(rand::distr::StandardUniform)
        .take(params.record_amount)
        .collect_vec();

    let maps = map_ids.iter().map(|map_id| maps::ActiveModel {
        id: Set(*map_id),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        player_id: Set(1),
        ..Default::default()
    });

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..params.record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..params.record_amount)
        .zip(map_ids.iter())
        .map(|(i, map_id)| records::ActiveModel {
            record_id: Set((i + 1) as _),
            map_id: Set(*map_id),
            record_player_id: Set(1),
            flags: Set(682),
            time: Set(1000),
            respawn_count: Set(0),
            record_date: Set(record_dates[i]),
            ..Default::default()
        });

    test_env::wrap(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_player_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            1,
            Default::default(),
            ConnectionParameters {
                first: params.first,
                after: Some(ID({
                    let idx = if params.is_desc {
                        params
                            .record_amount
                            .saturating_sub(1)
                            .saturating_sub(params.after_idx)
                    } else {
                        params.after_idx
                    };
                    RecordDateCursor(record_dates[idx].and_utc(), idx + 1).encode_cursor()
                })),
                ..Default::default()
            },
            params.is_desc.then_some(UnorderedRecordSort {
                field: UnorderedRecordSortableField::Date,
                order: Some(SortOrder::Descending),
            }),
            None,
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
                let i = if params.is_desc {
                    params
                        .record_amount
                        .saturating_sub(1)
                        .saturating_sub(params.after_idx)
                        .saturating_sub(1)
                        .saturating_sub(i)
                } else {
                    params.after_idx + 1 + i
                };
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordDateCursor(record_date, i as u32 + 1).encode_cursor(),
                    rank: 1,
                    map_id: map_ids[i],
                    flags: 682,
                    player_id: 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000,
                }
            }),
        );

        assert_eq!(result.has_next_page, has_next_page);
        assert!(result.has_previous_page);

        anyhow::Ok(())
    })
    .await
}

struct LastXBeforeYParams {
    record_amount: usize,
    last: Option<usize>,
    before_idx: usize,
    is_desc: bool,
}

#[tracing::instrument(
    skip(params),
    fields(
        record_amount = params.record_amount,
        last = params.last,
        before_idx = params.before_idx,
        is_desc = params.is_desc,
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
    let computed_expected_len = limit.min(params.before_idx).min(params.record_amount);
    if expected_len != computed_expected_len {
        tracing::warn!(
            computed_expected_len = computed_expected_len,
            "wrong expected_len",
        );
    }

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("boogalogin".to_owned()),
        name: Set("booganame".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map_ids = rand::rng()
        .sample_iter(rand::distr::StandardUniform)
        .take(params.record_amount)
        .collect_vec();

    let maps = map_ids.iter().map(|map_id| maps::ActiveModel {
        id: Set(*map_id),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        player_id: Set(1),
        ..Default::default()
    });

    // The higher the record ID, the less recent the record
    let now = chrono::Utc::now().naive_utc().trunc_subsecs(0);
    let record_dates = (0..params.record_amount)
        .map(|i| now - Duration::from_secs(3600 * (i as u64 + 1)))
        .collect_vec();

    let records = (0..params.record_amount)
        .zip(map_ids.iter())
        .map(|(i, map_id)| records::ActiveModel {
            record_id: Set((i + 1) as _),
            map_id: Set(*map_id),
            record_player_id: Set(1),
            flags: Set(682),
            time: Set(1000),
            respawn_count: Set(0),
            record_date: Set(record_dates[i]),
            ..Default::default()
        });

    test_env::wrap(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let result = get_player_records_connection(
            &db.sql_conn,
            &db.redis_pool,
            1,
            Default::default(),
            ConnectionParameters {
                before: Some(ID({
                    let idx = if params.is_desc {
                        params
                            .record_amount
                            .saturating_sub(1)
                            .saturating_sub(params.before_idx)
                    } else {
                        params.before_idx
                    };
                    RecordDateCursor(record_dates[idx].and_utc(), idx + 1).encode_cursor()
                })),
                after: None,
                first: None,
                last: params.last,
            },
            params.is_desc.then_some(UnorderedRecordSort {
                field: UnorderedRecordSortableField::Date,
                order: Some(SortOrder::Descending),
            }),
            None,
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
                let i = if params.is_desc {
                    params.record_amount - 1 - (i + params.before_idx - expected_len)
                } else {
                    i + params.before_idx - expected_len
                };
                let record_date = record_dates[i].and_utc();
                Record {
                    record_id: i as u32 + 1,
                    cursor: RecordDateCursor(record_date, i as u32 + 1).encode_cursor(),
                    rank: 1,
                    map_id: map_ids[i],
                    flags: 682,
                    player_id: 1,
                    record_date,
                    respawn_count: 0,
                    time: 1000,
                }
            }),
        );

        assert!(result.has_next_page);
        assert_eq!(result.has_previous_page, has_previous_page);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn first_x_after_y_desc() -> anyhow::Result<()> {
    setup();

    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(6),
            after_idx: 4,
            is_desc: true,
        },
        true,
        6,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(4),
            after_idx: 4,
            is_desc: true,
        },
        true,
        4,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(4),
            after_idx: 14,
            is_desc: true,
        },
        true,
        4,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(5),
            after_idx: 14,
            is_desc: true,
        },
        false,
        5,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 20,
            first: Some(5),
            after_idx: 16,
            is_desc: true,
        },
        false,
        3,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn after_x() -> anyhow::Result<()> {
    setup();
    let default_limit = crate::config().cursor_default_limit.get();

    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 100,
            first: None,
            after_idx: 15,
            is_desc: false,
        },
        true,
        default_limit,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 100,
            first: None,
            after_idx: 100 - default_limit - 1,
            is_desc: false,
        },
        false,
        default_limit,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 100,
            first: None,
            after_idx: 79,
            is_desc: false,
        },
        false,
        20,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn after_x_desc() -> anyhow::Result<()> {
    setup();
    let default_limit = crate::config().cursor_default_limit.get();

    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 100,
            first: None,
            after_idx: 15,
            is_desc: true,
        },
        true,
        default_limit,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 100,
            first: None,
            after_idx: 49,
            is_desc: true,
        },
        false,
        default_limit,
    )
    .await?;
    test_first_x_after_y(
        FirstXAfterYParams {
            record_amount: 100,
            first: None,
            after_idx: 79,
            is_desc: true,
        },
        false,
        20,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn last_x_before_y() -> anyhow::Result<()> {
    setup();

    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 10,
            last: Some(3),
            before_idx: 6,
            is_desc: false,
        },
        true,
        3,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 10,
            last: Some(3),
            before_idx: 3,
            is_desc: false,
        },
        false,
        3,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 10,
            last: Some(4),
            before_idx: 3,
            is_desc: false,
        },
        false,
        3,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn last_x_before_y_desc() -> anyhow::Result<()> {
    setup();

    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 10,
            last: Some(3),
            before_idx: 6,
            is_desc: true,
        },
        true,
        3,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 10,
            last: Some(3),
            before_idx: 3,
            is_desc: true,
        },
        false,
        3,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 10,
            last: Some(4),
            before_idx: 3,
            is_desc: true,
        },
        false,
        3,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn before_x() -> anyhow::Result<()> {
    setup();
    let default_limit = crate::config().cursor_default_limit.get();

    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 100,
            last: None,
            before_idx: 75,
            is_desc: false,
        },
        true,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 100,
            last: None,
            before_idx: 100 - default_limit,
            is_desc: false,
        },
        false,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 100,
            last: None,
            before_idx: 3,
            is_desc: false,
        },
        false,
        3,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn before_x_desc() -> anyhow::Result<()> {
    setup();
    let default_limit = crate::config().cursor_default_limit.get();

    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 100,
            last: None,
            before_idx: 75,
            is_desc: true,
        },
        true,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 100,
            last: None,
            before_idx: 100 - default_limit,
            is_desc: true,
        },
        false,
        default_limit,
    )
    .await?;
    test_last_x_before_y(
        LastXBeforeYParams {
            record_amount: 100,
            last: None,
            before_idx: 3,
            is_desc: true,
        },
        false,
        3,
    )
    .await?;

    Ok(())
}
