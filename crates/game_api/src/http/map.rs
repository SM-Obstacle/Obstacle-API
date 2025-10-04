use crate::{
    RecordsErrorKind, RecordsResult, RecordsResultExt, Res,
    auth::{self, ApiAvailable, AuthHeader, MPAuthGuard, privilege},
    internal,
    utils::{ExtractDbConn, any_repeated, json},
};
use actix_web::{
    HttpResponse, Responder, Scope,
    web::{self, Json},
};
use entity::{maps, player_rating, players, rating, rating_kind};
use futures::{StreamExt, future::try_join_all};
use records_lib::Database;
use sea_orm::{
    ActiveModelTrait as _, ActiveValue::Set, ColumnTrait as _, EntityTrait as _, FromQueryResult,
    PaginatorTrait, QueryFilter, QuerySelect, prelude::Expr, sea_query::Func,
};
use serde::{Deserialize, Serialize};

use super::player::{self, PlayerInfoNetBody};

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
#[cfg_attr(test, derive(Serialize))]
struct MedalTimes {
    bronze_time: i32,
    silver_time: i32,
    gold_time: i32,
    author_time: i32,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct UpdateMapBody {
    name: String,
    map_uid: String,
    cps_number: u32,
    author: PlayerInfoNetBody,
    medal_times: Option<MedalTimes>,
}

async fn insert(
    _: ApiAvailable,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<UpdateMapBody>,
) -> RecordsResult<impl Responder> {
    let map = records_lib::map::get_map_from_uid(&conn, &body.map_uid).await?;

    if let Some(map) = map {
        let is_map_medals_empty = map.bronze_time.is_none()
            && map.silver_time.is_none()
            && map.gold_time.is_none()
            && map.author_time.is_none();

        let is_map_cps_number_empty = map.cps_number.is_none();

        let mut updated_map = maps::ActiveModel::from(map);

        if is_map_cps_number_empty {
            updated_map.cps_number = Set(Some(body.cps_number));
        }

        if is_map_medals_empty
            && let Some(medal_times) = body.medal_times
            && medal_times.author_time > 0
            && medal_times.bronze_time > medal_times.silver_time
            && medal_times.silver_time > medal_times.gold_time
            && medal_times.gold_time > medal_times.author_time
        {
            updated_map.bronze_time = Set(Some(medal_times.bronze_time));
            updated_map.silver_time = Set(Some(medal_times.silver_time));
            updated_map.gold_time = Set(Some(medal_times.gold_time));
            updated_map.author_time = Set(Some(medal_times.author_time));
        }

        if updated_map.is_changed() {
            maps::Entity::update(updated_map)
                .exec(&conn)
                .await
                .with_api_err()?;
        }
    } else {
        let player = player::get_or_insert(&conn, &body.author).await?;

        let mut new_map = maps::ActiveModel {
            game_id: Set(body.map_uid),
            player_id: Set(player.id),
            name: Set(body.name),
            cps_number: Set(Some(body.cps_number)),
            ..Default::default()
        };

        if let Some(medal_times) = body.medal_times {
            new_map.bronze_time = Set(Some(medal_times.bronze_time));
            new_map.silver_time = Set(Some(medal_times.silver_time));
            new_map.gold_time = Set(Some(medal_times.gold_time));
            new_map.author_time = Set(Some(medal_times.author_time));
        }

        maps::Entity::insert(new_map)
            .exec(&conn)
            .await
            .with_api_err()?;
    }

    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct PlayerRatingBody {
    map_uid: String,
}

#[derive(Serialize, FromQueryResult)]
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
    MPAuthGuard { login }: MPAuthGuard,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<PlayerRatingBody>,
) -> RecordsResult<impl Responder> {
    let player_id = records_lib::must::have_player(&conn, &login).await?.id;
    let map_id = records_lib::must::have_map(&conn, &body.map_uid).await?.id;

    let rating = match rating::Entity::find()
        .filter(
            rating::Column::PlayerId
                .eq(player_id)
                .and(rating::Column::MapId.eq(map_id)),
        )
        .select_only()
        .column(rating::Column::RatingDate)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()?
    {
        Some(rating_date) => {
            let ratings = player_rating::Entity::find()
                .filter(
                    player_rating::Column::PlayerId
                        .eq(player_id)
                        .and(player_rating::Column::MapId.eq(map_id)),
                )
                .inner_join(rating_kind::Entity)
                .select_only()
                .column(rating_kind::Column::Kind)
                .column(player_rating::Column::Rating)
                .into_model()
                .all(&conn)
                .await
                .with_api_err()?;

            Some(PlayerRating {
                rating_date,
                ratings,
            })
        }
        None => None,
    };

    let (map_name, author_login) = maps::Entity::find_by_id(map_id)
        .inner_join(players::Entity)
        .select_only()
        .column(maps::Column::Name)
        .column(players::Column::Login)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()?
        .ok_or_else(|| internal!("Map {map_id} should exist in database"))?;

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
    db: Res<Database>,
    AuthHeader { login, token }: AuthHeader,
    Json(body): Json<RatingsBody>,
) -> RecordsResult<impl Responder> {
    let player = records_lib::must::have_player(&db.sql_conn, &login).await?;
    let map = records_lib::must::have_map(&db.sql_conn, &body.map_id).await?;

    let (role, author_login) = if map.player_id == player.id {
        (privilege::PLAYER, login.clone())
    } else {
        let login = players::Entity::find_by_id(map.player_id)
            .select_only()
            .column(players::Column::Login)
            .into_tuple()
            .one(&db.sql_conn)
            .await
            .with_api_err()?
            .ok_or_else(|| internal!("Player {} should be in database", map.player_id))?;
        (privilege::ADMIN, login)
    };

    let mut redis_conn = db.redis_pool.get().await.with_api_err()?;

    auth::check_auth_for(
        &db.sql_conn,
        &mut redis_conn,
        &login,
        Some(token.as_str()),
        role,
    )
    .await?;

    let players_ratings = rating::Entity::find()
        .filter(rating::Column::MapId.eq(map.id))
        .stream(&db.sql_conn)
        .await
        .with_api_err()?
        .map(|rating| async {
            let rating = rating.with_api_err()?;

            let player_login = players::Entity::find_by_id(rating.player_id)
                .select_only()
                .column(players::Column::Login)
                .into_tuple()
                .one(&db.sql_conn)
                .await
                .with_api_err()?
                .ok_or_else(|| internal!("Player {} should exist in database", rating.player_id))?;

            let ratings = player_rating::Entity::find()
                .inner_join(rating_kind::Entity)
                .filter(
                    player_rating::Column::PlayerId
                        .eq(rating.player_id)
                        .and(player_rating::Column::MapId.eq(rating.map_id)),
                )
                .select_only()
                .column(rating_kind::Column::Kind)
                .column(player_rating::Column::Rating)
                .into_model::<Rating>()
                .all(&db.sql_conn)
                .await
                .with_api_err()?;

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
    ExtractDbConn(conn): ExtractDbConn,
    web::Query(body): web::Query<RatingBody>,
) -> RecordsResult<impl Responder> {
    let info = maps::Entity::find()
        .inner_join(players::Entity)
        .filter(maps::Column::GameId.eq(&body.map_uid))
        .select_only()
        .column(maps::Column::Name)
        .column(players::Column::Login)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()?;

    let Some((map_name, author_login)) = info else {
        return Err(RecordsErrorKind::from(
            records_lib::error::RecordsError::MapNotFound(body.map_uid),
        ));
    };

    let ratings = player_rating::Entity::find()
        .inner_join(rating_kind::Entity)
        .inner_join(maps::Entity)
        .filter(maps::Column::GameId.eq(&body.map_uid))
        .select_only()
        .column(rating_kind::Column::Kind)
        .column_as(
            Expr::expr(Func::avg(Expr::col(player_rating::Column::Rating))),
            "rating",
        )
        .into_model()
        .all(&conn)
        .await
        .with_api_err()?;

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
    MPAuthGuard { login }: MPAuthGuard,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<RateBody>,
) -> RecordsResult<impl Responder> {
    let players::Model {
        id: player_id,
        login: player_login,
        ..
    } = records_lib::must::have_player(&conn, &login).await?;

    let maps::Model {
        id: map_id,
        name: map_name,
        player_id: author_id,
        ..
    } = records_lib::must::have_map(&conn, &body.map_id).await?;

    let author_login = players::Entity::find_by_id(author_id)
        .select_only()
        .column(players::Column::Login)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()?
        .ok_or_else(|| internal!("Player {author_id} should be in database"))?;

    let rate_count = rating_kind::Entity::find()
        .count(&conn)
        .await
        .with_api_err()?;

    if body.ratings.len() as u64 > rate_count || any_repeated(&body.ratings) {
        return Err(RecordsErrorKind::InvalidRates);
    }

    if body.ratings.is_empty() {
        let rating_date = rating::Entity::find()
            .filter(
                rating::Column::PlayerId
                    .eq(player_id)
                    .and(rating::Column::MapId.eq(map_id)),
            )
            .select_only()
            .column(rating::Column::RatingDate)
            .into_tuple()
            .one(&conn)
            .await
            .with_api_err()?;

        let Some(rating_date) = rating_date else {
            return Err(RecordsErrorKind::NoRatingFound(login, body.map_id));
        };

        json(RateResponse {
            player_login,
            map_name,
            author_login,
            rating_date,
            ratings: Vec::new(),
        })
    } else {
        // TODO: we can use the REPLACE INTO syntax instead of checking the count everytime

        let rating_date = {
            let count = rating::Entity::find()
                .filter(
                    rating::Column::MapId
                        .eq(map_id)
                        .and(rating::Column::PlayerId.eq(player_id)),
                )
                .count(&conn)
                .await
                .with_api_err()?;

            if count > 0 {
                let date = chrono::Utc::now().naive_utc();

                rating::Entity::update(rating::ActiveModel {
                    rating_date: Set(date),
                    ..Default::default()
                })
                .filter(
                    rating::Column::MapId
                        .eq(map_id)
                        .and(rating::Column::PlayerId.eq(player_id)),
                )
                .exec(&conn)
                .await
                .with_api_err()?;

                date
            } else {
                let new_rating = rating::ActiveModel {
                    player_id: Set(player_id),
                    map_id: Set(map_id),
                    rating_date: Set(chrono::Utc::now().naive_utc()),
                };

                let rating = rating::Entity::insert(new_rating)
                    .exec_with_returning(&conn)
                    .await
                    .with_api_err()?;

                rating.rating_date
            }
        };

        let mut ratings = Vec::with_capacity(body.ratings.len());

        for rate in body.ratings {
            let count = player_rating::Entity::find()
                .filter(
                    player_rating::Column::MapId
                        .eq(map_id)
                        .and(player_rating::Column::PlayerId.eq(player_id))
                        .and(player_rating::Column::Kind.eq(rate.kind)),
                )
                .count(&conn)
                .await
                .with_api_err()?;

            if count > 0 {
                player_rating::Entity::update(player_rating::ActiveModel {
                    rating: Set(rate.rating),
                    ..Default::default()
                })
                .filter(
                    player_rating::Column::MapId
                        .eq(map_id)
                        .and(player_rating::Column::PlayerId.eq(player_id))
                        .and(player_rating::Column::Kind.eq(rate.kind)),
                )
                .exec(&conn)
                .await
                .with_api_err()?;
            } else {
                let new_player_rating = player_rating::ActiveModel {
                    player_id: Set(player_id),
                    map_id: Set(map_id),
                    kind: Set(rate.kind),
                    rating: Set(rate.rating),
                };

                player_rating::Entity::insert(new_player_rating)
                    .exec(&conn)
                    .await
                    .with_api_err()?;
            }

            let rating = player_rating::Entity::find()
                .inner_join(rating_kind::Entity)
                .filter(player_rating::Column::MapId.eq(map_id).and(player_rating::Column::PlayerId.eq(player_id)).and(player_rating::Column::Kind.eq(rate.kind)))
                .select_only()
                .column(rating_kind::Column::Kind)
                .column(player_rating::Column::Rating)
                .into_model()
                .one(&conn)
                .await
                .with_api_err()?
                .ok_or_else(|| internal!("Player rating of {player_id} on {map_id} and rating kind {} should exist in database", rate.kind))?;

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
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<ResetRatingsBody>,
) -> RecordsResult<impl Responder> {
    let info = maps::Entity::find()
        .filter(maps::Column::GameId.eq(&body.map_id))
        .inner_join(players::Entity)
        .select_only()
        .columns([maps::Column::Id, maps::Column::Name])
        .column(players::Column::Login)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()?;

    let Some((map_id, map_name, author_login)) = info else {
        return Err(RecordsErrorKind::from(
            records_lib::error::RecordsError::MapNotFound(body.map_id),
        ));
    };

    let map_id: u32 = map_id;

    player_rating::Entity::delete_many()
        .filter(player_rating::Column::MapId.eq(map_id))
        .exec(&conn)
        .await
        .with_api_err()?;

    rating::Entity::delete_many()
        .filter(rating::Column::MapId.eq(map_id))
        .exec(&conn)
        .await
        .with_api_err()?;

    json(ResetRatingsResponse {
        admin_login: login,
        map_name,
        author_login,
    })
}
