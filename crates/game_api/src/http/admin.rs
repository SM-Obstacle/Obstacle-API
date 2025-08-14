use actix_web::{
    HttpResponse, Responder, Scope,
    web::{self, Json},
};
use entity::{banishments, current_bans, players};
use futures::TryStreamExt;
use records_lib::Database;
use sea_orm::{
    ActiveValue::Set,
    ColumnTrait as _, ConnectionTrait, DatabaseConnection, EntityTrait, FromQueryResult,
    QueryFilter as _, QuerySelect, RelationTrait as _, StatementBuilder,
    prelude::Expr,
    sea_query::{ExprTrait, Func, Query},
};
use serde::{Deserialize, Serialize};
use tracing_actix_web::RequestId;

use crate::{
    FitRequestId, RecordsErrorKind, RecordsResponse, RecordsResult, RecordsResultExt, Res,
    auth::{MPAuthGuard, privilege},
    utils::json,
};

pub fn admin_scope() -> Scope {
    web::scope("/admin")
        .route("/del_note", web::post().to(del_note))
        .route("/set_role", web::post().to(set_role))
        .route("/banishments", web::get().to(banishments))
        .route("/ban", web::post().to(ban))
        .route("/unban", web::post().to(unban))
        .route("/player_note", web::get().to(player_note))
}

#[derive(Deserialize)]
pub struct DelNoteBody {
    player_login: String,
}

