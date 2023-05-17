use actix_web::{
    web::{self, Data, Json, Query},
    Responder, Scope,
};
use serde::{Deserialize, Serialize};
use sqlx::{mysql::MySqlRow, FromRow, Row};

use crate::{
    auth::{self, AuthHeader},
    models::{Player, Role},
    utils::json,
    Database, RecordsError, RecordsResult,
};

use super::player;

pub fn admin_scope() -> Scope {
    web::scope("/admin")
        .route("/del_note", web::post().to(del_note))
        .route("/set_role", web::post().to(set_role))
        .route("/banishments", web::get().to(banishments))
        .route("/ban", web::post().to(ban))
        .route("/unban", web::post().to(unban))
        .route("/player_note", web::get().to(player_note))
}

#[derive(Serialize, Deserialize)]
struct AdminRequest {
    admin_login: String,
    player_login: String,
}

#[derive(Deserialize)]
pub struct DelNoteBody {
    #[serde(flatten)]
    req: AdminRequest,
}

#[derive(Serialize)]
struct DelNoteResponse {
    #[serde(flatten)]
    req: AdminRequest,
}

pub async fn del_note(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<DelNoteBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Admin).await?;
    let body = body.into_inner();
    sqlx::query!(
        "UPDATE players SET admins_note = NULL WHERE login = ?",
        body.req.player_login
    )
    .execute(&db.mysql_pool)
    .await?;

    json(DelNoteResponse { req: body.req })
}

#[derive(Deserialize)]
pub struct SetRoleBody {
    #[serde(flatten)]
    req: AdminRequest,
    role: u8,
}

#[derive(Serialize, Deserialize)]
struct SetRoleResponse {
    #[serde(flatten)]
    req: AdminRequest,
    role: String,
}

pub async fn set_role(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<SetRoleBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Admin).await?;
    let body = body.into_inner();
    sqlx::query!(
        "UPDATE players SET role = ? WHERE login = ?",
        body.role,
        body.req.player_login
    )
    .execute(&db.mysql_pool)
    .await?;

    let role = sqlx::query_scalar!("SELECT role_name FROM role WHERE id = ?", body.role)
        .fetch_one(&db.mysql_pool)
        .await?;

    json(SetRoleResponse {
        req: body.req,
        role,
    })
}

#[derive(Deserialize)]
pub struct BanishmentsBody {
    #[serde(flatten)]
    req: AdminRequest,
}

#[derive(Serialize, FromRow)]
struct BanishmentInner {
    id: u32,
    date_ban: chrono::NaiveDateTime,
    duration: Option<u32>,
    reason: Option<String>,
    banished_by: String,
}

#[derive(Serialize)]
pub struct Banishment {
    #[serde(flatten)]
    inner: BanishmentInner,
    was_reprieved: bool,
    is_current: bool,
}

impl<'r> FromRow<'r, MySqlRow> for Banishment {
    fn from_row(row: &'r MySqlRow) -> Result<Self, sqlx::Error> {
        let is_current = match row.try_get::<i8, _>("is_current") {
            Ok(s) => s,
            Err(sqlx::Error::ColumnNotFound(_)) => 1,
            Err(e) => return Err(e),
        } != 0;

        Ok(Self {
            inner: BanishmentInner::from_row(row)?,
            was_reprieved: row.try_get::<i8, _>("was_reprieved")? != 0,
            is_current,
        })
    }
}

#[derive(Serialize)]
struct BanishmentsResponse {
    #[serde(flatten)]
    req: AdminRequest,
    banishments: Vec<Banishment>,
}

pub async fn banishments(
    db: Data<Database>,
    auth: AuthHeader,
    body: Query<BanishmentsBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Admin).await?;
    let body = body.into_inner();

    let Some(Player { id: player_id, .. }) = player::get_player_from_login(&db, &body.req.player_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.req.player_login));
    };

    let banishments = sqlx::query_as(
        "SELECT
            id, date_ban, duration, was_reprieved, reason,
            (SELECT login FROM players WHERE id = banished_by) AS banished_by,
            id IN (SELECT id FROM banishments WHERE player_id = ?
                AND (date_ban + INTERVAL duration SECOND > NOW() OR duration IS NULL)) AS is_current
        FROM banishments
        WHERE player_id = ?",
    )
    .bind(player_id)
    .bind(player_id)
    .fetch_all(&db.mysql_pool)
    .await?;

    json(BanishmentsResponse {
        req: body.req,
        banishments,
    })
}

