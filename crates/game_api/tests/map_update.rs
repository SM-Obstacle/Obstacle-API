use actix_web::test;
use entity::{maps, players};
use sea_orm::{ActiveValue::Set, ColumnTrait as _, EntityTrait, QueryFilter};

mod base;

#[derive(Default, serde::Serialize)]
struct MapAuthor {
    login: String,
    name: String,
    zone_path: String,
}

#[derive(serde::Serialize)]
struct Request {
    name: String,
    map_uid: String,
    cps_number: u32,
    author: MapAuthor,
}

#[tokio::test]
async fn insert_map_existing_player() -> anyhow::Result<()> {
    let player = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/map/insert")
            .set_json(Request {
                map_uid: "map_uid".to_owned(),
                name: "map_name".to_owned(),
                cps_number: 5,
                author: MapAuthor {
                    login: "player_login".to_owned(),
                    ..Default::default()
                },
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 200);

        let map = maps::Entity::find()
            .filter(maps::Column::GameId.eq("map_uid"))
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Map should exist in database"));

        assert_eq!(map.name, "map_name");
        assert_eq!(map.cps_number, Some(5));
        assert_eq!(map.player_id, 1);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn insert_map_with_player() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/map/insert")
            .set_json(Request {
                map_uid: "map_uid".to_owned(),
                name: "map_name".to_owned(),
                cps_number: 5,
                author: MapAuthor {
                    login: "player_login".to_owned(),
                    name: "player_name".to_owned(),
                    zone_path: "France".to_owned(),
                },
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 200);

        let player = players::Entity::find()
            .filter(players::Column::Login.eq("player_login"))
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Player should exist in database"));

        assert_eq!(player.login, "player_login");
        assert_eq!(player.name, "player_name");
        assert_eq!(player.zone_path.as_deref(), Some("France"));
        assert_eq!(player.role, 0);
        assert!(player.join_date.is_some());

        let map = maps::Entity::find()
            .filter(maps::Column::GameId.eq("map_uid"))
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Map should exist in database"));

        assert_eq!(map.name, "map_name");
        assert_eq!(map.cps_number, Some(5));
        assert_eq!(map.player_id, player.id);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn update_map_cps_number() -> anyhow::Result<()> {
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
        cps_number: Set(None),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/map/insert")
            .set_json(Request {
                map_uid: "map_uid".to_owned(),
                name: Default::default(),
                cps_number: 5,
                author: Default::default(),
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 200);

        let map = maps::Entity::find_by_id(1u32)
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Map should exist in database"));

        assert_eq!(map.game_id, "map_uid");
        assert_eq!(map.name, "map_name");
        assert_eq!(map.player_id, 1);
        assert_eq!(map.cps_number, Some(5));

        anyhow::Ok(())
    })
    .await
}
