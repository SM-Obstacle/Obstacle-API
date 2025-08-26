mod base;

use std::{fmt, time::Duration};

use actix_web::test;
use anyhow::Context as _;
use entity::{maps, players, records};
use sea_orm::{ActiveValue::Set, ConnectionTrait, EntityTrait as _};

const ROWS: i32 = 15;
const ROWS_MINUS_TOP_3: i32 = ROWS - 3;

#[derive(Debug, PartialEq, serde::Deserialize)]
struct Row {
    rank: i32,
    login: String,
    nickname: String,
    time: i32,
}

#[derive(Debug, serde::Deserialize)]
struct Response {
    response: Vec<Row>,
}

fn player_id_to_player_active_model(player_id: u32) -> players::ActiveModel {
    players::ActiveModel {
        id: Set(player_id),
        login: Set(PlayerLogin(player_id as _).to_string()),
        name: Set(PlayerName(player_id as _).to_string()),
        role: Set(0),
        ..Default::default()
    }
}

fn new_record(
    map_id: u32,
    player_id: u32,
    time: i32,
    datetime_offset_secs: u64,
) -> records::ActiveModel {
    records::ActiveModel {
        record_player_id: Set(player_id),
        map_id: Set(map_id),
        record_date: Set(chrono::Utc::now().naive_utc() + Duration::from_secs(datetime_offset_secs)),
        respawn_count: Set(0),
        flags: Set(682),
        time: Set(time),
        ..Default::default()
    }
}

fn player_id_to_record_active_model(map_id: u32) -> impl Fn(u32) -> records::ActiveModel {
    move |player_id| {
        new_record(
            map_id,
            player_id,
            5000 + player_id as i32 * 1000,
            player_id as _,
        )
    }
}

struct PlayerLogin(i32);

impl fmt::Display for PlayerLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "player_{}_login", self.0)
    }
}

struct PlayerName(i32);

impl fmt::Display for PlayerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "player_{}_name", self.0)
    }
}

fn player_id_to_row(player_id: i32) -> Row {
    Row {
        rank: player_id,
        login: PlayerLogin(player_id).to_string(),
        nickname: PlayerName(player_id).to_string(),
        time: 5000 + player_id * 1000,
    }
}

async fn insert_sample_map<C: ConnectionTrait>(conn: &C) -> anyhow::Result<u32> {
    let map_id = rand::random_range(..1000);

    maps::Entity::insert(maps::ActiveModel {
        id: Set(map_id),
        game_id: Set("test_map_uid".to_owned()),
        player_id: Set(1),
        name: Set("test_map_name".to_owned()),
        ..Default::default()
    })
    .exec(conn)
    .await
    .context("couldn't insert map")?;

    Ok(map_id)
}