#[derive(Deserialize)]
pub struct BanBody {
    #[serde(flatten)]
    req: AdminRequest,
    duration: Option<u32>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct BanResponse {
    #[serde(flatten)]
    req: AdminRequest,
    ban: Banishment,
}

pub async fn ban(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<BanBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Admin).await?;
    let body = body.into_inner();

    let Some(Player { id: player_id, .. }) = player::get_player_from_login(&db, &body.req.player_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.req.player_login));
    };
    let Some(Player { id: admin_id, .. }) = player::get_player_from_login(&db, &body.req.admin_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.req.admin_login));
    };

    let was_reprieved =
        sqlx::query_as::<_, Banishment>("SELECT * FROM banishments WHERE player_id = ?")
            .bind(&body.req.player_login)
            .fetch_optional(&db.mysql_pool)
            .await?
            .is_some();

    let ban_id: u32 = sqlx::query_scalar!(
        "INSERT INTO banishments
                (date_ban,  duration,   was_reprieved,  reason, player_id,  banished_by)
        VALUES  (SYSDATE(), ?,          ?,              ?,      ?,          ?)
        RETURNING id",
        body.duration,
        was_reprieved,
        body.reason,
        player_id,
        admin_id
    )
    .fetch_one(&db.mysql_pool)
    .await?
    .try_get(0)?;

    let ban = sqlx::query_as(
        r#"SELECT id, date_ban, duration, reason, (SELECT login FROM players WHERE id = banished_by) as "banished_by", was_reprieved
        FROM banishments WHERE id = ?"#,
    )
    .bind(ban_id)
    .fetch_one(&db.mysql_pool)
    .await?;

    json(BanResponse { req: body.req, ban })
}

pub async fn is_banned(db: &Database, player_id: u32) -> RecordsResult<Option<Banishment>> {
    let ban = sqlx::query_as(
        "SELECT id, date_ban, duration, was_reprieved, reason,
        (SELECT login FROM players WHERE id = banished_by) as banished_by
        FROM banishments
        WHERE player_id = ? AND (date_ban + INTERVAL duration SECOND > NOW() OR duration IS NULL)",
    )
    .bind(player_id)
    .fetch_optional(&db.mysql_pool)
    .await?;

    Ok(ban)
}

#[derive(Deserialize)]
pub struct UnbanBody {
    #[serde(flatten)]
    req: AdminRequest,
}

#[derive(Serialize)]
struct UnbanResponse {
    #[serde(flatten)]
    inner: AdminRequest,
    ban: Banishment,
}

pub async fn unban(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<UnbanBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Admin).await?;
    let body = body.into_inner();

    let Some(Player { id: player_id, .. }) = player::get_player_from_login(&db, &body.req.player_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.req.player_login));
    };

    let Some(ban) = is_banned(&db, player_id).await? else {
        return Err(RecordsError::PlayerNotBanned(body.req.player_login));
    };

    if let Some(duration) = sqlx::query_scalar!(
        "SELECT SYSDATE() - date_ban FROM banishments WHERE id = ?",
        ban.inner.id
    )
    .fetch_optional(&db.mysql_pool)
    .await?
    {
        println!("duration: {duration}s");
    }

    sqlx::query("UPDATE banishments SET duration = SYSDATE() - date_ban WHERE id = ?")
        .bind(ban.inner.id)
        .execute(&db.mysql_pool)
        .await?;

    json(UnbanResponse {
        inner: body.req,
        ban,
    })
}

#[derive(Deserialize)]
pub struct PlayerNoteBody {
    #[serde(flatten)]
    req: AdminRequest,
}

#[derive(Serialize)]
struct PlayerNoteResponse {
    player_login: String,
    admins_note: Option<String>,
}

pub async fn player_note(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<PlayerNoteBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Admin).await?;
    let body = body.into_inner();

    let Some(admins_note) = sqlx::query_scalar!(
        "SELECT admins_note FROM players WHERE login = ?", body.req.player_login)
    .fetch_optional(&db.mysql_pool).await? else {
        return Err(RecordsError::PlayerNotFound(body.req.player_login));
    };

    json(PlayerNoteResponse {
        player_login: body.req.player_login,
        admins_note,
    })
}
