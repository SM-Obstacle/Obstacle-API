use entity::role;
use records_lib::RedisConnection;
use sea_orm::{ConnectionTrait, EntityTrait, QuerySelect};

use crate::{
    ApiErrorKind, RecordsResult, RecordsResultExt, auth::privilege, http::player, internal,
};

pub async fn check_auth_for<C: ConnectionTrait>(
    conn: &C,
    _redis_conn: &mut RedisConnection,
    login: &str,
    _token: Option<&str>,
    required: privilege::Flags,
) -> RecordsResult<u32> {
    let player = records_lib::must::have_player(conn, login).await?;

    if let Some(ban) = player::check_banned(conn, player.id).await? {
        return Err(ApiErrorKind::BannedPlayer(ban));
    }

    let privileges: privilege::Flags = role::Entity::find_by_id(player.role)
        .select_only()
        .column(role::Column::Privileges)
        .into_tuple()
        .one(conn)
        .await
        .with_api_err()?
        .ok_or_else(|| internal!("Role {} should exist", player.role))?;

    if privileges & required == required {
        Ok(player.id)
    } else {
        Err(ApiErrorKind::Unauthorized)
    }
}
