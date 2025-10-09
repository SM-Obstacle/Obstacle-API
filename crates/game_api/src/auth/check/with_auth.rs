use deadpool_redis::redis::AsyncCommands as _;
use entity::{players, role};
use records_lib::{RedisConnection, redis_key::mp_token_key};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait as _, QueryFilter as _, QuerySelect as _,
};
use sha256::digest;

use crate::{
    ApiErrorKind, RecordsResult, RecordsResultExt as _, auth::privilege, http::player, internal,
};

pub async fn check_auth_for<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    login: &str,
    token: Option<&str>,
    required: privilege::Flags,
) -> RecordsResult<u32> {
    let Some(token) = token else {
        return Err(ApiErrorKind::Unauthorized);
    };

    let player = records_lib::must::have_player(conn, login).await?;

    if let Some(ban) = player::check_banned(conn, player.id).await? {
        return Err(ApiErrorKind::BannedPlayer(ban));
    };

    let stored_token: Option<String> = redis_conn.get(mp_token_key(login)).await.with_api_err()?;

    if !matches!(stored_token, Some(t) if t == digest(token)) {
        return Err(ApiErrorKind::Unauthorized);
    }

    let role: privilege::Flags = players::Entity::find()
        .filter(players::Column::Id.eq(player.id))
        .inner_join(role::Entity)
        .select_only()
        .column(role::Column::Privileges)
        .into_tuple()
        .one(conn)
        .await
        .with_api_err()?
        .ok_or_else(|| internal!("Player {} should have a role", player.id))?;

    if role & required != required {
        return Err(ApiErrorKind::Forbidden);
    }

    Ok(player.id)
}
