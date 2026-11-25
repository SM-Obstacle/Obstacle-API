//! Tests for the records amount of players, maps and event edition maps.
//!
//! The fixtures below reproduce what the game API writes to the database when a player finishes
//! a map, in each context:
//!
//! * Outside of any event, a single row is inserted in the `records` table.
//! * In an event edition, a row is inserted in the `records` table for the event map, along with
//!   a row in the `event_edition_records` table.
//! * If the event map has an original map (e.g. a Benchmark map), another row is inserted in the
//!   `records` table for the original map, with its `event_record_id` pointing to the event record.
//! * If the edition saves non-event records, a record made outside of the event on one of its maps
//!   also gets a row in the `event_edition_records` table.

use chrono::TimeZone as _;
use entity::{
    event, event_edition, event_edition_maps, event_edition_records, maps, players, records,
};
use sea_orm::{ActiveValue::Set, ConnectionTrait, EntityTrait as _};

use crate::objects::{
    event_edition_map::get_event_edition_map_records_amount, map::get_map_records_amount,
    player::get_player_records_amount,
};

fn date(hour: u32) -> chrono::NaiveDateTime {
    chrono::Utc
        .with_ymd_and_hms(2026, 11, 22, hour, 0, 0)
        .unwrap()
        .naive_utc()
}

async fn insert_players<C: ConnectionTrait>(conn: &C, amount: u32) -> anyhow::Result<()> {
    players::Entity::insert_many((1..=amount).map(|id| players::ActiveModel {
        id: Set(id),
        login: Set(format!("player_{id}_login")),
        name: Set(format!("player_{id}_name")),
        join_date: Set(Some(date(0))),
        role: Set(0),
        ..Default::default()
    }))
    .exec(conn)
    .await?;
    Ok(())
}

async fn insert_maps<C: ConnectionTrait>(conn: &C, uids: &[&str]) -> anyhow::Result<()> {
    maps::Entity::insert_many(uids.iter().zip(1..).map(|(uid, id)| maps::ActiveModel {
        id: Set(id),
        game_id: Set((*uid).to_owned()),
        player_id: Set(1),
        name: Set(format!("{uid}_name")),
        ..Default::default()
    }))
    .exec(conn)
    .await?;
    Ok(())
}

struct Record {
    record_id: u32,
    player_id: u32,
    map_id: u32,
    time: i32,
    hour: u32,
    /// The ID of the event record this record was copied from, for records on an original map.
    event_record_id: Option<u32>,
}

impl Record {
    fn new(record_id: u32, player_id: u32, map_id: u32, time: i32, hour: u32) -> Self {
        Self {
            record_id,
            player_id,
            map_id,
            time,
            hour,
            event_record_id: None,
        }
    }

    fn copied_from(mut self, event_record_id: u32) -> Self {
        self.event_record_id = Some(event_record_id);
        self
    }
}

async fn insert_records<C: ConnectionTrait>(conn: &C, records: Vec<Record>) -> anyhow::Result<()> {
    records::Entity::insert_many(records.into_iter().map(|r| records::ActiveModel {
        record_id: Set(r.record_id),
        record_player_id: Set(r.player_id),
        map_id: Set(r.map_id),
        time: Set(r.time),
        respawn_count: Set(0),
        record_date: Set(date(r.hour)),
        flags: Set(0),
        event_record_id: Set(r.event_record_id),
        ..Default::default()
    }))
    .exec(conn)
    .await?;
    Ok(())
}

/// Inserts the event with ID 1 and its edition with ID 1, containing the provided maps.
///
/// Each map is given as `(map_id, original_map_id)`.
async fn insert_event<C: ConnectionTrait>(
    conn: &C,
    save_non_event_record: bool,
    non_original_maps: bool,
    maps: &[(u32, Option<u32>)],
) -> anyhow::Result<()> {
    event::Entity::insert(event::ActiveModel {
        id: Set(1),
        handle: Set("event".to_owned()),
        ..Default::default()
    })
    .exec(conn)
    .await?;

    event_edition::Entity::insert(event_edition::ActiveModel {
        id: Set(1),
        event_id: Set(1),
        name: Set("edition".to_owned()),
        start_date: Set(date(10)),
        save_non_event_record: Set(save_non_event_record as _),
        non_original_maps: Set(non_original_maps as _),
        is_transparent: Set(0),
        ..Default::default()
    })
    .exec(conn)
    .await?;

    event_edition_maps::Entity::insert_many(maps.iter().zip(0..).map(
        |((map_id, original_map_id), order)| event_edition_maps::ActiveModel {
            event_id: Set(1),
            edition_id: Set(1),
            map_id: Set(*map_id),
            original_map_id: Set(*original_map_id),
            order: Set(order),
            is_disabled: Set(false),
            ..Default::default()
        },
    ))
    .exec(conn)
    .await?;

    Ok(())
}

async fn insert_event_records<C: ConnectionTrait>(
    conn: &C,
    record_ids: &[u32],
) -> anyhow::Result<()> {
    event_edition_records::Entity::insert_many(record_ids.iter().map(|record_id| {
        event_edition_records::ActiveModel {
            record_id: Set(*record_id),
            event_id: Set(1),
            edition_id: Set(1),
        }
    }))
    .exec(conn)
    .await?;
    Ok(())
}

