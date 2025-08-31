use std::iter;

use actix_web::test;
use entity::{
    checkpoint_times, event, event_edition, event_edition_maps, event_edition_records, maps,
    players, records,
};
use sea_orm::{ActiveValue::Set, EntityTrait};

mod base;

#[derive(Debug, PartialEq, serde::Deserialize)]
struct ResponseItem {
    cp_num: u32,
    time: i32,
}

#[derive(serde::Deserialize)]
struct Response {
    rs_count: i32,
    cps_times: Vec<ResponseItem>,
}

#[tokio::test]
async fn diff_pb_original_map_event_map() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        id: Set(1),
        event_id: Set(1),
        name: Set("event_edition".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc()),
        non_original_maps: Set(0),
        is_transparent: Set(0),
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

    let map = maps::ActiveModel {
        id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    let edition_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(1),
        order: Set(1),
        ..Default::default()
    };

    let record_original_version = records::ActiveModel {
        record_id: Set(1),
        record_player_id: Set(1),
        map_id: Set(1),
        time: Set(10000),
        flags: Set(682),
        record_date: Set(chrono::Utc::now().naive_utc()),
        respawn_count: Set(7),
        ..Default::default()
    };

    let cp_time_original_version = checkpoint_times::ActiveModel {
        record_id: Set(1),
        cp_num: Set(0),
        map_id: Set(1),
        time: Set(10000),
    };

    let record_event_version = records::ActiveModel {
        record_id: Set(2),
        record_player_id: Set(1),
        map_id: Set(1),
        time: Set(7800),
        flags: Set(682),
        record_date: Set(chrono::Utc::now().naive_utc()),
        respawn_count: Set(4),
        ..Default::default()
    };

    let cp_time_event_version = checkpoint_times::ActiveModel {
        record_id: Set(2),
        cp_num: Set(0),
        map_id: Set(1),
        time: Set(7800),
    };

    let edition_record = event_edition_records::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        record_id: Set(2),
    };

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(edition_map)
            .exec(&db.sql_conn)
            .await?;
        records::Entity::insert_many([record_original_version, record_event_version])
            .exec(&db.sql_conn)
            .await?;
        event_edition_records::Entity::insert(edition_record)
            .exec(&db.sql_conn)
            .await?;
        checkpoint_times::Entity::insert_many([cp_time_original_version, cp_time_event_version])
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let event_req = test::TestRequest::get()
            .uri("/event/event_handle/1/player/pb?map_uid=map_uid")
            .insert_header(("PlayerLogin", "player_login"))
            .to_request();

        let res = test::call_service(&app, event_req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.rs_count, 4);
        itertools::assert_equal(
            body.cps_times.into_iter().map(|cp| cp.time),
            iter::once(7800),
        );

        let normal_req = test::TestRequest::get()
            .uri("/player/pb?map_uid=map_uid")
            .insert_header(("PlayerLogin", "player_login"))
            .to_request();

        let res = test::call_service(&app, normal_req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.rs_count, 7);
        itertools::assert_equal(
            body.cps_times.into_iter().map(|cp| cp.time),
            iter::once(10000),
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn transparent_event_same_pb() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        id: Set(1),
        event_id: Set(1),
        name: Set("event_edition".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc()),
        non_original_maps: Set(0),
        is_transparent: Set(1),
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

    let map = maps::ActiveModel {
        id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        ..Default::default()
    };

    let edition_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(1),
        order: Set(1),
        ..Default::default()
    };

    let record = records::ActiveModel {
        record_id: Set(1),
        record_player_id: Set(1),
        map_id: Set(1),
        time: Set(10000),
        flags: Set(682),
        record_date: Set(chrono::Utc::now().naive_utc()),
        respawn_count: Set(7),
        ..Default::default()
    };

    let cp_time = checkpoint_times::ActiveModel {
        record_id: Set(1),
        cp_num: Set(0),
        map_id: Set(1),
        time: Set(10000),
    };

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(edition_map)
            .exec(&db.sql_conn)
            .await?;
        records::Entity::insert(record).exec(&db.sql_conn).await?;
        checkpoint_times::Entity::insert(cp_time)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let event_req = test::TestRequest::get()
            .uri("/event/event_handle/1/player/pb?map_uid=map_uid")
            .insert_header(("PlayerLogin", "player_login"))
            .to_request();

        let res = test::call_service(&app, event_req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.rs_count, 7);
        itertools::assert_equal(
            body.cps_times.into_iter().map(|cp| cp.time),
            iter::once(10000),
        );

        let normal_req = test::TestRequest::get()
            .uri("/player/pb?map_uid=map_uid")
            .insert_header(("PlayerLogin", "player_login"))
            .to_request();

        let res = test::call_service(&app, normal_req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.rs_count, 7);
        itertools::assert_equal(
            body.cps_times.into_iter().map(|cp| cp.time),
            iter::once(10000),
        );

        anyhow::Ok(())
    })
    .await
}
