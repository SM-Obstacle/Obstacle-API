//! Module used to serve the routes mainly used by the Obstacle gamemode. Each submodule is
//! specific for a route segment.

use actix_web::web::JsonConfig;
use actix_web::{web, Scope};

use tracing_actix_web::RequestId;

use crate::{Database, RecordsResponse};
use actix_web::{web::Data, Responder};

use self::admin::admin_scope;
use self::event::event_scope;
use self::map::map_scope;
use self::player::player_scope;

pub mod admin;
pub mod event;
pub mod map;
pub mod player;

mod overview;
mod pb;
mod player_finished;

pub fn api_route() -> Scope {
    let json_config = JsonConfig::default().limit(1024 * 16);

    web::scope("")
        .app_data(json_config)
        .route("/overview", web::get().to(overview))
        .service(player_scope())
        .service(map_scope())
        .service(admin_scope())
        .service(event_scope())
}

async fn overview(
    req_id: RequestId,
    db: Data<Database>,
    query: overview::OverviewReq,
) -> RecordsResponse<impl Responder> {
    overview::overview(req_id, db, query, None).await
}