/// A normal map with normal records: only the personal bests are counted.
#[tokio::test]
async fn regular_map() -> anyhow::Result<()> {
    test_env::wrap(async |db| {
        let conn = &db.sql_conn;

        insert_players(conn, 3).await?;
        insert_maps(conn, &["map_uid", "other_map_uid"]).await?;
        insert_records(
            conn,
            vec![
                // Player 1 improves twice on map 1
                Record::new(1, 1, 1, 150, 11),
                Record::new(2, 1, 1, 120, 12),
                Record::new(3, 1, 1, 100, 13),
                // Player 2 has a single record on map 1, and one on map 2
                Record::new(4, 2, 1, 130, 11),
                Record::new(5, 2, 2, 130, 11),
                // Player 3 doesn't play map 1
                Record::new(6, 3, 2, 140, 12),
            ],
        )
        .await?;

        assert_eq!(get_map_records_amount(conn, 1).await?, 2);
        assert_eq!(get_map_records_amount(conn, 2).await?, 2);

        assert_eq!(get_player_records_amount(conn, 1).await?, 1);
        assert_eq!(get_player_records_amount(conn, 2).await?, 2);
        assert_eq!(get_player_records_amount(conn, 3).await?, 1);

        anyhow::Ok(())
    })
    .await
}

/// A normal map registered to an event without an original map, like a campaign event
/// (`non_original_maps = 1` and `save_non_event_record = 1`).
///
/// The event records are also regular records of the map.
#[tokio::test]
async fn event_map_without_original_map() -> anyhow::Result<()> {
    test_env::wrap(async |db| {
        let conn = &db.sql_conn;

        insert_players(conn, 3).await?;
        insert_maps(conn, &["map_uid"]).await?;
        insert_event(conn, true, true, &[(1, None)]).await?;
        insert_records(
            conn,
            vec![
                // Player 3 finished the map before it was in the event
                Record::new(1, 3, 1, 90, 9),
                // Player 1 finishes in the event
                Record::new(2, 1, 1, 150, 11),
                // Player 1 then improves outside of the event
                Record::new(3, 1, 1, 100, 12),
                // Player 2 finishes outside of the event
                Record::new(4, 2, 1, 130, 12),
            ],
        )
        .await?;
        // Records made outside of the event are saved in it too, because of
        // `save_non_event_record`, except the ones made before the map was added.
        insert_event_records(conn, &[2, 3, 4]).await?;

        // Regular count: players 1, 2 and 3
        assert_eq!(get_map_records_amount(conn, 1).await?, 3);
        // Event count: players 1 and 2
        assert_eq!(
            get_event_edition_map_records_amount(conn, 1, 1, 1).await?,
            2
        );
        // Unknown edition
        assert_eq!(
            get_event_edition_map_records_amount(conn, 1, 2, 1).await?,
            0
        );

        assert_eq!(get_player_records_amount(conn, 1).await?, 1);
        assert_eq!(get_player_records_amount(conn, 2).await?, 1);
        assert_eq!(get_player_records_amount(conn, 3).await?, 1);

        anyhow::Ok(())
    })
    .await
}

/// An event map registered to an event with an original map, like Benchmark 2
/// (`non_original_maps = 0`).
///
/// The event records stay on the event map, and their copy is a regular record of the original
/// map.
#[tokio::test]
async fn event_map_with_original_map() -> anyhow::Result<()> {
    test_env::wrap(async |db| {
        let conn = &db.sql_conn;

        insert_players(conn, 3).await?;
        // Map 1 is the original map, map 2 is the event map
        insert_maps(conn, &["X", "X_benchmark"]).await?;
        insert_event(conn, false, false, &[(2, Some(1))]).await?;
        insert_records(
            conn,
            vec![
                // Player 1 finishes the original map outside of the event
                Record::new(1, 1, 1, 150, 9),
                // Player 1 then improves in the event, which is copied to the original map
                Record::new(2, 1, 2, 100, 11),
                Record::new(3, 1, 1, 100, 11).copied_from(2),
                // Player 2 only finishes in the event
                Record::new(4, 2, 2, 130, 12),
                Record::new(5, 2, 1, 130, 12).copied_from(4),
                // Player 3 only finishes the original map outside of the event
                Record::new(6, 3, 1, 140, 12),
            ],
        )
        .await?;
        insert_event_records(conn, &[2, 4]).await?;

        // Regular count of the original map: players 1, 2 and 3
        assert_eq!(get_map_records_amount(conn, 1).await?, 3);
        // The event map has no regular records
        assert_eq!(get_map_records_amount(conn, 2).await?, 0);
        // Event count: players 1 and 2
        assert_eq!(
            get_event_edition_map_records_amount(conn, 1, 1, 2).await?,
            2
        );
        // The original map isn't in the event
        assert_eq!(
            get_event_edition_map_records_amount(conn, 1, 1, 1).await?,
            0
        );
        // Unknown edition
        assert_eq!(
            get_event_edition_map_records_amount(conn, 1, 2, 2).await?,
            0
        );

        // The event records aren't counted twice for the players
        assert_eq!(get_player_records_amount(conn, 1).await?, 1);
        assert_eq!(get_player_records_amount(conn, 2).await?, 1);
        assert_eq!(get_player_records_amount(conn, 3).await?, 1);

        anyhow::Ok(())
    })
    .await
}
