use crate::{
    auth::{self, AuthHeader},
    models::{self, Map, Player, Role},
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

#[derive(Deserialize, Serialize, FromRow)]
pub struct MapAuthor {
    pub login: String,
    pub name: String,
    pub country: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateMapBody {
    pub name: String,
    pub map_uid: String,
    pub cps_number: u32,
    pub author: MapAuthor,
}

pub async fn insert(
    db: Data<Database>,
    body: Json<UpdateMapBody>,
) -> RecordsResult<impl Responder> {
    let _ = get_or_insert(&db, &body).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn get_or_insert(db: &Database, body: &UpdateMapBody) -> RecordsResult<u32> {
    let res = player::get_map_from_game_id(db, &body.map_uid).await?;

    if let Some(Map { id, cps_number, .. }) = res {
        if cps_number.is_none() {
            sqlx::query!(
                "UPDATE maps SET cps_number = ? WHERE id = ?",
                body.cps_number,
                id
            )
            .execute(&db.mysql_pool)
            .await?;
        }

        return Ok(id);
    }

    let player_id = player::get_or_insert(
        db,
        &body.author.login,
        UpdatePlayerBody {
            nickname: body.author.name.clone(),
            country: body.author.country.clone(),
            login: "".to_string(),
        },
    )
    .await?;

    let id = sqlx::query_scalar(
        "INSERT INTO maps
        (game_id, player_id, name, cps_number)
        VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(&body.map_uid)
    .bind(player_id)
    .bind(&body.name)
    .bind(body.cps_number)
    .fetch_one(&db.mysql_pool)
    .await?;

    Ok(id)
}

#[derive(Deserialize)]
pub struct PlayerRatingBody {
    login: String,
    map_id: String,
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
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<PlayerRatingBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Player).await?;
    let body = body.into_inner();

    let Some(Player { id: player_id, .. }) = player::get_player_from_login(&db, &body.login).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };
    let Some(Map { id: map_id, .. }) = player::get_map_from_game_id(&db, &body.map_id).await? else {
        return Err(RecordsError::MapNotFound(body.map_id));
    };

    let rating = match sqlx::query_scalar!(
        "SELECT rating_date FROM rating WHERE player_id = ? AND map_id = ?",
        player_id,
        map_id
    )
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
        player_login: body.login,
        map_name,
        author_login,
        rating,
    })
}

