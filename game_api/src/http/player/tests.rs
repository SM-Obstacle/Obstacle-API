use std::future::Future;

use actix_web::{
    http::StatusCode,
    test::{self, read_body},
    web,
};
use futures::{stream, FutureExt as _, StreamExt as _, TryStreamExt as _};
use itertools::Itertools;
use rand::Rng as _;
use records_lib::{
    leaderboard::{leaderboard, sql_query, CompetRankingByKeyIter},
    models, Database, MySqlPool,
};
use tracing_test::traced_test;

use crate::{
    http::{
        map::tests::with_map,
        overview,
        player_finished::{HasFinishedBody, InsertRecordParams},
    },
    init_app,
    utils::generate_token,
};

pub async fn new_player(pool: &MySqlPool, debug_msg: bool) -> anyhow::Result<models::Player> {
    let player = models::Player {
        id: 0,
        login: generate_token(10),
        name: "".to_owned(),
        join_date: Some(chrono::Utc::now().naive_utc()),
        zone_path: None,
        admins_note: None,
        role: 0,
    };

    let id = sqlx::query_scalar(
        "insert into players (login, name, join_date, zone_path, admins_note, role)
        values (?, ?, ?, ?, ?, ?) returning id",
    )
    .bind(&player.login)
    .bind(&player.name)
    .bind(&player.join_date)
    .bind(&player.zone_path)
    .bind(&player.admins_note)
    .bind(player.role)
    .fetch_one(pool)
    .await?;

    let player = models::Player { id, ..player };

    if debug_msg {
        tracing::debug!("with player: {player:#?}");
    }

    Ok(player)
}

pub async fn remove_player(pool: &MySqlPool, id: u32, debug_msg: bool) -> anyhow::Result<()> {
    if debug_msg {
        tracing::debug!("removing player {id} from db...");
    }

    sqlx::query("delete from records where record_player_id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    sqlx::query("delete from players where id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn with_player_debug<F, Fut, R>(db: Database, debug_msg: bool, f: F) -> anyhow::Result<R>
where
    F: FnOnce(models::Player) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    let player = new_player(&db.mysql_pool, debug_msg).await?;
    let id = player.id;
    let out = f(player).await?;
    remove_player(&db.mysql_pool, id, debug_msg).await?;
    Ok(out)
}

pub async fn with_player<F, Fut, R>(db: Database, f: F) -> anyhow::Result<R>
where
    F: FnOnce(models::Player) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    with_player_debug(db, true, f).await
}

fn get_finish_body(map: &models::Map) -> HasFinishedBody {
    let (cps, time) = {
        let mut rng = rand::thread_rng();
        match map.cps_number {
            Some(cps_number) => {
                let mut time = 0;

                let cps = rng
                    .sample_iter(rand::distributions::Uniform::new_inclusive(0, 5000))
                    .take((cps_number + 1) as _)
                    .map(|cptime| {
                        time += cptime;
                        cptime
                    })
                    .collect();

                (cps, time)
            }
            None => (Vec::new(), rng.gen()),
        }
    };

    HasFinishedBody {
        map_uid: map.game_id.clone(),
        rest: InsertRecordParams {
            cps,
            time,
            flags: Some(682),
            respawn_count: 0,
        },
    }
}

const PAR_FINISH_AMOUNT: usize = 1000;

#[traced_test]
#[tokio::test]
async fn many_finishes_same_player() -> anyhow::Result<()> {
    let (app, db) = init_app!(
        .route("/finished", web::post().to(super::finished))
    );

    let player_db = db.clone();

    let res = with_map(db.clone(), |map, _| async move {
        with_player(player_db, |player| async move {
            let reqs = stream::repeat_with(|| async {
                let body = get_finish_body(&map);

                let req = test::TestRequest::post()
                    .insert_header(("PlayerLogin", player.login.as_str()))
                    .uri("/finished")
                    .set_json(body)
                    .to_request();
                let resp = test::call_service(&app, req).await;

                let res = match resp.status() {
                    status if status.is_success() => Ok(status),
                    _ => Err(read_body(resp).await),
                };

                anyhow::Ok(res)
            })
            .take(PAR_FINISH_AMOUNT)
            .buffer_unordered(PAR_FINISH_AMOUNT)
            .collect::<Vec<_>>()
            .await;

            Ok(reqs)
        })
        .await
    })
    .await?;

    for res in res.into_iter().collect::<Result<Vec<_>, _>>()? {
        assert_eq!(res, Ok(StatusCode::OK));
    }

    Ok(())
}

