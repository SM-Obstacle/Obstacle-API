use crate::{
    auth::{self, privilege, AuthHeader, MPAuthGuard},
    models::{self, Map, Player},
    must,
    utils::{any_repeated, json},
    Database, RecordsError, RecordsResult,
};
use actix_web::{
    web::{self, Data, Json, Query},
    HttpResponse, Responder, Scope,
};
use futures::{future::try_join_all, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::player::{self, UpdatePlayerBody};

pub fn map_scope() -> Scope {
    web::scope("/map")
        .route("/insert", web::post().to(insert))
        .route("/player_rating", web::get().to(player_rating))
        .route("/ratings", web::get().to(ratings))
        .route("/rating", web::get().to(rating))
        .route("/rate", web::post().to(rate))
        .route("/reset_ratings", web::post().to(reset_ratings))
}

#[derive(Deserialize)]
struct UpdateMapBody {
    name: String,
    map_uid: String,
    cps_number: u32,
    author: UpdatePlayerBody,
    // Keep it optional for backward compatibility
    reversed: Option<bool>,
}

async fn insert(
    db: Data<Database>,
    Json(body): Json<UpdateMapBody>,
) -> RecordsResult<impl Responder> {
    let res = player::get_map_from_game_id(&db, &body.map_uid).await?;

    if let Some(Map { id, cps_number, .. }) = res {
        if cps_number.is_none() {
            sqlx::query("UPDATE maps SET cps_number = ? WHERE id = ?")
                .bind(body.cps_number)
                .bind(id)
                .execute(&db.mysql_pool)
                .await?;
        }

        return Ok(HttpResponse::Ok().finish());
    }

    let player_id = player::get_or_insert(&db, &body.author.login.clone(), body.author).await?;

    sqlx::query(
        "INSERT INTO maps
        (game_id, player_id, name, cps_number, reversed)
        VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&body.map_uid)
    .bind(player_id)
    .bind(&body.name)
    .bind(body.cps_number)
    .bind(body.reversed)
    .execute(&db.mysql_pool)
    .await?;

    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct PlayerRatingBody {
    map_uid: String,
}

#[derive(Serialize, FromRow)]
struct Rating {
    kind: String,
    rating: f32,
}

#[derive(Serialize)]
struct PlayerRating {
    rating_date: chrono::NaiveDateTime,
    ratings: Vec<Rating>,
}

#[derive(Serialize)]
struct PlayerRatingResponse {
    player_login: String,
    map_name: String,
    author_login: String,
    rating: Option<PlayerRating>,
}

pub async fn player_rating(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::PLAYER }>,
    db: Data<Database>,
    Json(body): Json<PlayerRatingBody>,
) -> RecordsResult<impl Responder> {
    let player_id = must::have_player(&db, &login).await?.id;
    let map_id = must::have_map(&db, &body.map_uid).await?.id;

    let rating = match sqlx::query_scalar(
        "SELECT rating_date FROM rating WHERE player_id = ? AND map_id = ?",
    )
    .bind(player_id)
    .bind(map_id)
    .fetch_optional(&db.mysql_pool)
    .await?
    {
        Some(rating_date) => {
            let ratings = sqlx::query_as(
                "SELECT k.kind, rating
            FROM player_rating r
            INNER JOIN rating_kind k ON k.id = r.kind
            WHERE player_id = ? AND map_id = ?",
            )
            .bind(player_id)
            .bind(map_id)
            .fetch_all(&db.mysql_pool)
            .await?;

            Some(PlayerRating {
                rating_date,
                ratings,
            })
        }
        None => None,
    };

    let (map_name, author_login) = sqlx::query_as(
        "SELECT m.name, p.login
        FROM maps m
        INNER JOIN players p ON p.id = m.player_id
        WHERE m.id = ?",
    )
    .bind(map_id)
    .fetch_one(&db.mysql_pool)
    .await?;

    json(PlayerRatingResponse {
        player_login: login,
        map_name,
        author_login,
        rating,
    })
}

#[derive(Deserialize)]
pub struct RatingsBody {
    map_id: String,
}

#[derive(Serialize)]
struct PlayerGroupedRatings {
    player_login: String,
    rating_date: chrono::NaiveDateTime,
    ratings: Vec<Rating>,
}

#[derive(Serialize)]
struct RatingsResponse {
    map_name: String,
    author_login: String,
    players_ratings: Vec<PlayerGroupedRatings>,
}

pub async fn ratings(
    db: Data<Database>,
    AuthHeader { login, token }: AuthHeader,
    Json(body): Json<RatingsBody>,
) -> RecordsResult<impl Responder> {
    let player = must::have_player(&db, &login).await?;
    let map = must::have_map(&db, &body.map_id).await?;

    let (role, author_login) = if map.player_id == player.id {
        (privilege::PLAYER, login.clone())
    } else {
        let login = sqlx::query_scalar("SELECT login FROM players WHERE id = ?")
            .bind(map.player_id)
            .fetch_one(&db.mysql_pool)
            .await?;
        (privilege::ADMIN, login)
    };

    auth::check_auth_for(&db, &login, &token, role).await?;

    let players_ratings =
        sqlx::query_as::<_, models::Rating>("SELECT * FROM rating WHERE map_id = ?")
            .bind(map.id)
            .fetch(&db.mysql_pool)
            .map(|rating| async {
                let rating = rating?;

                let player_login = sqlx::query_scalar("SELECT login FROM players WHERE id = ?")
                    .bind(rating.player_id)
                    .fetch_one(&db.mysql_pool)
                    .await?;

                let ratings = sqlx::query_as::<_, Rating>(
                    "SELECT k.kind, rating
            FROM player_rating r
            INNER JOIN rating_kind k ON k.id = r.kind
            WHERE player_id = ? AND map_id = ?",
                )
                .bind(rating.player_id)
                .bind(rating.map_id)
                .fetch_all(&db.mysql_pool)
                .await?;

                RecordsResult::Ok(PlayerGroupedRatings {
                    player_login,
                    rating_date: rating.rating_date,
                    ratings,
                })
            })
            .collect::<Vec<_>>()
            .await;

    let players_ratings = try_join_all(players_ratings).await?;

    json(RatingsResponse {
        map_name: map.name,
        author_login,
        players_ratings,
    })
}

#[derive(Deserialize)]
pub struct RatingBody {
    map_uid: String,
}

#[derive(Serialize)]
struct RatingResponse {
    map_name: String,
    author_login: String,
    ratings: Vec<Rating>,
}

pub async fn rating(
    db: Data<Database>,
    Query(body): Query<RatingBody>,
) -> RecordsResult<impl Responder> {
    let Some((map_name, author_login)) = sqlx::query_as(
        "SELECT m.name, login FROM maps m
        INNER JOIN players p ON p.id = player_id
        WHERE game_id = ?")
    .bind(&body.map_uid)
    .fetch_optional(&db.mysql_pool).await? else {
        return Err(RecordsError::MapNotFound(body.map_uid));
    };

    let ratings = sqlx::query_as(
        "SELECT k.kind, AVG(rating) as rating
        FROM player_rating r
        INNER JOIN rating_kind k ON k.id = r.kind
        INNER JOIN maps m ON m.id = r.map_id
        WHERE game_id = ?
        GROUP BY k.id ORDER BY k.id",
    )
    .bind(&body.map_uid)
    .fetch_all(&db.mysql_pool)
    .await?;

    json(RatingResponse {
        map_name,
        author_login,
        ratings,
    })
}

#[derive(Deserialize)]
struct PlayerRate {
    kind: u8,
    rating: f32,
}

impl PartialEq for PlayerRate {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

#[derive(Deserialize)]
pub struct RateBody {
    map_id: String,
    ratings: Vec<PlayerRate>,
}

#[derive(Serialize)]
struct RateResponse {
    player_login: String,
    map_name: String,
    author_login: String,
    rating_date: chrono::NaiveDateTime,
    ratings: Vec<Rating>,
}

pub async fn rate(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::PLAYER }>,
    db: Data<Database>,
    Json(body): Json<RateBody>,
) -> RecordsResult<impl Responder> {
    let Player {
        id: player_id,
        login: player_login,
        ..
    } = must::have_player(&db, &login).await?;

    let Map {
        id: map_id,
        name: map_name,
        player_id: author_id,
        ..
    } = must::have_map(&db, &body.map_id).await?;

    let author_login = sqlx::query_scalar("SELECT login FROM players WHERE id = ?")
        .bind(author_id)
        .fetch_one(&db.mysql_pool)
        .await?;

    let rate_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rating_kind")
        .fetch_one(&db.mysql_pool)
        .await?;
    if body.ratings.len()
        > <i64 as TryInto<usize>>::try_into(rate_count).expect("couldn't convert from i64 to usize")
        || any_repeated(&body.ratings)
    {
        return Err(RecordsError::InvalidRates);
    }

    if body.ratings.is_empty() {
        let Some(rating_date) = sqlx::query_scalar(
            "SELECT rating_date FROM rating WHERE player_id = ? AND map_id = ?")
        .bind(player_id)
        .bind(map_id)
        .fetch_optional(&db.mysql_pool)
        .await? else {
            return Err(RecordsError::NoRatingFound(login, body.map_id));
        };

        return json(RateResponse {
            player_login,
            map_name,
            author_login,
            rating_date,
            ratings: Vec::new(),
        });
    }

    let rating_date = {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM rating WHERE map_id = ? AND player_id = ?")
                .bind(map_id)
                .bind(player_id)
                .fetch_one(&db.mysql_pool)
                .await?;

        if count != 0 {
            sqlx::query(
                "UPDATE rating SET rating_date = SYSDATE() WHERE map_id = ? AND player_id = ?",
            )
            .bind(map_id)
            .bind(player_id)
            .execute(&db.mysql_pool)
            .await?;

            sqlx::query_scalar("SELECT rating_date FROM rating WHERE map_id = ? AND player_id = ?")
                .bind(map_id)
                .bind(player_id)
                .fetch_one(&db.mysql_pool)
                .await?
        } else {
            sqlx::query_scalar(
                "INSERT INTO rating (player_id, map_id, rating_date)
                VALUES (?, ?, SYSDATE()) RETURNING rating_date",
            )
            .bind(player_id)
            .bind(map_id)
            .fetch_one(&db.mysql_pool)
            .await?
        }
    };

    let mut ratings = Vec::with_capacity(body.ratings.len());

    for rate in body.ratings {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM player_rating
                    WHERE map_id = ? AND player_id = ? AND kind = ?",
        )
        .bind(map_id)
        .bind(player_id)
        .bind(rate.kind)
        .fetch_one(&db.mysql_pool)
        .await?;

        if count != 0 {
            sqlx::query(
                "UPDATE player_rating SET rating = ?
                        WHERE map_id = ? AND player_id = ? AND kind = ?",
            )
            .bind(rate.rating)
            .bind(map_id)
            .bind(player_id)
            .bind(rate.kind)
            .execute(&db.mysql_pool)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO player_rating (player_id, map_id, kind, rating)
                        VALUES (?, ?, ?, ?)",
            )
            .bind(player_id)
            .bind(map_id)
            .bind(rate.kind)
            .bind(rate.rating)
            .execute(&db.mysql_pool)
            .await?;
        }

        let rating = sqlx::query_as(
            "SELECT k.kind, rating
                    FROM player_rating r
                    INNER JOIN rating_kind k ON k.id = r.kind
                    WHERE map_id = ? AND player_id = ? AND r.kind = ?",
        )
        .bind(map_id)
        .bind(player_id)
        .bind(rate.kind)
        .fetch_one(&db.mysql_pool)
        .await?;

        ratings.push(rating);
    }

    json(RateResponse {
        player_login,
        map_name,
        author_login,
        rating_date,
        ratings,
    })
}