#[derive(Deserialize)]
pub struct RatingsBody {
    login: String,
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
    auth: AuthHeader,
    body: Json<RatingsBody>,
) -> RecordsResult<impl Responder> {
    let body = body.into_inner();

    let Some(player) = player::get_player_from_login(&db, &body.login).await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };
    let Some(map) = player::get_map_from_game_id(&db, &body.map_id).await? else {
        return Err(RecordsError::MapNotFound(body.map_id));
    };

    let (role, author_login) = if map.player_id == player.id {
        (Role::Player, body.login.clone())
    } else {
        let login = sqlx::query_scalar!("SELECT login FROM players WHERE id = ?", map.player_id)
            .fetch_one(&db.mysql_pool)
            .await?;
        (Role::Admin, login)
    };

    auth::check_auth_for(&db, auth, role).await?;

    let players_ratings = sqlx::query_as!(
        models::Rating,
        "SELECT * FROM rating WHERE map_id = ?",
        map.id
    )
    .fetch(&db.mysql_pool)
    .map(|rating| async {
        let rating = rating?;

        let player_login =
            sqlx::query_scalar!("SELECT login FROM players WHERE id = ?", rating.player_id)
                .fetch_one(&db.mysql_pool)
                .await?;

        let ratings = sqlx::query_as!(
            Rating,
            "SELECT k.kind, rating
            FROM player_rating r
            INNER JOIN rating_kind k ON k.id = r.kind
            WHERE player_id = ? AND map_id = ?",
            rating.player_id,
            rating.map_id,
        )
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

pub async fn rating(db: Data<Database>, body: Query<RatingBody>) -> RecordsResult<impl Responder> {
    let body = body.into_inner();

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
    login: String,
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
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<RateBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Player).await?;
    let body = body.into_inner();

    let Some(Player { id: player_id, login: player_login, .. }) = sqlx::query_as(
        "SELECT * FROM players WHERE login = ?")
    .bind(&body.login)
    .fetch_optional(&db.mysql_pool)
    .await? else {
        return Err(RecordsError::PlayerNotFound(body.login));
    };
    let Some(Map { id: map_id, name: map_name, player_id: author_id, .. }) = sqlx::query_as(
        "SELECT * FROM maps WHERE game_id = ?")
    .bind(&body.map_id)
    .fetch_optional(&db.mysql_pool)
    .await? else {
        return Err(RecordsError::MapNotFound(body.map_id));
    };

    let author_login = sqlx::query_scalar("SELECT login FROM players WHERE id = ?")
        .bind(author_id)
        .fetch_one(&db.mysql_pool)
        .await?;

    let rate_count = sqlx::query_scalar!("SELECT COUNT(*) FROM rating_kind")
        .fetch_one(&db.mysql_pool)
        .await?;
    if body.ratings.len()
        > rate_count
            .try_into()
            .expect("couldn't convert from i64 to usize")
        || any_repeated(&body.ratings)
    {
        return Err(RecordsError::InvalidRates);
    }

    if body.ratings.is_empty() {
        let Some(rating_date) = sqlx::query_scalar!(
            "SELECT rating_date FROM rating WHERE player_id = ? AND map_id = ?", player_id, map_id)
        .fetch_optional(&db.mysql_pool)
        .await? else {
            return Err(RecordsError::NoRatingFound(body.login, body.map_id));
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
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM rating WHERE map_id = ? AND player_id = ?",
            map_id,
            player_id
        )
        .fetch_one(&db.mysql_pool)
        .await?;

        if count != 0 {
            sqlx::query!(
                "UPDATE rating SET rating_date = SYSDATE() WHERE map_id = ? AND player_id = ?",
                map_id,
                player_id
            )
            .execute(&db.mysql_pool)
            .await?;

            sqlx::query_scalar!(
                "SELECT rating_date FROM rating WHERE map_id = ? AND player_id = ?",
                map_id,
                player_id
            )
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
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM player_rating
                    WHERE map_id = ? AND player_id = ? AND kind = ?",
            map_id,
            player_id,
            rate.kind
        )
        .fetch_one(&db.mysql_pool)
        .await?;

        if count != 0 {
            sqlx::query!(
                "UPDATE player_rating SET rating = ?
                        WHERE map_id = ? AND player_id = ? AND kind = ?",
                rate.rating,
                map_id,
                player_id,
                rate.kind
            )
            .execute(&db.mysql_pool)
            .await?;
        } else {
            sqlx::query!(
                "INSERT INTO player_rating (player_id, map_id, kind, rating)
                        VALUES (?, ?, ?, ?)",
                player_id,
                map_id,
                rate.kind,
                rate.rating
            )
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
    admin_login: String,
    map_id: String,
}

#[derive(Serialize)]
struct ResetRatingsResponse {
    admin_login: String,
    map_name: String,
    author_login: String,
}

pub async fn reset_ratings(
    db: Data<Database>,
    auth: AuthHeader,
    body: Json<ResetRatingsBody>,
) -> RecordsResult<impl Responder> {
    auth::check_auth_for(&db, auth, Role::Admin).await?;
    let body = body.into_inner();

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

    sqlx::query!("DELETE FROM player_rating WHERE map_id = ?", map_id)
        .execute(&db.mysql_pool)
        .await?;

    sqlx::query!("DELETE FROM rating WHERE map_id = ?", map_id)
        .execute(&db.mysql_pool)
        .await?;

    json(ResetRatingsResponse {
        admin_login: body.admin_login,
        map_name,
        author_login,
    })
}
