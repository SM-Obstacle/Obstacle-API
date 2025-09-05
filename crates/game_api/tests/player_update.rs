use actix_web::test;
use chrono::SubsecRound;
use entity::players;
use sea_orm::{ActiveValue::Set, ColumnTrait as _, EntityTrait, QueryFilter};

mod base;

#[derive(serde::Serialize)]
struct Request {
    login: String,
    name: String,
    zone_path: String,
}

#[tokio::test]
async fn test_insert() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/player/update")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                login: "player_login".to_owned(),
                name: "player_name".to_owned(),
                zone_path: "France".to_owned(),
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 200);

        let player = players::Entity::find()
            .filter(
                players::Column::Login
                    .eq("player_login")
                    .and(players::Column::Name.eq("player_name"))
                    .and(players::Column::ZonePath.eq("France")),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Player should be inserted in database"));

        assert_eq!(player.login, "player_login");
        assert_eq!(player.name, "player_name");
        assert_eq!(player.zone_path.as_deref(), Some("France"));
        assert!(player.join_date.is_some());
        assert_eq!(player.role, 0);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn test_update_name_zone_path() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let now = chrono::Utc::now().naive_utc();

        let player = players::ActiveModel {
            login: Set("player_login".to_owned()),
            name: Set("player_name".to_owned()),
            zone_path: Set(Some("France".to_owned())),
            role: Set(0),
            join_date: Set(Some(now)),
            ..Default::default()
        };

        let player_id = players::Entity::insert(player)
            .exec(&db.sql_conn)
            .await?
            .last_insert_id;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/player/update")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                login: "player_login".to_owned(),
                name: "booga".to_owned(),
                zone_path: "Germany".to_owned(),
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 200);

        let player = players::Entity::find_by_id(player_id)
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Player should exist in database"));
        assert_eq!(player.login, "player_login");
        assert_eq!(player.name, "booga");
        assert_eq!(player.zone_path.as_deref(), Some("Germany"));
        assert_eq!(player.join_date, Some(now.trunc_subsecs(0)));

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn test_insert_zone_path_join_date() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let player = players::ActiveModel {
            login: Set("player_login".to_owned()),
            name: Set("player_name".to_owned()),
            role: Set(0),
            ..Default::default()
        };

        let player_id = players::Entity::insert(player)
            .exec(&db.sql_conn)
            .await?
            .last_insert_id;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/player/update")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                login: "player_login".to_owned(),
                name: "booga".to_owned(),
                zone_path: "Germany".to_owned(),
            })
            .to_request();

        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), 200);

        let player = players::Entity::find_by_id(player_id)
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Player should exist in database"));
        assert_eq!(player.login, "player_login");
        assert_eq!(player.name, "booga");
        assert_eq!(player.zone_path.as_deref(), Some("Germany"));
        assert!(player.join_date.is_some());

        anyhow::Ok(())
    })
    .await
}
