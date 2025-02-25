use actix_web::{Responder, Scope, web};
use chrono::TimeZone;
use records_lib::Database;
use tracing_actix_web::RequestId;

use crate::{
    RecordsResponse, Res,
    auth::{ApiAvailable, MPAuthGuard},
};

use super::{event, player, player_finished as pf};

pub fn staggered_scope() -> Scope {
    web::scope("/staggered")
        .route("/player/finished", web::post().to(staggered_finished))
        .route(
            "/event/{event_handle}/{event_edition}/player/finished",
            web::post().to(staggered_edition_finished),
        )
}

#[derive(serde::Deserialize)]
struct Staggered<B> {
    req_tstp: i64,
    body: B,
}

impl<B> Staggered<B> {
    fn get_time(&self) -> chrono::NaiveDateTime {
        chrono::Utc
            .timestamp_opt(self.req_tstp, 0)
            .unwrap()
            .naive_utc()
    }
}

type StaggeredBody<B> = web::Json<Staggered<B>>;

#[inline(always)]
async fn staggered_finished(
    _: ApiAvailable,
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    db: Res<Database>,
    body: StaggeredBody<pf::HasFinishedBody>,
) -> RecordsResponse<impl Responder> {
    let time = body.get_time();
    player::finished_at(req_id, login, db, body.0.body, time).await
}

#[inline(always)]
async fn staggered_edition_finished(
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    db: Res<Database>,
    path: web::Path<(String, u32)>,
    body: StaggeredBody<pf::HasFinishedBody>,
) -> RecordsResponse<impl Responder> {
    let time = body.get_time();
    event::edition_finished_at(login, req_id, db, path, body.0.body, time).await
}
