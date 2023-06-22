use actix_web::{HttpResponse, Responder};
use rand::Rng;
use serde::Serialize;

/// Converts the provided body to a `200 OK` JSON responses.
pub fn json<T: Serialize, E>(obj: T) -> Result<impl Responder, E> {
    Ok(HttpResponse::Ok().json(obj))
}

/// Checks for any repeated item in a slice.
pub fn any_repeated<T: PartialEq>(slice: &[T]) -> bool {
    for (i, t) in slice.iter().enumerate() {
        if slice.split_at(i + 1).1.iter().any(|x| x == t) {
            return true;
        }
    }
    false
}

/// Returns the formatted string of the provided key
fn format_key(sub: String) -> String {
    format!("v3:{sub}")
}

/// Returns the formatted string for the map id, used to store its leaderboard
pub fn format_map_key(map_id: u32) -> String {
    format_key(format!("lb:{map_id}"))
}

fn inner_format_token_key(prefix: &str, login: &str) -> String {
    format_key(format!("token:{prefix}:{login}"))
}

/// Returns the formatted string for the website token key of the player with the provided login.
pub fn format_web_token_key(login: &str) -> String {
    inner_format_token_key("web", login)
}

/// Returns the formatted string for the ManiaPlanet token key of the player with the provided login.
pub fn format_mp_token_key(login: &str) -> String {
    inner_format_token_key("mp", login)
}

/// Returns a randomly-generated token with the `len` length. It contains alphanumeric characters.
pub fn generate_token(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .map(char::from)
        .take(len)
        .collect()
}
