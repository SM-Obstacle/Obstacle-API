use deadpool_redis::redis::AsyncCommands as _;
use records_lib::{
    RedisConnection, gen_random_str,
    redis_key::{mp_token_key, web_token_key},
};
use sha256::digest;

use crate::{RecordsResult, RecordsResultExt as _};

/// Generates a ManiaPlanet and Website token for the player with the provided login.
///
/// The player might not yet exist in the database.
/// It returns a couple of (ManiaPlanet token ; Website token).
///
/// The tokens are stored in the Redis database.
pub async fn gen_token_for(
    redis_conn: &mut RedisConnection,
    login: &str,
) -> RecordsResult<(String, String)> {
    let mp_token = gen_random_str(256);
    let web_token = gen_random_str(32);

    let mp_key = mp_token_key(login);
    let web_key = web_token_key(login);

    let ex = crate::env().auth_token_ttl as _;

    let mp_token_hash = digest(&*mp_token);
    let web_token_hash = digest(&*web_token);

    let _: () = redis_conn
        .set_ex(mp_key, mp_token_hash, ex)
        .await
        .with_api_err()?;
    let _: () = redis_conn
        .set_ex(web_key, web_token_hash, ex)
        .await
        .with_api_err()?;
    Ok((mp_token, web_token))
}
