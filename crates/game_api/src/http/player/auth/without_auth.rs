use actix_web::{HttpResponse, Responder};

use crate::{RecordsResponse, utils::json};

// Handler used when the `auth` feature is disabled.
// This is used for older versions of the game that still rely on the `/player/get_token` route.
pub async fn get_token() -> RecordsResponse<impl Responder> {
    json(super::GetTokenResponse {
        token: "if you see this gg".to_owned(),
    })
}

#[inline(always)]
pub async fn post_give_token() -> RecordsResponse<impl Responder> {
    Ok(HttpResponse::Ok().finish())
}
