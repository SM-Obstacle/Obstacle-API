use std::{array, iter, time::Duration};

use actix_http::StatusCode;
use actix_web::test;
use anyhow::Context;
use chrono::SubsecRound;
use entity::{
    event, event_category, event_edition, event_edition_categories, event_edition_maps,
    event_edition_records, maps, players, records,
};
use game_api_lib::TracedError;
use sea_orm::{ActiveValue::Set, EntityTrait};

mod base;

#[derive(Debug, PartialEq, serde::Deserialize)]
struct EventListItem {
    handle: String,
    last_edition_id: i64,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct EventHandleEditionListItem {
    id: u32,
    name: String,
    subtitle: String,
    start_date: chrono::NaiveDateTime,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct EventEditionInGameParams {
    titles_align: String,
    lb_link_align: String,
    authors_align: String,
    put_subtitle_on_newline: bool,
    titles_pos_x: f64,
    titles_pos_y: f64,
    lb_link_pos_x: f64,
    lb_link_pos_y: f64,
    authors_pos_x: f64,
    authors_pos_y: f64,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct MapAuthor {
    login: String,
    name: String,
    zone_path: String,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct NextOpponent {
    login: String,
    name: String,
    time: i32,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct OriginalMap {
    mx_id: i32,
    name: String,
    map_uid: String,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct Map {
    mx_id: i32,
    main_author: MapAuthor,
    name: String,
    map_uid: String,
    bronze_time: i32,
    silver_time: i32,
    gold_time: i32,
    champion_time: i32,
    personal_best: i32,
    next_opponent: NextOpponent,
    original_map: OriginalMap,
}

#[derive(Debug, Default, PartialEq, serde::Deserialize)]
struct Category {
    handle: String,
    name: String,
    banner_img_url: String,
    hex_color: String,
    maps: Vec<Map>,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct EventEditionResponse {
    id: u32,
    name: String,
    subtitle: String,
    authors: Vec<String>,
    start_date: i32,
    end_date: i32,
    ingame_params: EventEditionInGameParams,
    banner_img_url: String,
    banner2_img_url: String,
    mx_id: i32,
    expired: bool,
    original_map_uids: Vec<String>,
    categories: Vec<Category>,
}

#[tokio::test]
async fn events_solely() -> anyhow::Result<()> {
    let events = (1..=3).map(|event_id| event::ActiveModel {
        id: Set(event_id),
        handle: Set(format!("event_{event_id}_handle")),
        ..Default::default()
    });

    base::with_db(async |db| {
        event::Entity::insert_many(events)
            .exec(&db.sql_conn)
            .await?;
        let app = base::get_app(db).await;

        let req = test::TestRequest::get().uri("/event").to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventListItem>>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.len(), 0);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_editions_solely() -> anyhow::Result<()> {
    let events = (1..=3).map(|event_id| event::ActiveModel {
        id: Set(event_id),
        handle: Set(format!("event_{event_id}_handle")),
        ..Default::default()
    });

    let editions = (1..=3).map(|event_id| event_edition::ActiveModel {
        event_id: Set(event_id),
        id: Set(1),
        name: Set(format!("event_{event_id}_1_name")),
        start_date: Set(chrono::Utc::now().naive_utc()),
        is_transparent: Set(0),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        ..Default::default()
    });

    base::with_db(async |db| {
        event::Entity::insert_many(events)
            .exec(&db.sql_conn)
            .await?;
        event_edition::Entity::insert_many(editions)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::get().uri("/event").to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventListItem>>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.len(), 0);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map() -> anyhow::Result<()> {
    let events = (1..=3).map(|event_id| event::ActiveModel {
        id: Set(event_id),
        handle: Set(format!("event_{event_id}_handle")),
        ..Default::default()
    });

    // Declared here to compare them with API responses later
    let start_dates: [_; 3] = array::from_fn(|_| {
        // The test might take less than one second to run, and the API checks the availability
        // of an event edition with second-level precision, so we substract some time to ensure
        // the edition is visible in the test.
        chrono::Utc::now().naive_utc() - Duration::from_secs(3600 * 24)
    });

    let editions = (1..=3).map(|event_id| event_edition::ActiveModel {
        event_id: Set(event_id),
        id: Set(1),
        name: Set(format!("event_{event_id}_1_name")),
        start_date: Set(start_dates[event_id as usize - 1]),
        is_transparent: Set(0),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        ..Default::default()
    });

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_maps = [1, 2].map(|event_id| event_edition_maps::ActiveModel {
        event_id: Set(event_id),
        edition_id: Set(1),
        order: Set(0),
        map_id: Set(1),
        ..Default::default()
    });

    base::with_db(async |db| {
        event::Entity::insert_many(events)
            .exec(&db.sql_conn)
            .await?;
        event_edition::Entity::insert_many(editions)
            .exec(&db.sql_conn)
            .await?;
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event_edition_maps::Entity::insert_many(event_maps)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::get().uri("/event").to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventListItem>>(&body)?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body,
            [1, 2].map(|event_id| EventListItem {
                handle: format!("event_{event_id}_handle"),
                last_edition_id: 1,
            }),
        );

        for event_id in [1, 2] {
            let req = test::TestRequest::get()
                .uri(&format!("/event/event_{event_id}_handle"))
                .to_request();

            let res = test::call_service(&app, req).await;
            let status = res.status();
            let body = test::read_body(res).await;
            let body = base::try_from_slice::<Vec<EventHandleEditionListItem>>(&body)?;

            assert_eq!(status, 200);
            itertools::assert_equal(
                body,
                iter::once(EventHandleEditionListItem {
                    id: 1,
                    name: format!("event_{event_id}_1_name"),
                    subtitle: Default::default(),
                    start_date: start_dates[event_id as usize - 1].trunc_subsecs(0),
                }),
            );

            let req = test::TestRequest::get()
                .uri(&format!("/event/event_{event_id}_handle/1"))
                .to_request();

            let res = test::call_service(&app, req).await;
            let status = res.status();
            let body = test::read_body(res).await;
            let body = base::try_from_slice::<EventEditionResponse>(&body)?;

            assert_eq!(status, 200);
            assert_eq!(
                body,
                EventEditionResponse {
                    id: 1,
                    name: format!("event_{event_id}_1_name"),
                    subtitle: Default::default(),
                    authors: vec![],
                    start_date: start_dates[event_id as usize - 1]
                        .trunc_subsecs(0)
                        .and_utc()
                        .timestamp() as _,
                    end_date: -1,
                    ingame_params: EventEditionInGameParams {
                        titles_align: "L".to_owned(),
                        lb_link_align: "L".to_owned(),
                        authors_align: "R".to_owned(),
                        put_subtitle_on_newline: false,
                        titles_pos_x: -1.,
                        titles_pos_y: -1.,
                        lb_link_pos_x: -1.,
                        lb_link_pos_y: -1.,
                        authors_pos_x: -1.,
                        authors_pos_y: -1.,
                    },
                    banner_img_url: Default::default(),
                    banner2_img_url: Default::default(),
                    mx_id: -1,
                    expired: false,
                    original_map_uids: vec![],
                    categories: vec![Category {
                        maps: vec![Map {
                            mx_id: 0,
                            main_author: MapAuthor {
                                login: "player_login".to_owned(),
                                name: "player_name".to_owned(),
                                zone_path: Default::default(),
                            },
                            name: "map_name".to_owned(),
                            map_uid: "map_uid".to_owned(),
                            bronze_time: -1,
                            silver_time: -1,
                            gold_time: -1,
                            champion_time: -1,
                            personal_best: -1,
                            next_opponent: NextOpponent {
                                login: Default::default(),
                                name: Default::default(),
                                time: -1,
                            },
                            original_map: OriginalMap {
                                mx_id: 0,
                                name: Default::default(),
                                map_uid: Default::default()
                            },
                        },],
                        ..Default::default()
                    }],
                }
            );
        }

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_expired() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(chrono::Utc::now().naive_utc() - Duration::from_secs(3600 * 24)),
        ttl: Set(Some(50)),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(1),
        order: Set(0),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;
        let req = test::TestRequest::get().uri("/event").to_request();
        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventListItem>>(&body)?;

        assert_eq!(status, 200);
        assert_eq!(body.len(), 0);

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_include_expired() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let start_date =
        chrono::Utc::now().naive_utc().trunc_subsecs(0) - Duration::from_secs(3600 * 24);

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(start_date),
        ttl: Set(Some(50)),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(1),
        order: Set(0),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;
        let req = test::TestRequest::get()
            .uri("/event?include_expired=true")
            .to_request();
        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventListItem>>(&body)?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body,
            iter::once(EventListItem {
                handle: "event_handle".to_owned(),
                last_edition_id: 1,
            }),
        );

        let req = test::TestRequest::get()
            .uri("/event/event_handle")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventHandleEditionListItem>>(&body)
            .context("/event/event_handle")?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body,
            iter::once(EventHandleEditionListItem {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                start_date,
            }),
        );

        let req = test::TestRequest::get()
            .uri("/event/event_handle/1")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<EventEditionResponse>(&body).context("/event/event_handle/1")?;

        assert_eq!(status, 200);
        assert_eq!(
            body,
            EventEditionResponse {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                authors: vec![],
                start_date: start_date.and_utc().timestamp() as _,
                end_date: start_date.and_utc().timestamp() as i32 + 50,
                ingame_params: EventEditionInGameParams {
                    titles_align: "L".to_owned(),
                    lb_link_align: "L".to_owned(),
                    authors_align: "R".to_owned(),
                    put_subtitle_on_newline: false,
                    titles_pos_x: -1.,
                    titles_pos_y: -1.,
                    lb_link_pos_x: -1.,
                    lb_link_pos_y: -1.,
                    authors_pos_x: -1.,
                    authors_pos_y: -1.,
                },
                banner_img_url: Default::default(),
                banner2_img_url: Default::default(),
                mx_id: -1,
                expired: true,
                original_map_uids: vec![],
                categories: vec![Category {
                    maps: vec![Map {
                        mx_id: 0,
                        main_author: MapAuthor {
                            login: "player_login".to_owned(),
                            name: "player_name".to_owned(),
                            zone_path: Default::default(),
                        },
                        name: "map_name".to_owned(),
                        map_uid: "map_uid".to_owned(),
                        bronze_time: -1,
                        silver_time: -1,
                        gold_time: -1,
                        champion_time: -1,
                        personal_best: -1,
                        next_opponent: NextOpponent {
                            login: Default::default(),
                            name: Default::default(),
                            time: -1,
                        },
                        original_map: OriginalMap {
                            mx_id: 0,
                            name: Default::default(),
                            map_uid: Default::default()
                        },
                    },],
                    ..Default::default()
                }],
            }
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_not_yet_released() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let start_date =
        chrono::Utc::now().naive_utc().trunc_subsecs(0) - Duration::from_secs(3600 * 24);

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(start_date),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let edition_not_yet_released = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(2),
        name: Set("event_1_2_name".to_owned()),
        start_date: Set(start_date + chrono::Duration::days(1)),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_maps = [1, 2].map(|edition_id| event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(edition_id),
        map_id: Set(1),
        order: Set(0),
        ..Default::default()
    });

    base::with_db(async |db| {
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert_many([edition, edition_not_yet_released])
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert_many(event_maps)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;
        let req = test::TestRequest::get().uri("/event").to_request();
        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventListItem>>(&body)?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body,
            iter::once(EventListItem {
                handle: "event_handle".to_owned(),
                last_edition_id: 1,
            }),
        );

        let req = test::TestRequest::get()
            .uri("/event/event_handle")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body = base::try_from_slice::<Vec<EventHandleEditionListItem>>(&body)
            .context("/event/event_handle")?;

        assert_eq!(status, 200);
        itertools::assert_equal(
            body,
            iter::once(EventHandleEditionListItem {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                start_date,
            }),
        );

        let req = test::TestRequest::get()
            .uri("/event/event_handle/1")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<EventEditionResponse>(&body).context("/event/event_handle/1")?;

        assert_eq!(status, 200);
        assert_eq!(
            body,
            EventEditionResponse {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                authors: vec![],
                start_date: start_date.and_utc().timestamp() as _,
                end_date: -1,
                ingame_params: EventEditionInGameParams {
                    titles_align: "L".to_owned(),
                    lb_link_align: "L".to_owned(),
                    authors_align: "R".to_owned(),
                    put_subtitle_on_newline: false,
                    titles_pos_x: -1.,
                    titles_pos_y: -1.,
                    lb_link_pos_x: -1.,
                    lb_link_pos_y: -1.,
                    authors_pos_x: -1.,
                    authors_pos_y: -1.,
                },
                banner_img_url: Default::default(),
                banner2_img_url: Default::default(),
                mx_id: -1,
                expired: false,
                original_map_uids: vec![],
                categories: vec![Category {
                    maps: vec![Map {
                        mx_id: 0,
                        main_author: MapAuthor {
                            login: "player_login".to_owned(),
                            name: "player_name".to_owned(),
                            zone_path: Default::default(),
                        },
                        name: "map_name".to_owned(),
                        map_uid: "map_uid".to_owned(),
                        bronze_time: -1,
                        silver_time: -1,
                        gold_time: -1,
                        champion_time: -1,
                        personal_best: -1,
                        next_opponent: NextOpponent {
                            login: Default::default(),
                            name: Default::default(),
                            time: -1,
                        },
                        original_map: OriginalMap {
                            mx_id: 0,
                            name: Default::default(),
                            map_uid: Default::default()
                        },
                    },],
                    ..Default::default()
                }],
            }
        );

        let req = test::TestRequest::get()
            .uri("/event/event_handle/2")
            .to_request();

        let res = test::try_call_service(&app, req).await;
        let err = res.err().expect("Request should return error");
        let traced_err = err
            .as_error::<TracedError>()
            .expect("Returned error should be a traced error");
        assert_eq!(traced_err.status_code, Some(StatusCode::BAD_REQUEST));
        // Event edition not found
        assert_eq!(traced_err.r#type, Some(311));

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_medals() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let start_date =
        chrono::Utc::now().naive_utc().trunc_subsecs(0) - Duration::from_secs(3600 * 24);

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(start_date),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(1),
        order: Set(0),
        author_time: Set(Some(10000)),
        gold_time: Set(Some(11000)),
        silver_time: Set(Some(12000)),
        bronze_time: Set(Some(13000)),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::get()
            .uri("/event/event_handle/1")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<EventEditionResponse>(&body).context("/event/event_handle/1")?;

        assert_eq!(status, 200);
        assert_eq!(
            body,
            EventEditionResponse {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                authors: vec![],
                start_date: start_date.and_utc().timestamp() as _,
                end_date: -1,
                ingame_params: EventEditionInGameParams {
                    titles_align: "L".to_owned(),
                    lb_link_align: "L".to_owned(),
                    authors_align: "R".to_owned(),
                    put_subtitle_on_newline: false,
                    titles_pos_x: -1.,
                    titles_pos_y: -1.,
                    lb_link_pos_x: -1.,
                    lb_link_pos_y: -1.,
                    authors_pos_x: -1.,
                    authors_pos_y: -1.,
                },
                banner_img_url: Default::default(),
                banner2_img_url: Default::default(),
                mx_id: -1,
                expired: false,
                original_map_uids: vec![],
                categories: vec![Category {
                    maps: vec![Map {
                        mx_id: 0,
                        main_author: MapAuthor {
                            login: "player_login".to_owned(),
                            name: "player_name".to_owned(),
                            zone_path: Default::default(),
                        },
                        name: "map_name".to_owned(),
                        map_uid: "map_uid".to_owned(),
                        bronze_time: 13000,
                        silver_time: 12000,
                        gold_time: 11000,
                        champion_time: 10000,
                        personal_best: -1,
                        next_opponent: NextOpponent {
                            login: Default::default(),
                            name: Default::default(),
                            time: -1,
                        },
                        original_map: OriginalMap {
                            mx_id: 0,
                            name: Default::default(),
                            map_uid: Default::default()
                        },
                    },],
                    ..Default::default()
                }],
            }
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_two_categories() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let start_date =
        chrono::Utc::now().naive_utc().trunc_subsecs(0) - Duration::from_secs(3600 * 24);

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(start_date),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let maps = [1, 2].map(|map_id| maps::ActiveModel {
        id: Set(map_id),
        player_id: Set(1),
        game_id: Set(format!("map_{map_id}_uid")),
        name: Set(format!("map_{map_id}_name")),
        ..Default::default()
    });

    let categories = [1, 2].map(|cat_id| event_category::ActiveModel {
        id: Set(cat_id),
        handle: Set(format!("category_{cat_id}_handle")),
        name: Set(format!("category_{cat_id}_name")),
        ..Default::default()
    });

    let event_categories = [1, 2].map(|cat_id| event_edition_categories::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        category_id: Set(cat_id),
    });

    let event_maps = [1, 2].map(|id| event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(id),
        category_id: Set(Some(id)),
        order: Set(0),
        ..Default::default()
    });

    base::with_db(async |db| {
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert_many(maps).exec(&db.sql_conn).await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_category::Entity::insert_many(categories)
            .exec(&db.sql_conn)
            .await?;
        event_edition_categories::Entity::insert_many(event_categories)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert_many(event_maps)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::get()
            .uri("/event/event_handle/1")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<EventEditionResponse>(&body).context("/event/event_handle/1")?;

        assert_eq!(status, 200);
        assert_eq!(
            body,
            EventEditionResponse {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                authors: vec![],
                start_date: start_date.and_utc().timestamp() as _,
                end_date: -1,
                ingame_params: EventEditionInGameParams {
                    titles_align: "L".to_owned(),
                    lb_link_align: "L".to_owned(),
                    authors_align: "R".to_owned(),
                    put_subtitle_on_newline: false,
                    titles_pos_x: -1.,
                    titles_pos_y: -1.,
                    lb_link_pos_x: -1.,
                    lb_link_pos_y: -1.,
                    authors_pos_x: -1.,
                    authors_pos_y: -1.,
                },
                banner_img_url: Default::default(),
                banner2_img_url: Default::default(),
                mx_id: -1,
                expired: false,
                original_map_uids: vec![],
                categories: [1, 2]
                    .map(|id| Category {
                        handle: format!("category_{id}_handle"),
                        name: format!("category_{id}_name"),
                        maps: vec![Map {
                            mx_id: 0,
                            main_author: MapAuthor {
                                login: "player_login".to_owned(),
                                name: "player_name".to_owned(),
                                zone_path: Default::default(),
                            },
                            name: format!("map_{id}_name"),
                            map_uid: format!("map_{id}_uid"),
                            bronze_time: -1,
                            silver_time: -1,
                            gold_time: -1,
                            champion_time: -1,
                            personal_best: -1,
                            next_opponent: NextOpponent {
                                login: Default::default(),
                                name: Default::default(),
                                time: -1,
                            },
                            original_map: OriginalMap {
                                mx_id: 0,
                                name: Default::default(),
                                map_uid: Default::default()
                            },
                        }],
                        ..Default::default()
                    })
                    .into(),
            }
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_original_map_transitive_save() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let start_date =
        chrono::Utc::now().naive_utc().trunc_subsecs(0) - Duration::from_secs(3600 * 24);

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(start_date),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let original_map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_original_uid".to_owned()),
        name: Set("map_original_name".to_owned()),
        ..Default::default()
    };

    let map = maps::ActiveModel {
        id: Set(2),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(2),
        order: Set(0),
        original_map_id: Set(Some(1)),
        transitive_save: Set(Some(1)),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert_many([map, original_map])
            .exec(&db.sql_conn)
            .await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::get()
            .uri("/event/event_handle/1")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<EventEditionResponse>(&body).context("/event/event_handle/1")?;

        assert_eq!(status, 200);
        assert_eq!(
            body,
            EventEditionResponse {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                authors: vec![],
                start_date: start_date.and_utc().timestamp() as _,
                end_date: -1,
                ingame_params: EventEditionInGameParams {
                    titles_align: "L".to_owned(),
                    lb_link_align: "L".to_owned(),
                    authors_align: "R".to_owned(),
                    put_subtitle_on_newline: false,
                    titles_pos_x: -1.,
                    titles_pos_y: -1.,
                    lb_link_pos_x: -1.,
                    lb_link_pos_y: -1.,
                    authors_pos_x: -1.,
                    authors_pos_y: -1.,
                },
                banner_img_url: Default::default(),
                banner2_img_url: Default::default(),
                mx_id: -1,
                expired: false,
                original_map_uids: vec!["map_original_uid".to_owned()],
                categories: vec![Category {
                    maps: vec![Map {
                        mx_id: 0,
                        main_author: MapAuthor {
                            login: "player_login".to_owned(),
                            name: "player_name".to_owned(),
                            zone_path: Default::default(),
                        },
                        name: "map_name".to_owned(),
                        map_uid: "map_uid".to_owned(),
                        bronze_time: -1,
                        silver_time: -1,
                        gold_time: -1,
                        champion_time: -1,
                        personal_best: -1,
                        next_opponent: NextOpponent {
                            login: Default::default(),
                            name: Default::default(),
                            time: -1,
                        },
                        original_map: OriginalMap {
                            mx_id: 0,
                            name: "map_original_name".to_owned(),
                            map_uid: "map_original_uid".to_owned(),
                        },
                    }],
                    ..Default::default()
                }],
            }
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_original_map_no_transitive_save() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let start_date =
        chrono::Utc::now().naive_utc().trunc_subsecs(0) - Duration::from_secs(3600 * 24);

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(start_date),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let map_author = players::ActiveModel {
        id: Set(1),
        login: Set("player_login".to_owned()),
        name: Set("player_name".to_owned()),
        role: Set(0),
        ..Default::default()
    };

    let original_map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_original_uid".to_owned()),
        name: Set("map_original_name".to_owned()),
        ..Default::default()
    };

    let map = maps::ActiveModel {
        id: Set(2),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(2),
        order: Set(0),
        original_map_id: Set(Some(1)),
        transitive_save: Set(Some(0)),
        ..Default::default()
    };

    base::with_db(async |db| {
        players::Entity::insert(map_author)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert_many([map, original_map])
            .exec(&db.sql_conn)
            .await?;
        event::Entity::insert(event).exec(&db.sql_conn).await?;
        event_edition::Entity::insert(edition)
            .exec(&db.sql_conn)
            .await?;
        event_edition_maps::Entity::insert(event_map)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::get()
            .uri("/event/event_handle/1")
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<EventEditionResponse>(&body).context("/event/event_handle/1")?;

        assert_eq!(status, 200);
        assert_eq!(
            body,
            EventEditionResponse {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                authors: vec![],
                start_date: start_date.and_utc().timestamp() as _,
                end_date: -1,
                ingame_params: EventEditionInGameParams {
                    titles_align: "L".to_owned(),
                    lb_link_align: "L".to_owned(),
                    authors_align: "R".to_owned(),
                    put_subtitle_on_newline: false,
                    titles_pos_x: -1.,
                    titles_pos_y: -1.,
                    lb_link_pos_x: -1.,
                    lb_link_pos_y: -1.,
                    authors_pos_x: -1.,
                    authors_pos_y: -1.,
                },
                banner_img_url: Default::default(),
                banner2_img_url: Default::default(),
                mx_id: -1,
                expired: false,
                original_map_uids: vec![],
                categories: vec![Category {
                    maps: vec![Map {
                        mx_id: 0,
                        main_author: MapAuthor {
                            login: "player_login".to_owned(),
                            name: "player_name".to_owned(),
                            zone_path: Default::default(),
                        },
                        name: "map_name".to_owned(),
                        map_uid: "map_uid".to_owned(),
                        bronze_time: -1,
                        silver_time: -1,
                        gold_time: -1,
                        champion_time: -1,
                        personal_best: -1,
                        next_opponent: NextOpponent {
                            login: Default::default(),
                            name: Default::default(),
                            time: -1,
                        },
                        original_map: OriginalMap {
                            mx_id: 0,
                            name: Default::default(),
                            map_uid: Default::default(),
                        },
                    }],
                    ..Default::default()
                }],
            }
        );

        anyhow::Ok(())
    })
    .await
}

#[tokio::test]
async fn event_edition_one_map_next_opponent() -> anyhow::Result<()> {
    let event = event::ActiveModel {
        id: Set(1),
        handle: Set("event_handle".to_owned()),
        ..Default::default()
    };

    let start_date =
        chrono::Utc::now().naive_utc().trunc_subsecs(0) - Duration::from_secs(3600 * 24);

    let edition = event_edition::ActiveModel {
        event_id: Set(1),
        id: Set(1),
        name: Set("event_1_1_name".to_owned()),
        start_date: Set(start_date),
        save_non_event_record: Set(0),
        non_original_maps: Set(0),
        is_transparent: Set(0),
        ..Default::default()
    };

    let times_per_player = [10000, 5000, 15000, 3000];

    let players = (1..=times_per_player.len()).map(|player_id| players::ActiveModel {
        id: Set(player_id as _),
        login: Set(format!("player_{player_id}_login")),
        name: Set(format!("player_{player_id}_name")),
        role: Set(0),
        ..Default::default()
    });

    let map = maps::ActiveModel {
        id: Set(1),
        player_id: Set(1),
        game_id: Set("map_uid".to_owned()),
        name: Set("map_name".to_owned()),
        ..Default::default()
    };

    let event_map = event_edition_maps::ActiveModel {
        event_id: Set(1),
        edition_id: Set(1),
        map_id: Set(1),
        order: Set(0),
        ..Default::default()
    };

    let records = times_per_player.iter().enumerate().map(|(i, time)| {
        let id = i as u32 + 1;
        records::ActiveModel {
            record_id: Set(id),
            record_player_id: Set(id),
            map_id: Set(1),
            respawn_count: Set(0),
            record_date: Set(chrono::Utc::now().naive_utc()),
            flags: Set(682),
            time: Set(*time),
            ..Default::default()
        }
    });

    let event_records =
        (1..=times_per_player.len()).map(|record_id| event_edition_records::ActiveModel {
            event_id: Set(1),
            edition_id: Set(1),
            record_id: Set(record_id as _),
        });

    base::with_db(async |db| {
        players::Entity::insert_many(players)
            .exec(&db.sql_conn)
            .await?;
        maps::Entity::insert(map).exec(&db.sql_conn).await?;
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
        event_edition_records::Entity::insert_many(event_records)
            .exec(&db.sql_conn)
            .await?;

        let app = base::get_app(db).await;

        let req = test::TestRequest::get()
            .uri("/event/event_handle/1")
            .insert_header(("PlayerLogin", "player_1_login"))
            .to_request();

        let res = test::call_service(&app, req).await;
        let status = res.status();
        let body = test::read_body(res).await;
        let body =
            base::try_from_slice::<EventEditionResponse>(&body).context("/event/event_handle/1")?;

        assert_eq!(status, 200);
        assert_eq!(
            body,
            EventEditionResponse {
                id: 1,
                name: "event_1_1_name".to_owned(),
                subtitle: Default::default(),
                authors: vec![],
                start_date: start_date.and_utc().timestamp() as _,
                end_date: -1,
                ingame_params: EventEditionInGameParams {
                    titles_align: "L".to_owned(),
                    lb_link_align: "L".to_owned(),
                    authors_align: "R".to_owned(),
                    put_subtitle_on_newline: false,
                    titles_pos_x: -1.,
                    titles_pos_y: -1.,
                    lb_link_pos_x: -1.,
                    lb_link_pos_y: -1.,
                    authors_pos_x: -1.,
                    authors_pos_y: -1.,
                },
                banner_img_url: Default::default(),
                banner2_img_url: Default::default(),
                mx_id: -1,
                expired: false,
                original_map_uids: vec![],
                categories: vec![Category {
                    maps: vec![Map {
                        mx_id: 0,
                        main_author: MapAuthor {
                            login: "player_1_login".to_owned(),
                            name: "player_1_name".to_owned(),
                            zone_path: Default::default(),
                        },
                        name: "map_name".to_owned(),
                        map_uid: "map_uid".to_owned(),
                        bronze_time: -1,
                        silver_time: -1,
                        gold_time: -1,
                        champion_time: -1,
                        personal_best: 10000,
                        next_opponent: NextOpponent {
                            login: "player_2_login".to_owned(),
                            name: "player_2_name".to_owned(),
                            time: 5000,
                        },
                        original_map: OriginalMap {
                            mx_id: 0,
                            name: Default::default(),
                            map_uid: Default::default(),
                        },
                    }],
                    ..Default::default()
                }],
            }
        );

        anyhow::Ok(())
    })
    .await
}
