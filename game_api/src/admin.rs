use actix_web::{
    web::{Data, Json},
    Responder,
};
use serde::{Deserialize, Serialize};
use sqlx::{mysql::MySqlRow, FromRow, Row};

use crate::{
    auth::{AuthFields, ExtractAuthFields},
    models::{Player, Role},
    player,
    utils::{wrap_xml, xml_seq},
    AuthState, Database, RecordsError, RecordsResult,
};

#[derive(Serialize, Deserialize)]
struct AdminRequest {
    admin_login: String,
    player_login: String,
}

#[derive(Deserialize)]
pub struct DelNoteBody {
    secret: String,
    #[serde(flatten)]
    req: AdminRequest,
}

impl ExtractAuthFields for DelNoteBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.req.admin_login,
        }
    }
}

#[derive(Serialize)]
struct DelNoteResponse {
    req: AdminRequest,
}

pub async fn del_note(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<DelNoteBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Admin, &body).await?;
    sqlx::query!(
        "UPDATE players SET admins_note = NULL WHERE login = ?",
        body.req.player_login
    )
    .execute(&db.mysql_pool)
    .await?;

    wrap_xml(&DelNoteResponse { req: body.req })
}

#[derive(Deserialize)]
pub struct SetRoleBody {
    secret: String,
    #[serde(flatten)]
    req: AdminRequest,
    role: u8,
}

impl ExtractAuthFields for SetRoleBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.req.admin_login,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SetRoleResponse {
    req: AdminRequest,
    role: String,
}

pub async fn set_role(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<SetRoleBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Admin, &body).await?;
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

    wrap_xml(&SetRoleResponse {
        req: body.req,
        role,
    })
}

#[derive(Deserialize)]
pub struct BanishmentsBody {
    secret: String,
    #[serde(flatten)]
    req: AdminRequest,
}

impl ExtractAuthFields for BanishmentsBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.req.admin_login,
        }
    }
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
    req: AdminRequest,
    // = Vec<Banishment>, but sequence serializing is fucked up
    banishments: String,
}

pub async fn banishments(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<BanishmentsBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Moderator, &body).await?;

    let Some(Player { id: player_id, .. }) = player::get_player_from_login(&db, &body.req.player_login).await? else {
        return Err(RecordsError::PlayerNotFound(body.req.player_login));
    };

    let bans = sqlx::query_as(
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

    let banishments = xml_seq::<Banishment>(None, &bans)?;
    wrap_xml(&BanishmentsResponse {
        req: body.req,
        banishments,
    })
}

#[derive(Deserialize)]
pub struct BanBody {
    secret: String,
    #[serde(flatten)]
    req: AdminRequest,
    duration: Option<u32>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct BanResponse {
    req: AdminRequest,
    ban: Banishment,
}

impl ExtractAuthFields for BanBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.req.admin_login,
        }
    }
}

pub async fn ban(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<BanBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Admin, &body).await?;

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

    wrap_xml(&BanResponse { req: body.req, ban })
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
    secret: String,
    #[serde(flatten)]
    req: AdminRequest,
}

impl ExtractAuthFields for UnbanBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.req.admin_login,
        }
    }
}

#[derive(Serialize)]
struct UnbanResponse {
    inner: AdminRequest,
    ban: Banishment,
}

pub async fn unban(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<UnbanBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Admin, &body).await?;

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

    wrap_xml(&UnbanResponse {
        inner: body.req,
        ban,
    })
}

#[derive(Deserialize)]
pub struct PlayerNoteBody {
    secret: String,
    #[serde(flatten)]
    req: AdminRequest,
}

impl ExtractAuthFields for PlayerNoteBody {
    fn get_auth_fields(&self) -> AuthFields {
        AuthFields {
            token: &self.secret,
            login: &self.req.admin_login,
        }
    }
}

#[derive(Serialize)]
struct PlayerNoteResponse {
    player_login: String,
    admins_note: Option<String>,
}

pub async fn player_note(
    db: Data<Database>,
    state: Data<AuthState>,
    body: Json<PlayerNoteBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();
    state.check_auth_for(&db, Role::Admin, &body).await?;

    let Some(admins_note) = sqlx::query_scalar!(
        "SELECT admins_note FROM players WHERE login = ?", body.req.player_login)
    .fetch_optional(&db.mysql_pool).await? else {
        return Err(RecordsError::PlayerNotFound(body.req.player_login));
    };

    wrap_xml(&PlayerNoteResponse {
        player_login: body.req.player_login,
        admins_note,
    })
}
