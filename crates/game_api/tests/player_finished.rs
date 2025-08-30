use std::{array, iter};

use actix_web::test;
use entity::{checkpoint_times, global_records, maps, players, records};
use sea_orm::{
    ActiveValue::Set, ColumnTrait as _, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};

mod base;

#[derive(serde::Serialize)]
struct Request {
    map_uid: String,
    time: i32,
    respawn_count: i32,
    flags: Option<u32>,
    cps: Vec<i32>,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct Response {
    has_improved: bool,
    old: i32,
    new: i32,
    current_rank: i32,
    old_rank: i32,
}

#[derive(Debug)]
struct Record {
    map_id: u32,
    player_id: u32,
    time: i32,
    respawn_count: i32,
    flags: u32,
}

impl PartialEq<Record> for global_records::Model {
    fn eq(&self, other: &Record) -> bool {
        self.map_id == other.map_id
            && self.record_player_id == other.player_id
            && self.time == other.time
            && self.respawn_count == other.respawn_count
            && self.time == other.time
            && self.flags == other.flags
    }
}

impl PartialEq<Record> for records::Model {
    fn eq(&self, other: &Record) -> bool {
        self.map_id == other.map_id
            && self.record_player_id == other.player_id
            && self.time == other.time
            && self.respawn_count == other.respawn_count
            && self.time == other.time
            && self.flags == other.flags
    }
}

/// Setup: one player, one map
/// Test: /player/finished of that player on the map, just once
/// Expected: the API response should be coherent and database should contain the inserted record and CP times.
#[tokio::test]
async fn single_try() -> anyhow::Result<()> {
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

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/player/finished")
            .insert_header(("PlayerLogin", "player_login"))
            .set_json(Request {
                map_uid: "map_uid".to_owned(),
                time: 10000,
                flags: Some(682),
                respawn_count: 7,
                cps: vec![0, 2000, 2000, 2000, 2000, 2000],
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
                    .eq(1)
                    .and(global_records::Column::RecordPlayerId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Record should exist in database"));

        assert_eq!(
            record,
            Record {
                map_id: 1,
                player_id: 1,
                time: 10000,
                respawn_count: 7,
                flags: 682,
            }
        );

        // Check CP times saved in DB
        let cp_times = checkpoint_times::Entity::find()
            .filter(checkpoint_times::Column::RecordId.eq(record.record_id))
            .all(&db.sql_conn)
            .await?;

        itertools::assert_equal(
            cp_times,
            iter::once(checkpoint_times::Model {
                cp_num: 0,
                map_id: 1,
                record_id: record.record_id,
                time: 0,
            })
            .chain((1..=5).map(|cp_num| checkpoint_times::Model {
                map_id: 1,
                record_id: record.record_id,
                time: 2000,
                cp_num,
            })),
        );

        anyhow::Ok(())
    })
    .await
}

/// Setup: one player, one map
/// Test: /player/finished of that player on the map, many times
/// Expected: the API response for each request should be coherent,
/// and database should contain the inserted records with their CP times.
/// The global_records view should return the PB of the players.
#[tokio::test]
async fn many_tries() -> anyhow::Result<()> {
    struct TimeWithExpectedResponse {
        time: i32,
        rs_count: i32,
        expected_response: Response,
    }

    let times = [
        TimeWithExpectedResponse {
            time: 10000,
            rs_count: 10,
            expected_response: Response {
                has_improved: true,
                old: 10000,
                new: 10000,
                current_rank: 1,
                old_rank: -1,
            },
        },
        TimeWithExpectedResponse {
            time: 6500,
            rs_count: 7,
            expected_response: Response {
                has_improved: true,
                old: 10000,
                new: 6500,
                current_rank: 1,
                old_rank: 1,
            },
        },
        TimeWithExpectedResponse {
            time: 3000,
            rs_count: 3,
            expected_response: Response {
                has_improved: true,
                old: 6500,
                new: 3000,
                current_rank: 1,
                old_rank: 1,
            },
        },
        TimeWithExpectedResponse {
            time: 5400,
            rs_count: 8,
            expected_response: Response {
                has_improved: false,
                old: 3000,
                new: 5400,
                current_rank: 1,
                old_rank: 1,
            },
        },
    ];

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

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;

        let app = base::get_app(db.clone()).await;

        let reqs = times.iter().map(|time| {
            test::TestRequest::post()
                .uri("/player/finished")
                .insert_header(("PlayerLogin", "player_login"))
                .set_json(Request {
                    map_uid: "map_uid".to_owned(),
                    time: time.time,
                    flags: Some(682),
                    respawn_count: time.rs_count,
                    cps: iter::once(0)
                        .chain(iter::repeat_n(250, 4))
                        .chain(iter::once(time.time - 1000))
                        .collect::<Vec<_>>(),
                })
                .to_request()
        });

        // Check responses for each request
        for (i, req) in reqs.enumerate() {
            let res = test::call_service(&app, req).await;
            let status = res.status();

            let body = test::read_body(res).await;
            let body = base::try_from_slice::<Response>(&body)?;

            assert_eq!(status, 200);
            assert_eq!(body, times[i].expected_response);
        }

        // Sort the times in ascending order, to compare with the records in DB
        let times = {
            let mut times = times;
            times.sort_by_key(|r| r.time);
            times
        };

        let records = records::Entity::find()
            .filter(
                records::Column::MapId
                    .eq(1)
                    .and(records::Column::RecordPlayerId.eq(1)),
            )
            .order_by_asc(records::Column::Time)
            .all(&db.sql_conn)
            .await?;
        let record_ids = records.iter().map(|r| r.record_id).collect::<Vec<_>>();

        // Check records saved in DB
        itertools::assert_equal(
            records,
            times.iter().map(|time| Record {
                flags: 682,
                player_id: 1,
                map_id: 1,
                respawn_count: time.rs_count,
                time: time.time,
            }),
        );

        // Check PB record (global_records view)
        let initial_pb_record = times.iter().min_by_key(|time| time.time).unwrap();
        let pb_record_from_db = global_records::Entity::find()
            .filter(
                global_records::Column::MapId
                    .eq(1)
                    .and(global_records::Column::RecordPlayerId.eq(1)),
            )
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Record should exist in database"));

        assert_eq!(
            pb_record_from_db,
            Record {
                map_id: 1,
                player_id: 1,
                time: initial_pb_record.time,
                respawn_count: initial_pb_record.rs_count,
                flags: 682,
            }
        );

        // Check CP times saved for each record
        for (i, record_id) in record_ids.into_iter().enumerate() {
            let cp_times = checkpoint_times::Entity::find()
                .filter(checkpoint_times::Column::RecordId.eq(record_id))
                .all(&db.sql_conn)
                .await?;

            itertools::assert_equal(
                cp_times,
                iter::once(checkpoint_times::Model {
                    cp_num: 0,
                    map_id: 1,
                    record_id,
                    time: 0,
                })
                .chain((1..=4).map(|cp_num| checkpoint_times::Model {
                    map_id: 1,
                    record_id,
                    time: 250,
                    cp_num,
                }))
                .chain(iter::once(checkpoint_times::Model {
                    map_id: 1,
                    record_id,
                    time: times[i].time - 1000,
                    cp_num: 5,
                })),
            );
        }

        anyhow::Ok(())
    })
    .await
}

