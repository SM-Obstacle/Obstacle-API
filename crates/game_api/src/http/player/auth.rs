#[cfg(auth)]
mod with_auth;
#[cfg(auth)]
use with_auth as imp;

#[cfg(not(auth))]
mod without_auth;
#[cfg(not(auth))]
use without_auth as imp;

#[derive(serde::Serialize)]
struct GetTokenResponse {
    token: String,
}

pub use imp::{get_token, post_give_token};