pub async fn del_note(
    _: MPAuthGuard<{ privilege::ADMIN }>,
    req_id: RequestId,
    db: Res<Database>,
    Json(body): Json<DelNoteBody>,
) -> RecordsResponse<impl Responder> {
    sqlx::query("UPDATE players SET admins_note = NULL WHERE login = ?")
        .bind(&body.player_login)
        .execute(&db.mysql_pool)
        .await
        .with_api_err()
        .fit(req_id)?;

    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct SetRoleBody {
    player_login: String,
    role: u8,
}

#[derive(Serialize, Deserialize)]
struct SetRoleResponse {
    player_login: String,
    role: String,
}

pub async fn set_role(
    _: MPAuthGuard<{ privilege::ADMIN }>,
    req_id: RequestId,
    db: Res<Database>,
    Json(body): Json<SetRoleBody>,
) -> RecordsResponse<impl Responder> {
    sqlx::query("UPDATE players SET role = ? WHERE login = ?")
        .bind(body.role)
        .bind(&body.player_login)
        .execute(&db.mysql_pool)
        .await
        .with_api_err()
        .fit(req_id)?;

    let role = sqlx::query_scalar("SELECT role_name FROM role WHERE id = ?")
        .bind(body.role)
        .fetch_one(&db.mysql_pool)
        .await
        .with_api_err()
        .fit(req_id)?;

    json(SetRoleResponse {
        player_login: body.player_login,
        role,
    })
}

#[derive(Deserialize)]
pub struct BanishmentsBody {
    player_login: String,
}

#[derive(Serialize, FromQueryResult)]
struct BanishmentInner {
    id: u32,
    date_ban: chrono::NaiveDateTime,
    duration: Option<u32>,
    reason: Option<String>,
    banished_by: String,
    was_reprieved: i8,
    is_current: i8,
}

#[derive(Serialize)]
pub struct Banishment {
    #[serde(flatten)]
    inner: BanishmentInner,
    was_reprieved: bool,
    is_current: bool,
}

#[derive(Serialize)]
struct BanishmentsResponse {
    player_login: String,
    banishments: Vec<Banishment>,
}

pub async fn banishments(
    _: MPAuthGuard<{ privilege::ADMIN }>,
    req_id: RequestId,
    db: Res<Database>,
    web::Query(body): web::Query<BanishmentsBody>,
) -> RecordsResponse<impl Responder> {
    let conn = DatabaseConnection::from(db.0.mysql_pool);

    let player_id = records_lib::must::have_player(&conn, &body.player_login)
        .await
        .fit(req_id)?
        .id;

    let banishments = banishments::Entity::find()
        .filter(banishments::Column::PlayerId.eq(player_id))
        .join_as(
            sea_orm::JoinType::InnerJoin,
            banishments::Relation::Players1.def(),
            "ban_author_player",
        )
        .select_only()
        .column(banishments::Column::Id)
        .column(banishments::Column::DateBan)
        .column(banishments::Column::Duration)
        .column(banishments::Column::WasReprieved)
        .column(banishments::Column::Reason)
        .column_as(
            Expr::col(("ban_author_player", players::Column::Login)),
            "banished_by",
        )
        .column_as(
            banishments::Column::Id.in_subquery(
                Query::select()
                    .from(banishments::Entity)
                    .column(banishments::Column::Id)
                    .and_where(
                        banishments::Column::PlayerId.eq(player_id).and(
                            Func::cust("TIMESTAMPADD")
                                .arg(Expr::custom_keyword("SECOND"))
                                .arg(Expr::col(banishments::Column::Duration))
                                .arg(Expr::col(banishments::Column::DateBan))
                                .gt(Func::cust("NOW"))
                                .or(banishments::Column::Duration.is_null()),
                        ),
                    )
                    .take(),
            ),
            "is_current",
        )
        .into_model::<BanishmentInner>()
        .stream(&conn)
        .await
        .with_api_err()
        .fit(req_id)?
        .map_ok(|ban| Banishment {
            was_reprieved: ban.was_reprieved != 0,
            is_current: ban.is_current != 0,
            inner: ban,
        })
        .try_collect()
        .await
        .with_api_err()
        .fit(req_id)?;

    json(BanishmentsResponse {
        player_login: body.player_login,
        banishments,
    })
}

#[derive(Deserialize)]
pub struct BanBody {
    player_login: String,
    duration: Option<i64>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct BanResponse {
    player_login: String,
    ban: Banishment,
}

pub async fn ban(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::ADMIN }>,
    req_id: RequestId,
    db: Res<Database>,
    Json(body): Json<BanBody>,
) -> RecordsResponse<impl Responder> {
    let conn = DatabaseConnection::from(db.0.mysql_pool);

    let player = records_lib::must::have_player(&conn, &body.player_login)
        .await
        .fit(req_id)?;

    let admin_id = records_lib::must::have_player(&conn, &login)
        .await
        .fit(req_id)?
        .id;

    let was_reprieved = banishments::Entity::find()
        .filter(banishments::Column::PlayerId.eq(player.id))
        .one(&conn)
        .await
        .with_api_err()
        .fit(req_id)?
        .is_some();

    let new_ban = banishments::ActiveModel {
        date_ban: Set(chrono::Utc::now().naive_utc()),
        duration: Set(body.duration),
        was_reprieved: Set(if was_reprieved { 1 } else { 0 }),
        reason: body.reason.map(Set).unwrap_or_default(),
        player_id: Set(Some(player.id)),
        banished_by: Set(Some(admin_id)),
        ..Default::default()
    };

    let ban_id = banishments::Entity::insert(new_ban)
        .exec_with_returning_keys(&conn)
        .await
        .with_api_err()
        .fit(req_id)?[0];

    let ban = banishments::Entity::find_by_id(ban_id)
        .join_as(
            sea_orm::JoinType::InnerJoin,
            banishments::Relation::Players2.def(),
            "ban_author_player",
        )
        .select_only()
        .column(banishments::Column::Id)
        .column(banishments::Column::DateBan)
        .column(banishments::Column::Duration)
        .column(banishments::Column::Reason)
        .column_as(
            Expr::col(("ban_author_player", players::Column::Login)),
            "banished_by",
        )
        .column(banishments::Column::WasReprieved)
        .into_model::<BanishmentInner>()
        .one(&conn)
        .await
        .with_api_err()
        .fit(req_id)?
        .expect("Ban is supposed to be in database");

    json(BanResponse {
        player_login: body.player_login,
        ban: Banishment {
            is_current: ban.is_current != 0,
            was_reprieved: ban.was_reprieved != 0,
            inner: ban,
        },
    })
}

pub async fn get_ban_of<C: ConnectionTrait>(
    conn: &C,
    player_id: u32,
) -> RecordsResult<Option<Banishment>> {
    let ban = current_bans::Entity::find()
        .filter(
            current_bans::Column::PlayerId.eq(player_id).and(
                Func::cust("TIMESTAMPADD")
                    .arg(Expr::custom_keyword("SECOND"))
                    .arg(Expr::col(current_bans::Column::Duration))
                    .arg(Expr::col(current_bans::Column::DateBan))
                    .gt(Func::cust("NOW"))
                    .or(current_bans::Column::Duration.is_null()),
            ),
        )
        .join_as(
            sea_orm::JoinType::InnerJoin,
            current_bans::Relation::Players2.def(),
            "ban_author_player",
        )
        .select_only()
        .column(current_bans::Column::Id)
        .column(current_bans::Column::DateBan)
        .column(current_bans::Column::Duration)
        .column(current_bans::Column::WasReprieved)
        .column(current_bans::Column::Reason)
        .column_as(
            Expr::col(("ban_author_player", players::Column::Login)),
            "banished_by",
        )
        .into_model::<BanishmentInner>()
        .one(conn)
        .await
        .with_api_err()?;

    Ok(ban.map(|ban| Banishment {
        was_reprieved: ban.was_reprieved != 0,
        is_current: ban.is_current != 0,
        inner: ban,
    }))
}

#[derive(Deserialize)]
pub struct UnbanBody {
    player_login: String,
}

#[derive(Serialize)]
struct UnbanResponse {
    player_login: String,
    ban: Banishment,
}

pub async fn unban(
    _: MPAuthGuard<{ privilege::ADMIN }>,
    req_id: RequestId,
    db: Res<Database>,
    Json(body): Json<UnbanBody>,
) -> RecordsResponse<impl Responder> {
    let conn = DatabaseConnection::from(db.0.mysql_pool);

    let player_id = records_lib::must::have_player(&conn, &body.player_login)
        .await
        .fit(req_id)?
        .id;

    let Some(ban) = get_ban_of(&conn, player_id).await.fit(req_id)? else {
        return Err(RecordsErrorKind::PlayerNotBanned(body.player_login)).fit(req_id);
    };

    let mut query = Query::update();
    let query = query
        .table(banishments::Entity)
        .value(
            banishments::Column::Duration,
            Func::cust("SYSDATE").sub(Expr::col(banishments::Column::DateBan)),
        )
        .and_where(banishments::Column::Id.eq(ban.inner.id));

    let query = StatementBuilder::build(&*query, &conn.get_database_backend());

    conn.execute(query).await.with_api_err().fit(req_id)?;

    json(UnbanResponse {
        player_login: body.player_login,
        ban,
    })
}

#[derive(Deserialize)]
pub struct PlayerNoteBody {
    player_login: String,
}

#[derive(Serialize)]
struct PlayerNoteResponse {
    player_login: String,
    admins_note: Option<String>,
}

pub async fn player_note(
    _: MPAuthGuard<{ privilege::ADMIN }>,
    req_id: RequestId,
    db: Res<Database>,
    Json(body): Json<PlayerNoteBody>,
) -> RecordsResponse<impl Responder> {
    let conn = DatabaseConnection::from(db.0.mysql_pool);

    let admins_note = records_lib::must::have_player(&conn, &body.player_login)
        .await
        .fit(req_id)?
        .admins_note;

    json(PlayerNoteResponse {
        player_login: body.player_login,
        admins_note,
    })
}