/// Similar to [`single_try`], with an additional "ModeVersion" header in the request.
/// The record saved in DB should contain the corresponding mode version.
#[tokio::test]
async fn with_mode_version() -> anyhow::Result<()> {
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

    base::with_db(async |db| {
        players::Entity::insert(player).exec(&db.sql_conn).await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;

        let app = base::get_app(db.clone()).await;

        let req = test::TestRequest::post()
            .uri("/player/finished")
            .insert_header(("PlayerLogin", "player_login"))
            .insert_header(("ObstacleModeVersion", "2.7.4"))
            .set_json(Request {
                map_uid: "map_uid".to_owned(),
                time: 10000,
                flags: Some(682),
                respawn_count: 7,
                cps: vec![0, 2000, 2000, 2000, 2000, 2000],
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

        // Check modeversion column of the record saved in DB
        let modeversion = records::Entity::find()
            .filter(
                records::Column::MapId
                    .eq(1)
                    .and(records::Column::RecordPlayerId.eq(1)),
            )
            .select_only()
            .column(records::Column::Modeversion)
            .into_tuple::<Option<String>>()
            .one(&db.sql_conn)
            .await?
            .unwrap_or_else(|| panic!("Record should exist in database"));
        assert_eq!(modeversion.as_deref(), Some("2.7.4"));

        anyhow::Ok(())
    })
    .await
}

/// Setup: many players, one map
/// Test: /player/finished of each player on the map, with one of them who makes a second try,
/// and goes from last to first
/// Expected: each API response should be coherent (ranks updating accordingly)
#[tokio::test]
async fn many_records() -> anyhow::Result<()> {
    struct TimeWithExpectedResponse {
        player_id: u32,
        time: i32,
        rs_count: i32,
        expected_response: Response,
    }

    let players: [_; 5] = array::from_fn(|player_id| players::ActiveModel {
        id: Set(player_id as u32 + 1),
        login: Set(format!("player_{}_login", player_id + 1)),
        name: Set(format!("player_{}_name", player_id + 1)),
        role: Set(0),
        ..Default::default()
    });

    let map = maps::ActiveModel {
        id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        player_id: Set(1),
        cps_number: Set(Some(5)),
        ..Default::default()
    };

    let times = [
        TimeWithExpectedResponse {
            player_id: 1,
            time: 10000,
            rs_count: 10,
            expected_response: Response {
                has_improved: true,
                old: 10000,
                new: 10000,
                current_rank: 1,
                old_rank: -1,
            },
        },
        TimeWithExpectedResponse {
            player_id: 2,
            time: 6500,
            rs_count: 7,
            expected_response: Response {
                has_improved: true,
                old: 6500,
                new: 6500,
                current_rank: 1,
                old_rank: -1,
            },
        },
        TimeWithExpectedResponse {
            player_id: 3,
            time: 3000,
            rs_count: 3,
            expected_response: Response {
                has_improved: true,
                old: 3000,
                new: 3000,
                current_rank: 1,
                old_rank: -1,
            },
        },
        TimeWithExpectedResponse {
            player_id: 4,
            time: 5400,
            rs_count: 8,
            expected_response: Response {
                has_improved: true,
                old: 5400,
                new: 5400,
                current_rank: 2,
                old_rank: -1,
            },
        },
        TimeWithExpectedResponse {
            player_id: 5,
            time: 9800,
            rs_count: 11,
            expected_response: Response {
                has_improved: true,
                old: 9800,
                new: 9800,
                current_rank: 4,
                old_rank: -1,
            },
        },
        TimeWithExpectedResponse {
            player_id: 1,
            time: 2500,
            rs_count: 1,
            expected_response: Response {
                has_improved: true,
                old: 10000,
                new: 2500,
                current_rank: 1,
                old_rank: 5,
            },
        },
    ];

    base::with_db(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;

        let app = base::get_app(db.clone()).await;

        let reqs = times.iter().map(|time| {
            test::TestRequest::post()
                .uri("/player/finished")
                .insert_header(("PlayerLogin", format!("player_{}_login", time.player_id)))
                .set_json(Request {
                    map_uid: "map_uid".to_owned(),
                    time: time.time,
                    flags: Some(682),
                    respawn_count: time.rs_count,
                    cps: iter::once(0)
                        .chain(iter::repeat_n(250, 4))
                        .chain(iter::once(time.time - 1000))
                        .collect::<Vec<_>>(),
                })
                .to_request()
        });

        // Check responses for each request
        for (i, req) in reqs.enumerate() {
            let res = test::call_service(&app, req).await;
            let status = res.status();

            let body = test::read_body(res).await;
            let body = base::try_from_slice::<Response>(&body)?;

            assert_eq!(status, 200);
            assert_eq!(body, times[i].expected_response);
        }

        anyhow::Ok(())
    })
    .await
}
