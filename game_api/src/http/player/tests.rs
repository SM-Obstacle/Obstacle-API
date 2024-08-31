use std::future::Future;

use actix_web::{
    http::StatusCode,
    test::{self, read_body},
    web,
};
use futures::{stream, StreamExt};
use rand::Rng as _;
use records_lib::{models, Database};
use tracing_test::traced_test;

use crate::{
    http::{
        map::tests::with_map,
        player_finished::{HasFinishedBody, InsertRecordParams},
    },
    init_app,
    utils::generate_token,
};

pub async fn with_player<F, Fut, R>(db: Database, f: F) -> anyhow::Result<R>
where
    F: FnOnce(models::Player) -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    let login = generate_token(10);

    let player = models::Player {
        id: 0,
        login,
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
    .fetch_one(&db.mysql_pool)
    .await?;

    let player = models::Player { id, ..player };

    tracing::debug!("with player: {player:#?}");

    let out = f(player).await?;

    tracing::debug!("removing player from db...");

    sqlx::query("delete from records where record_player_id = ?")
        .bind(id)
        .execute(&db.mysql_pool)
        .await?;

    sqlx::query("delete from players where id = ?")
        .bind(id)
        .execute(&db.mysql_pool)
        .await?;

    Ok(out)
}

#[traced_test]
#[tokio::test]
async fn many_finishes() -> anyhow::Result<()> {
    const AMOUNT: usize = 1000;

    let (app, db) = init_app!(
        .route("/finished", web::post().to(super::finished))
    );

    let player_db = db.clone();

    let res = with_map(db.clone(), |map| async move {
        with_player(player_db, |player| async move {
            let reqs = stream::repeat_with(|| async {
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

                let body = HasFinishedBody {
                    map_uid: map.game_id.clone(),
                    rest: InsertRecordParams {
                        cps,
                        time,
                        flags: Some(682),
                        respawn_count: 0,
                    },
                };

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
            .take(AMOUNT)
            .buffer_unordered(AMOUNT)
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