#[derive(Deserialize)]
pub struct ResetRatingsBody {
    map_id: String,
}

#[derive(Serialize)]
struct ResetRatingsResponse {
    admin_login: String,
    map_name: String,
    author_login: String,
}

pub async fn reset_ratings(
    MPAuthGuard { login }: MPAuthGuard<{ privilege::ADMIN }>,
    db: Data<Database>,
    Json(body): Json<ResetRatingsBody>,
) -> RecordsResult<impl Responder> {
    let Some((map_id, map_name, author_login)) = sqlx::query_as(
        "SELECT m.id, m.name, login
        FROM maps m
        INNER JOIN players p ON p.id = player_id
        WHERE game_id = ?"
    )
    .bind(&body.map_id)
    .fetch_optional(&db.mysql_pool).await? else {
        return Err(RecordsError::MapNotFound(body.map_id));
    };
    let map_id: u32 = map_id;

    sqlx::query("DELETE FROM player_rating WHERE map_id = ?")
        .bind(map_id)
        .execute(&db.mysql_pool)
        .await?;

    sqlx::query("DELETE FROM rating WHERE map_id = ?")
        .bind(map_id)
        .execute(&db.mysql_pool)
        .await?;

    json(ResetRatingsResponse {
        admin_login: login,
        map_name,
        author_login,
    })
}
