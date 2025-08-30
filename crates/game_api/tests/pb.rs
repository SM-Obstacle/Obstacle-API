use std::{iter, time::Duration};

use actix_web::test;
use entity::{checkpoint_times, maps, players, records};
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

fn new_record(time: i32, rs_count: i32, datetime_offset_secs: u64) -> records::ActiveModel {
    records::ActiveModel {
        record_player_id: Set(1),
        map_id: Set(1),
        flags: Set(682),
        time: Set(time),
        respawn_count: Set(rs_count),
        record_date: Set(chrono::Utc::now().naive_utc() + Duration::from_secs(datetime_offset_secs)),
        ..Default::default()
    }
}

#[tokio::test]
async fn test_pb() -> anyhow::Result<()> {
    base::with_db(async |db| {
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
            cps_number: Set(Some(5)),
            ..Default::default()
        };

        let new_records = [
            new_record(10000, 5, 0),
            new_record(9500, 4, 1),
            new_record(11000, 6, 2),
            new_record(7500, 1, 3),
        ];

        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;

        // FIXME: change this when sea-orm supports `exec_with_returning_many` for MariaDB
        // see https://github.com/SeaQL/sea-orm/issues/2709
        let mut records = Vec::with_capacity(new_records.len());
        for new_record in new_records {
            let record_id = records::Entity::insert(new_record)
                .exec(&db.sql_conn)
                .await?
                .last_insert_id;
            let record = records::Entity::find_by_id(record_id)
                .one(&db.sql_conn)
                .await?
                .unwrap();
            records.push(record);
        }

        // 4 first times are 0.25s, the 5th one is <record_time> - 1s
        let cp_times = records.into_iter().flat_map(|record| {
            iter::repeat_n(250, 4)
                .chain(iter::once(record.time - 1000))
                .enumerate()
                .map(move |(cp_num, cp_time)| checkpoint_times::ActiveModel {
                    cp_num: Set(cp_num as _),
                    map_id: Set(1),
                    time: Set(cp_time),
                    record_id: Set(record.record_id),
                })
        });

        checkpoint_times::Entity::insert_many(cp_times)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::get()
            .uri("/player/pb?map_uid=map_uid")
            .insert_header(("PlayerLogin", "player_login"))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.rs_count, 1);

        itertools::assert_equal(
            body.cps_times,
            iter::repeat_n(250, 4)
                .enumerate()
                .map(|(cp_num, time)| ResponseItem {
                    cp_num: cp_num as _,
                    time,
                })
                .chain(iter::once(ResponseItem {
                    cp_num: 4,
                    time: 6500,
                })),
        );

        anyhow::Ok(())
    })
    .await
}
