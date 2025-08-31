use std::time::Duration;

use actix_web::test;
use entity::{
    event, event_edition, event_edition_maps, global_event_records, global_records, maps, players,
};
use sea_orm::{ActiveValue::Set, ColumnTrait as _, EntityTrait, QueryFilter as _};

use crate::player_finished_base::{Record, Request, Response};

mod base;
mod player_finished_base;

#[tokio::test]
async fn finished_on_transparent_event() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc() - Duration::from_secs(100)),
        is_transparent: Set(1),
        non_original_maps: Set(0),
        save_non_event_record: Set(0),
        ..Default::default()
    };

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map_id = rand::random_range(1..=100);

    let map = maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(map_id),
        order: Set(0),
        ..Default::default()
    };

    base::with_db(async |db| {
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/event/event_handle/1/player/finished")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                map_uid: "map_uid".to_owned(),
                time: 10000,
                flags: Some(682),
                respawn_count: 7,
                cps: vec![10000],
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();

        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        // Check response
        assert_eq!(status, 200);
        assert_eq!(
            body,
            Response {
                current_rank: 1,
                has_improved: true,
                old: 10000,
                new: 10000,
                old_rank: -1,
            }
        );

        // Check record saved in DB
        let record = global_records::Entity::find()
            .filter(
                global_records::Column::MapId
                    .eq(map_id)
                    .and(global_records::Column::RecordPlayerId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Record should exist in database"));

        assert_eq!(
            record,
            Record {
                map_id,
                player_id: 1,
                time: 10000,
                respawn_count: 7,
                flags: 682,
            }
        );

        let event_record = global_event_records::Entity::find()
            .filter(
                global_event_records::Column::MapId
                    .eq(map_id)
                    .and(global_event_records::Column::RecordPlayerId.eq(1))
                    .and(global_event_records::Column::EventId.eq(1))
                    .and(global_event_records::Column::EditionId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?;
        assert_eq!(event_record, None);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_finish_transitive_save() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc() - Duration::from_secs(100)),
        is_transparent: Set(0),
        non_original_maps: Set(0),
        save_non_event_record: Set(0),
        ..Default::default()
    };

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let original_map_id = rand::random_range(1..=100);
    let map_id = rand::random_range(1..=100);

    let maps = [original_map_id, map_id].map(|map_id| maps::ActiveModel {
        id: Set(map_id),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        player_id: Set(1),
        ..Default::default()
    });

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(map_id),
        original_map_id: Set(Some(original_map_id)),
        order: Set(0),
        transitive_save: Set(Some(1)),
        ..Default::default()
    };

    base::with_db(async |db| {
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/event/event_handle/1/player/finished")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                map_uid: format!("map_{original_map_id}_uid"),
                time: 10000,
                flags: Some(682),
                respawn_count: 7,
                cps: vec![10000],
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();

        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        // Check response
        assert_eq!(status, 200);
        assert_eq!(
            body,
            Response {
                current_rank: 1,
                has_improved: true,
                old: 10000,
                new: 10000,
                old_rank: -1,
            }
        );

        // Check record saved in DB
        let event_record = global_event_records::Entity::find()
            .filter(
                global_event_records::Column::MapId
                    .eq(map_id)
                    .and(global_event_records::Column::RecordPlayerId.eq(1))
                    .and(global_event_records::Column::EventId.eq(1))
                    .and(global_event_records::Column::EditionId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("global_event_records should exist in database"));

        let expected_record = Record {
            map_id: original_map_id,
            player_id: 1,
            time: 10000,
            respawn_count: 7,
            flags: 682,
        };

        assert_eq!(
            event_record,
            Record {
                map_id,
                ..expected_record
            }
        );

        let record_on_original_map = global_records::Entity::find()
            .filter(
                global_records::Column::MapId
                    .eq(original_map_id)
                    .and(global_records::Column::RecordPlayerId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Record should exist in database"));

        assert_eq!(record_on_original_map, expected_record);
        assert_eq!(
            record_on_original_map.event_record_id,
            Some(event_record.record_id)
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_finish_save_to_original() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc() - Duration::from_secs(100)),
        is_transparent: Set(0),
        non_original_maps: Set(0),
        save_non_event_record: Set(0),
        ..Default::default()
    };

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let original_map_id = rand::random_range(1..=100);
    let map_id = rand::random_range(1..=100);

    let maps = [original_map_id, map_id].map(|map_id| maps::ActiveModel {
        id: Set(map_id),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        player_id: Set(1),
        ..Default::default()
    });

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(map_id),
        original_map_id: Set(Some(original_map_id)),
        order: Set(0),
        ..Default::default()
    };

    base::with_db(async |db| {
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/event/event_handle/1/player/finished")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                map_uid: format!("map_{map_id}_uid"),
                time: 10000,
                flags: Some(682),
                respawn_count: 7,
                cps: vec![10000],
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();

        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        // Check response
        assert_eq!(status, 200);
        assert_eq!(
            body,
            Response {
                current_rank: 1,
                has_improved: true,
                old: 10000,
                new: 10000,
                old_rank: -1,
            }
        );

        // Check record saved in DB
        let event_record = global_event_records::Entity::find()
            .filter(
                global_event_records::Column::MapId
                    .eq(map_id)
                    .and(global_event_records::Column::RecordPlayerId.eq(1))
                    .and(global_event_records::Column::EventId.eq(1))
                    .and(global_event_records::Column::EditionId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("global_event_records should exist in database"));

        let expected_record = Record {
            map_id: original_map_id,
            player_id: 1,
            time: 10000,
            respawn_count: 7,
            flags: 682,
        };

        assert_eq!(
            event_record,
            Record {
                map_id,
                ..expected_record
            }
        );

        let record_on_original_map = global_records::Entity::find()
            .filter(
                global_records::Column::MapId
                    .eq(original_map_id)
                    .and(global_records::Column::RecordPlayerId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Record should exist in database"));

        assert_eq!(record_on_original_map, expected_record);
        assert_eq!(
            record_on_original_map.event_record_id,
            Some(event_record.record_id)
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_finish_non_transitive_save() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc() - Duration::from_secs(100)),
        is_transparent: Set(0),
        non_original_maps: Set(0),
        save_non_event_record: Set(0),
        ..Default::default()
    };

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let original_map_id = rand::random_range(1..=100);
    let map_id = rand::random_range(1..=100);

    let maps = [original_map_id, map_id].map(|map_id| maps::ActiveModel {
        id: Set(map_id),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        player_id: Set(1),
        ..Default::default()
    });

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(map_id),
        original_map_id: Set(Some(original_map_id)),
        order: Set(0),
        ..Default::default()
    };

    base::with_db(async |db| {
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/event/event_handle/1/player/finished")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                map_uid: format!("map_{original_map_id}_uid"),
                time: 10000,
                flags: Some(682),
                respawn_count: 7,
                cps: vec![10000],
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();

        let body = test::read_body(res).await;
        let expected_err = base::try_from_slice::<base::ErrorResponse>(&body)?;

        // Check response
        assert_eq!(status, 400);
        // Original map isn't counted as being part of the edition
        assert_eq!(expected_err.r#type, 312);

        // Check record saved in DB
        let event_record = global_event_records::Entity::find()
            .filter(
                global_event_records::Column::MapId
                    .eq(map_id)
                    .and(global_event_records::Column::RecordPlayerId.eq(1))
                    .and(global_event_records::Column::EventId.eq(1))
                    .and(global_event_records::Column::EditionId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?;

        assert_eq!(event_record, None);

        let record_on_original_map = global_records::Entity::find()
            .filter(
                global_records::Column::MapId
                    .eq(original_map_id)
                    .and(global_records::Column::RecordPlayerId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?;

        assert_eq!(record_on_original_map, None);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_finish_save_non_event_record() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc() - Duration::from_secs(100)),
        is_transparent: Set(0),
        non_original_maps: Set(0),
        save_non_event_record: Set(1),
        ..Default::default()
    };

    let player = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let original_map_id = rand::random_range(1..=100);
    let map_id = rand::random_range(1..=100);

    let maps = [original_map_id, map_id].map(|map_id| maps::ActiveModel {
        id: Set(map_id),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        player_id: Set(1),
        ..Default::default()
    });

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(map_id),
        original_map_id: Set(Some(original_map_id)),
        order: Set(0),
        ..Default::default()
    };

    base::with_db(async |db| {
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/player/finished")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                map_uid: format!("map_{map_id}_uid"),
                time: 10000,
                flags: Some(682),
                respawn_count: 7,
                cps: vec![10000],
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();

        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        // Check response
        assert_eq!(status, 200);
        assert_eq!(
            body,
            Response {
                current_rank: 1,
                has_improved: true,
                old: 10000,
                new: 10000,
                old_rank: -1,
            }
        );

        // Check record saved in DB
        let event_record = global_event_records::Entity::find()
            .filter(
                global_event_records::Column::MapId
                    .eq(map_id)
                    .and(global_event_records::Column::RecordPlayerId.eq(1))
                    .and(global_event_records::Column::EventId.eq(1))
                    .and(global_event_records::Column::EditionId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("global_event_records should exist in database"));

        let expected_record = Record {
            map_id: original_map_id,
            player_id: 1,
            time: 10000,
            respawn_count: 7,
            flags: 682,
        };

        assert_eq!(
            event_record,
            Record {
                map_id,
                ..expected_record
            }
        );

        let record_on_original_map = global_records::Entity::find()
            .filter(
                global_records::Column::MapId
                    .eq(original_map_id)
                    .and(global_records::Column::RecordPlayerId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Record should exist in database"));

        assert_eq!(record_on_original_map, expected_record);
        assert_eq!(
            record_on_original_map.event_record_id,
            Some(event_record.record_id)
        );

        anyhow::Ok(())
    })
    .await
}