#[traced_test]
#[tokio::test]
async fn many_finishes_diff_players() -> anyhow::Result<()> {
    let (app, db) = init_app!(
        .route("/finished", web::post().to(super::finished))
    );

    let res = with_map(db.clone(), |map, _| async move {
        let reqs = stream::repeat_with(|| async {
            with_player_debug(db.clone(), false, |player| {
                let body = get_finish_body(&map);

                let req = test::TestRequest::post()
                    .insert_header(("PlayerLogin", player.login.as_str()))
                    .uri("/finished")
                    .set_json(body)
                    .to_request();

                async {
                    let resp = test::call_service(&app, req).await;

                    let res = match resp.status() {
                        status if status.is_success() => Ok(status),
                        _ => Err(read_body(resp).await),
                    };

                    anyhow::Ok(res)
                }
            })
            .await
        })
        .take(PAR_FINISH_AMOUNT)
        .buffer_unordered(PAR_FINISH_AMOUNT)
        .collect::<Vec<_>>()
        .await;

        Ok(reqs)
    })
    .await?;

    for res in res.into_iter().collect::<Result<Vec<_>, _>>()? {
        assert_eq!(res, Ok(StatusCode::OK));
    }

    Ok(())
}

async fn generate_records<F, Fut>(
    pool: &MySqlPool,
    map: &models::Map,
    amount: usize,
    mut call_service: F,
) -> anyhow::Result<Vec<(usize, models::Player, HasFinishedBody)>>
where
    F: FnMut(test::TestRequest) -> Fut,
    Fut: Future<Output = ()>,
{
    let records = stream::repeat_with(|| async {
        let player = new_player(pool, false).await?;
        anyhow::Ok((player, get_finish_body(map)))
    })
    .take(amount)
    .buffer_unordered(amount)
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .sorted_unstable_by_key(|item| match item {
        Ok((_, r)) => r.rest.time,
        _ => -1,
    })
    .compet_rank_by_key(|i| match i {
        Ok((_, r)) => r.rest.time,
        _ => -1,
    })
    .map(|(i, r)| match r {
        Ok((p, r)) => Ok((i, p, r)),
        Err(e) => Err(e),
    })
    .collect::<Result<Vec<_>, _>>()?;

    stream::iter(records.iter())
        .then(|(_, player, record)| {
            let req = test::TestRequest::post()
                .insert_header(("PlayerLogin", player.login.clone()))
                .uri("/finished")
                .set_json(record);

            (call_service)(req)
        })
        .collect::<()>()
        .await;
    Ok(records)
}

#[traced_test]
#[tokio::test]
async fn sorted_finishes_diff_players() -> anyhow::Result<()> {
    const FINISH_AMOUNT: usize = 50;

    let (app, db) = init_app!(
        .route("/finished", web::post().to(super::finished))
    );

    let (records, lb) = with_map(db.clone(), |map, _| async move {
        // Generate the records
        let records = generate_records(&db.mysql_pool, &map, FINISH_AMOUNT, |req| {
            test::call_service(&app, req.to_request()).map(|_| ())
        })
        .await?;

        let lb = leaderboard(
            &db,
            map.id,
            Default::default(),
            &sql_query(Default::default(), None, None),
        )
        .try_collect::<Vec<_>>()
        .await?;

        // Remove the generated players
        for (_, p, _) in &records {
            remove_player(&db.mysql_pool, p.id, false).await?;
        }

        Ok((records, lb))
    })
    .await?;

    for ((expected_rank, player, body), row) in records.into_iter().zip(lb) {
        assert_eq!(row.rank, expected_rank as i32);
        assert_eq!(row.player, player.login);
        assert_eq!(row.time, body.rest.time);
    }

    Ok(())
}

#[traced_test]
#[tokio::test]
async fn overview_after_finishes() -> anyhow::Result<()> {
    const FINISH_AMOUNT: usize = 100;

    let (app, db) = init_app!(
        .route("/finished", web::post().to(super::finished))
        .route("/overview", web::get().to(crate::http::overview))
    );

    let (records, r1) = with_map(db.clone(), |map, _| async move {
        // Generate the records
        let records = generate_records(&db.mysql_pool, &map, FINISH_AMOUNT, |req| {
            test::call_service(&app, req.to_request()).map(|_| ())
        })
        .await?;

        let r1_login = records
            .first()
            .map(|(_, p, _)| p.login.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing r1"))?;

        let req = test::TestRequest::get()
            .uri(&format!(
                "/overview?playerId={r1_login}&mapId={}",
                map.game_id
            ))
            .to_request();

        let body = test::call_service(&app, req).await;

        // Remove the generated players
        for (_, p, _) in &records {
            remove_player(&db.mysql_pool, p.id, false).await?;
        }

        Ok((records, body))
    })
    .await?;

    match r1.status() {
        StatusCode::OK => (),
        other => {
            let txt = test::read_body(r1).await;
            panic!("overview request failed: {other} {txt:?}");
        }
    }
    let r1: overview::ResponseBody = test::read_body_json(r1).await;

    let (_, r1_p, r1_b) = records.first().expect("missing first record");
    let r1_r = r1.response.first().expect("missing first overview record");
    assert_eq!(r1_r.rank, 1);
    assert_eq!(r1_r.login, r1_p.login);
    assert_eq!(r1_r.time, r1_b.rest.time);

    Ok(())
}
