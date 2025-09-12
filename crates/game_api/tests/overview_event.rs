use std::iter;

use actix_web::test;
use anyhow::Context as _;
use chrono::SubsecRound;
use entity::{
    event, event_edition, event_edition_maps, event_edition_records, maps, players, records,
};
use sea_orm::{ActiveValue::Set, EntityTrait};

use crate::overview_base::{Response, Row};

mod base;
mod overview_base;

#[tokio::test]
async fn event_overview_original_maps_diff() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc().trunc_subsecs(0)),
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

    let map_id = rand::random_range(1..=100);
    let original_map_id = rand::random_range(1..=100);

    let maps = [map_id, original_map_id].map(|map_id| maps::ActiveModel {
        id: Set(map_id),
        player_id: Set(1),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
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

    let times_per_map_id = [(map_id, 10000), (original_map_id, 5000)];

    let records = times_per_map_id
        .iter()
        .enumerate()
        .map(|(i, (map_id, time))| records::ActiveModel {
            record_id: Set(i as u32 + 1),
            map_id: Set(*map_id),
            flags: Set(682),
            record_player_id: Set(1),
            time: Set(*time),
            respawn_count: Set(10),
            record_date: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        });

    let event_record = event_edition_records::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        record_id: Set(1),
    };

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;
        event_edition_records::Entity::insert(event_record)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        // Test the regular /overview version
        let req = test::TestRequest::get()
            .uri(&format!(
                "/overview?playerId=player_login&mapId=map_{original_map_id}_uid"
            ))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body).context("/overview")?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body.response,
            iter::once(Row {
                login: "player_login".to_owned(),
                nickname: "player_name".to_owned(),
                rank: 1,
                time: 5000,
            }),
        );

        // Test the event /overview version
        let req = test::TestRequest::get()
            .uri(&format!(
                "/event/event_handle/1/overview?playerId=player_login&mapId=map_{map_id}_uid"
            ))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<Response>(&body).context("/event/event_handle/1/overview")?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body.response,
            iter::once(Row {
                login: "player_login".to_owned(),
                nickname: "player_name".to_owned(),
                rank: 1,
                time: 10000,
            }),
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_overview_transparent_equal() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc().trunc_subsecs(0)),
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
        player_id: Set(1),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(map_id),
        order: Set(0),
        ..Default::default()
    };

    let record = records::ActiveModel {
        record_id: Set(1),
        map_id: Set(map_id),
        flags: Set(682),
        record_player_id: Set(1),
        time: Set(10000),
        respawn_count: Set(10),
        record_date: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;
        records::Entity::insert(record).exec(&db.sql_conn).await?;

        let app = base::get_app(db).await;

        // Test the regular /overview version
        let req = test::TestRequest::get()
            .uri(&format!(
                "/overview?playerId=player_login&mapId=map_{map_id}_uid"
            ))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body).context("/overview")?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body.response,
            iter::once(Row {
                login: "player_login".to_owned(),
                nickname: "player_name".to_owned(),
                rank: 1,
                time: 10000,
            }),
        );

        // Test the event /overview version
        let req = test::TestRequest::get()
            .uri(&format!(
                "/event/event_handle/1/overview?playerId=player_login&mapId=map_{map_id}_uid"
            ))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<Response>(&body).context("/event/event_handle/1/overview")?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body.response,
            iter::once(Row {
                login: "player_login".to_owned(),
                nickname: "player_name".to_owned(),
                rank: 1,
                time: 10000,
            }),
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_overview_original_maps_empty() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc().trunc_subsecs(0)),
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

    let map_id = rand::random_range(1..=100);

    let map = maps::ActiveModel {
        id: Set(map_id),
        player_id: Set(1),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(map_id),
        order: Set(0),
        ..Default::default()
    };

    let record = records::ActiveModel {
        record_id: Set(1),
        map_id: Set(map_id),
        flags: Set(682),
        record_player_id: Set(1),
        time: Set(10000),
        respawn_count: Set(10),
        record_date: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;
        records::Entity::insert(record).exec(&db.sql_conn).await?;

        let app = base::get_app(db).await;

        // Test the regular /overview version
        let req = test::TestRequest::get()
            .uri(&format!(
                "/overview?playerId=player_login&mapId=map_{map_id}_uid"
            ))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body).context("/overview")?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body.response,
            iter::once(Row {
                login: "player_login".to_owned(),
                nickname: "player_name".to_owned(),
                rank: 1,
                time: 10000,
            }),
        );

        // Test the event /overview version
        let req = test::TestRequest::get()
            .uri(&format!(
                "/event/event_handle/1/overview?playerId=player_login&mapId=map_{map_id}_uid"
            ))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<Response>(&body).context("/event/event_handle/1/overview")?;

        assert_eq!(status, 200);
        assert_eq!(body.response.len(), 0);

        anyhow::Ok(())
    })
    .await
}
