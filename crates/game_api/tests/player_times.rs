use std::array;

use actix_web::test;
use entity::{maps, players, records};
use sea_orm::{ActiveValue::Set, EntityTrait};

mod base;

#[derive(serde::Serialize)]
struct Request {
    maps_uids: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ResponseItem {
    map_uid: String,
    time: i32,
}

#[tokio::test]
async fn test_times() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let player = players::ActiveModel {
            id: Set(1),
            login: Set("player_login".to_owned()),
            name: Set("player_name".to_owned()),
            role: Set(0),
            ..Default::default()
        };

        let maps = (1..=10).map(|map_id| maps::ActiveModel {
            id: Set(map_id),
            game_id: Set(format!("map_{map_id}_uid")),
            name: Set(format!("map_{map_id}_name")),
            player_id: Set(1),
            ..Default::default()
        });

        let times: [_; 10] = array::from_fn(|_| rand::random_range(100..=1000) * 10);

        let records = times
            .iter()
            .zip(1..=10)
            .map(|(time, map_id)| records::ActiveModel {
                record_player_id: Set(1),
                map_id: Set(map_id),
                record_date: Set(chrono::Utc::now().naive_utc()),
                respawn_count: Set(10),
                time: Set(*time),
                flags: Set(682),
                ..Default::default()
            });

        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::post()
            .uri("/player/times")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                maps_uids: vec!["map_1_uid".to_owned()],
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<ResponseItem>>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.len(), 1);
        assert_eq!(body[0].map_uid, "map_1_uid");
        assert_eq!(body[0].time, times[0]);

        let req = test::TestRequest::post()
            .uri("/player/times")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                maps_uids: (1..=10)
                    .map(|map_id| format!("map_{map_id}_uid"))
                    .collect::<Vec<_>>(),
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<ResponseItem>>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.len(), 10);
        for (i, item) in body.into_iter().enumerate() {
            assert_eq!(item.map_uid, format!("map_{}_uid", i + 1));
            assert_eq!(item.time, times[i]);
        }

        anyhow::Ok(())
    })
    .await
}
