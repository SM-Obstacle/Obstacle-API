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

#[tokio::test]
async fn sorted_finishes_diff_players() -> anyhow::Result<()> {
    let (app, db) = init_app!(
        .route("/finished", web::post().to(super::finished))
    );

    let (records, lb) = with_map(db.clone(), |map, _| async move {
        // Generate the records
        let records = stream::repeat_with(|| async {
            let player = new_player(&db.mysql_pool, false).await?;
            anyhow::Ok((player, get_finish_body(&map)))
        })
        .take(20)
        .buffer_unordered(20)
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
                    .set_json(record)
                    .to_request();

                test::call_service(&app, req).map(|_| ())
            })
            .collect::<()>()
            .await;

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