#[tokio::test]
async fn single_record() -> anyhow::Result<()> {
    base::with_db(async |db| {
        players::Entity::insert_many([
            players::ActiveModel {
                id: Set(1),
                login: Set("a_login".to_owned()),
                name: Set("a_name".to_owned()),
                role: Set(0),
                ..Default::default()
            },
            players::ActiveModel {
                id: Set(2),
                login: Set("b_login".to_owned()),
                name: Set("b_name".to_owned()),
                role: Set(0),
                ..Default::default()
            },
        ])
        .exec(&db.sql_conn)
        .await
        .context("couldn't insert players")?;

        let map_id = insert_sample_map(&db.sql_conn).await?;

        records::Entity::insert(records::ActiveModel {
            record_player_id: Set(2),
            map_id: Set(map_id),
            record_date: Set(chrono::Utc::now().naive_utc()),
            respawn_count: Set(0),
            flags: Set(682),
            time: Set(5000),
            ..Default::default()
        })
        .exec(&db.sql_conn)
        .await
        .context("couldn't insert record")?;

        let app = base::get_app(db.clone()).await;
        let req = test::TestRequest::get()
            .uri("/overview?mapId=test_map_uid&playerId=b_login")
            .to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status();

        let body = test::read_body(resp).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.response.len(), 1);
        assert_eq!(body.response[0].login, "b_login");
        assert_eq!(body.response[0].nickname, "b_name");
        assert_eq!(body.response[0].rank, 1);
        assert_eq!(body.response[0].time, 5000);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn no_record() -> anyhow::Result<()> {
    base::with_db(async |db| {
        players::Entity::insert_many([players::ActiveModel {
            id: Set(1),
            login: Set("a_login".to_owned()),
            name: Set("a_name".to_owned()),
            role: Set(0),
            ..Default::default()
        }])
        .exec(&db.sql_conn)
        .await
        .context("couldn't insert players")?;

        insert_sample_map(&db.sql_conn).await?;

        let app = base::get_app(db.clone()).await;
        let req = test::TestRequest::get()
            .uri("/overview?mapId=test_map_uid&playerId=a_login")
            .to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status();

        let body = test::read_body(resp).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.response.len(), 0);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn show_full_lb() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let players = (1..=ROWS as _).map(player_id_to_player_active_model);

        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert players")?;

        let map_id = insert_sample_map(&db.sql_conn).await?;
        let records = (1..=ROWS as _).map(player_id_to_record_active_model(map_id));

        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert records")?;

        let app = base::get_app(db.clone()).await;

        for player_id in 1..=ROWS {
            let req = test::TestRequest::get()
                .uri(&format!(
                    "/overview?mapId=test_map_uid&playerId=player_{player_id}_login"
                ))
                .to_request();

            let resp = test::call_service(&app, req).await;
            let status = resp.status();

            let body = test::read_body(resp).await;
            let body = base::try_from_slice::<Response>(&body)?;

            assert_eq!(status, 200);
            assert_eq!(body.response.len(), ROWS as usize);
            itertools::assert_equal(body.response, (1..=ROWS).map(player_id_to_row));
        }

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn show_last_3() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let players = (1..=30).map(player_id_to_player_active_model);

        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert players")?;

        let map_id = insert_sample_map(&db.sql_conn).await?;

        // Notice the exclusive range. Player 30 has no record on the map.
        let records = (1..30).map(player_id_to_record_active_model(map_id));

        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert records")?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::get()
            .uri("/overview?mapId=test_map_uid&playerId=player_30_login")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();

        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.response.len(), 14);
        itertools::assert_equal(
            body.response,
            (1..=11)
                .map(player_id_to_row)
                .chain((27..=29).map(player_id_to_row)),
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn show_around() -> anyhow::Result<()> {
    const COUNT: i32 = 30;

    base::with_db(async |db| {
        let players = (1..=COUNT as _).map(player_id_to_player_active_model);

        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert players")?;

        let map_id = insert_sample_map(&db.sql_conn).await?;
        let records = (1..=COUNT as _).map(player_id_to_record_active_model(map_id));

        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert records")?;

        let app = base::get_app(db.clone()).await;

        for player_id in (ROWS + 1)..=COUNT {
            let req = test::TestRequest::get()
                .uri(&format!(
                    "/overview?mapId=test_map_uid&playerId=player_{player_id}_login"
                ))
                .to_request();

            let resp = test::call_service(&app, req).await;
            let status = resp.status();

            let body = test::read_body(resp).await;
            let body = base::try_from_slice::<Response>(&body)?;
            println!("ok ({player_id}) {body:#?}");

            assert_eq!(status, 200);
            assert_eq!(body.response.len(), ROWS as usize);

            let expected_range = {
                // Plus one because the current player is in the top half of the framing
                let up = player_id - ROWS_MINUS_TOP_3 / 2 + 1;
                let down = player_id + ROWS_MINUS_TOP_3 / 2;
                if down >= COUNT {
                    (up - (down - COUNT))..=COUNT
                } else {
                    up..=down
                }
            };

            itertools::assert_equal(
                body.response,
                (1..=3)
                    .map(player_id_to_row)
                    .chain(expected_range.map(player_id_to_row)),
            );
        }

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn competition_ranking() -> anyhow::Result<()> {
    base::with_db(async |db| {
        let players = (1..=5).map(player_id_to_player_active_model);

        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert players")?;

        let player_ids = 1..=5;
        let times = [1000, 3000, 3000, 5000, 7000];
        let datetime_offsets = [0, 2, 1, 0, 0];

        let expected_player_ids = [1, 3, 2, 4, 5];
        let expected_ranks = [1, 2, 2, 4, 5];

        let map_id = insert_sample_map(&db.sql_conn).await?;
        let records = player_ids
            .zip(times)
            .zip(datetime_offsets)
            .map(|((player_id, time), offset)| new_record(map_id, player_id, time, offset));

        records::Entity::insert_many(records)
            .exec(&db.sql_conn)
            .await
            .context("couldn't insert records")?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::get()
            .uri(&format!(
                "/overview?mapId=test_map_uid&playerId=player_1_login"
            ))
            .to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status();

        let body = test::read_body(resp).await;
        let body = base::try_from_slice::<Response>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.response.len(), 5);

        itertools::assert_equal(
            body.response,
            expected_player_ids
                .into_iter()
                .zip(times)
                .zip(expected_ranks)
                .map(|((player_id, time), rank)| Row {
                    login: PlayerLogin(player_id).to_string(),
                    nickname: PlayerName(player_id).to_string(),
                    rank,
                    time,
                }),
        );

        anyhow::Ok(())
    })
    .await
}
