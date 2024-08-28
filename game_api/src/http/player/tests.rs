use actix_web::{
    http::StatusCode,
    test::{self, read_body},
    web,
};
use futures::{stream, StreamExt};
use rand::Rng as _;
use tracing_test::traced_test;

use crate::{
    http::{
        map::tests::with_map_uid,
        player_finished::{HasFinishedBody, InsertRecordParams},
    },
    init_app,
};

#[traced_test]
#[tokio::test]
async fn many_finishes() -> anyhow::Result<()> {
    const AMOUNT: usize = 1000;

    let (app, db) = init_app!(
        .route("/finished", web::post().to(super::finished))
    );

    let res = with_map_uid(db.clone(), |map| async move {
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
                .insert_header(("PlayerLogin", "ahmad3"))
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
    .await?;

    for res in res.into_iter().collect::<Result<Vec<_>, _>>()? {
        assert_eq!(res, Ok(StatusCode::OK));
    }

    Ok(())
}
