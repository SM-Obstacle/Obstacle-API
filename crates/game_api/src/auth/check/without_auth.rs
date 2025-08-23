use records_lib::RedisConnection;
use sea_orm::ConnectionTrait;

use crate::{RecordsErrorKind, RecordsResult, auth::privilege, http::player};

pub async fn check_auth_for<C: ConnectionTrait>(
    conn: &C,
    _redis_conn: &mut RedisConnection,
    login: &str,
    _token: Option<&str>,
    _required: privilege::Flags,
) -> RecordsResult<u32> {
    let player = records_lib::must::have_player(conn, login).await?;

    match player::check_banned(conn, player.id).await? {
        Some(ban) => Err(RecordsErrorKind::BannedPlayer(ban)),
        None => Ok(player.id),
    }
}
