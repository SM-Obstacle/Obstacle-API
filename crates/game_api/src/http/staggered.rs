use actix_web::{Responder, Scope, web};
use chrono::{DateTime, TimeZone, Utc};
use records_lib::{Database, records_notifier::RecordsNotifier};

use crate::{
    RecordsResult, Res,
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
    fn get_datetime(&self) -> DateTime<Utc> {
        chrono::Utc.timestamp_opt(self.req_tstp, 0).unwrap()
    }
}

type StaggeredBody<B> = web::Json<Staggered<B>>;

#[inline(always)]
async fn staggered_finished(
    _: ApiAvailable,
    mode_version: Option<crate::ModeVersion>,
    MPAuthGuard { login }: MPAuthGuard,
    db: Res<Database>,
    body: StaggeredBody<pf::HasFinishedBody>,
    Res(records_notifier): Res<RecordsNotifier>,
) -> RecordsResult<impl Responder> {
    let time = body.get_datetime();
    player::finished_at_with_pool(
        db.0,
        mode_version.map(|x| x.0),
        login,
        body.0.body,
        time,
        &records_notifier,
    )
    .await
}

#[inline(always)]
async fn staggered_edition_finished(
    MPAuthGuard { login }: MPAuthGuard,
    db: Res<Database>,
    path: web::Path<(String, u32)>,
    body: StaggeredBody<pf::HasFinishedBody>,
    mode_version: crate::ModeVersion,
    Res(records_notifier): Res<RecordsNotifier>,
) -> RecordsResult<impl Responder> {
    let time = body.get_datetime();
    event::edition_finished_at(
        login,
        db,
        path,
        body.0.body,
        time,
        Some(mode_version.0),
        &records_notifier,
    )
    .await
}
