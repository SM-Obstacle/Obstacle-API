use crate::{
    FitRequestId, RecordsErrorKind, RecordsResponse, RecordsResult, RecordsResultExt, Res,
    auth::{self, ApiAvailable, AuthHeader, MPAuthGuard, privilege},
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
    ActiveValue::Set,
    ColumnTrait as _, ConnectionTrait, EntityTrait as _, FromQueryResult, PaginatorTrait,
    QueryFilter, QuerySelect, StatementBuilder,
    prelude::Expr,
    sea_query::{Func, Query},
};
use serde::{Deserialize, Serialize};
use tracing_actix_web::RequestId;

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
struct UpdateMapBody {
    name: String,
    map_uid: String,
    cps_number: u32,
    author: PlayerInfoNetBody,
}

async fn insert(
    _: ApiAvailable,
    req_id: RequestId,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<UpdateMapBody>,
) -> RecordsResponse<impl Responder> {
    let res = records_lib::map::get_map_from_uid(&conn, &body.map_uid)
        .await
        .fit(req_id)?;

    if let Some(maps::Model { id, cps_number, .. }) = res {
        if cps_number.is_none() {
            let mut update = Query::update();
            let update = update
                .table(maps::Entity)
                .value(maps::Column::CpsNumber, body.cps_number)
                .and_where(maps::Column::Id.eq(id));
            let stmt = StatementBuilder::build(&*update, &conn.get_database_backend());
            conn.execute(stmt).await.with_api_err().fit(req_id)?;
        }
    } else {
        let player = player::get_or_insert(&conn, &body.author)
            .await
            .fit(req_id)?;

        let new_map = maps::ActiveModel {
            game_id: Set(body.map_uid),
            player_id: Set(player.id),
            name: Set(body.name),
            cps_number: Set(Some(body.cps_number)),
            ..Default::default()
        };

        maps::Entity::insert(new_map)
            .exec(&conn)
            .await
            .with_api_err()
            .fit(req_id)?;
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
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<PlayerRatingBody>,
) -> RecordsResponse<impl Responder> {
    let player_id = records_lib::must::have_player(&conn, &login)
        .await
        .fit(req_id)?
        .id;
    let map_id = records_lib::must::have_map(&conn, &body.map_uid)
        .await
        .fit(req_id)?
        .id;

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
        .with_api_err()
        .fit(req_id)?
    {
        Some(rating_date) => {
            let ratings = player_rating::Entity::find()
                .filter(
                    player_rating::Column::PlayerId
                        .eq(player_id)
                        .and(player_rating::Column::MapId.eq(map_id)),
                )
                .select_only()
                .column(rating_kind::Column::Kind)
                .column(player_rating::Column::Rating)
                .into_model()
                .all(&conn)
                .await
                .with_api_err()
                .fit(req_id)?;

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
        .with_api_err()
        .fit(req_id)?
        .unwrap_or_else(|| panic!("Map {map_id} should exist in database"));

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
    req_id: RequestId,
    db: Res<Database>,
    AuthHeader { login, token }: AuthHeader,
    Json(body): Json<RatingsBody>,
) -> RecordsResponse<impl Responder> {
    let player = records_lib::must::have_player(&db.sql_conn, &login)
        .await
        .fit(req_id)?;
    let map = records_lib::must::have_map(&db.sql_conn, &body.map_id)
        .await
        .fit(req_id)?;

    let (role, author_login) = if map.player_id == player.id {
        (privilege::PLAYER, login.clone())
    } else {
        let login = players::Entity::find_by_id(map.player_id)
            .select_only()
            .column(players::Column::Login)
            .into_tuple()
            .one(&db.sql_conn)
            .await
            .with_api_err()
            .fit(req_id)?
            .unwrap_or_else(|| panic!("Player {} should be in database", map.player_id));
        (privilege::ADMIN, login)
    };

    let mut redis_conn = db.redis_pool.get().await.with_api_err().fit(req_id)?;

    auth::check_auth_for(
        &db.sql_conn,
        &mut redis_conn,
        &login,
        Some(token.as_str()),
        role,
    )
    .await
    .fit(req_id)?;

    let players_ratings = rating::Entity::find()
        .filter(rating::Column::MapId.eq(map.id))
        .stream(&db.sql_conn)
        .await
        .with_api_err()
        .fit(req_id)?
        .map(|rating| async {
            let rating = rating.with_api_err()?;

            let player_login = players::Entity::find_by_id(rating.player_id)
                .select_only()
                .column(players::Column::Login)
                .into_tuple()
                .one(&db.sql_conn)
                .await
                .with_api_err()?
                .unwrap_or_else(|| panic!("Player {} should exist in database", rating.player_id));

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

    let players_ratings = try_join_all(players_ratings).await.fit(req_id)?;

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
    req_id: RequestId,
    ExtractDbConn(conn): ExtractDbConn,
    web::Query(body): web::Query<RatingBody>,
) -> RecordsResponse<impl Responder> {
    let info = maps::Entity::find()
        .inner_join(players::Entity)
        .filter(maps::Column::GameId.eq(&body.map_uid))
        .select_only()
        .column(maps::Column::Name)
        .column(players::Column::Login)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    let Some((map_name, author_login)) = info else {
        return Err(RecordsErrorKind::from(
            records_lib::error::RecordsError::MapNotFound(body.map_uid),
        ))
        .fit(req_id);
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
        .with_api_err()
        .fit(req_id)?;

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
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<RateBody>,
) -> RecordsResponse<impl Responder> {
    let players::Model {
        id: player_id,
        login: player_login,
        ..
    } = records_lib::must::have_player(&conn, &login)
        .await
        .fit(req_id)?;

    let maps::Model {
        id: map_id,
        name: map_name,
        player_id: author_id,
        ..
    } = records_lib::must::have_map(&conn, &body.map_id)
        .await
        .fit(req_id)?;

    let author_login = players::Entity::find_by_id(author_id)
        .select_only()
        .column(players::Column::Login)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()
        .fit(req_id)?
        .unwrap_or_else(|| panic!("Player {author_id} should be in database"));

    let rate_count = rating_kind::Entity::find()
        .count(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    if body.ratings.len() as u64 > rate_count || any_repeated(&body.ratings) {
        return Err(RecordsErrorKind::InvalidRates).fit(req_id);
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
            .with_api_err()
            .fit(req_id)?;

        let Some(rating_date) = rating_date else {
            return Err(RecordsErrorKind::NoRatingFound(login, body.map_id)).fit(req_id);
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
                .with_api_err()
                .fit(req_id)?;

            if count > 0 {
                let mut update = Query::update();
                let update = update
                    .table(rating::Entity)
                    .value(rating::Column::RatingDate, Func::cust("SYSDATE"))
                    .and_where(
                        rating::Column::MapId
                            .eq(map_id)
                            .and(rating::Column::PlayerId.eq(player_id)),
                    );
                let stmt = StatementBuilder::build(&*update, &conn.get_database_backend());
                conn.execute(stmt).await.with_api_err().fit(req_id)?;

                rating::Entity::find()
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
                    .with_api_err()
                    .fit(req_id)?
                    .unwrap_or_else(|| {
                        panic!("Rating of player {player_id} on {map_id} should exist in database")
                    })
            } else {
                let new_rating = rating::ActiveModel {
                    player_id: Set(player_id),
                    map_id: Set(map_id),
                    rating_date: Set(chrono::Utc::now().naive_utc()),
                };

                let rating = rating::Entity::insert(new_rating)
                    .exec_with_returning(&conn)
                    .await
                    .with_api_err()
                    .fit(req_id)?;

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
                .with_api_err()
                .fit(req_id)?;

            if count > 0 {
                let mut update = Query::update();
                let update = update
                    .table(player_rating::Entity)
                    .value(player_rating::Column::Rating, rate.rating)
                    .and_where(
                        player_rating::Column::MapId
                            .eq(map_id)
                            .and(player_rating::Column::PlayerId.eq(player_id))
                            .and(player_rating::Column::Kind.eq(rate.kind)),
                    );
                let stmt = StatementBuilder::build(&*update, &conn.get_database_backend());
                conn.execute(stmt).await.with_api_err().fit(req_id)?;
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
                    .with_api_err()
                    .fit(req_id)?;
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
                .with_api_err()
                .fit(req_id)?
                .unwrap_or_else(|| panic!("Player rating of {player_id} on {map_id} and rating kind {} should exist in database", rate.kind));

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
    req_id: RequestId,
    MPAuthGuard { login }: MPAuthGuard<{ privilege::ADMIN }>,
    ExtractDbConn(conn): ExtractDbConn,
    Json(body): Json<ResetRatingsBody>,
) -> RecordsResponse<impl Responder> {
    let info = maps::Entity::find()
        .filter(maps::Column::GameId.eq(&body.map_id))
        .inner_join(players::Entity)
        .select_only()
        .columns([maps::Column::Id, maps::Column::Name])
        .column(players::Column::Login)
        .into_tuple()
        .one(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    let Some((map_id, map_name, author_login)) = info else {
        return Err(RecordsErrorKind::from(
            records_lib::error::RecordsError::MapNotFound(body.map_id),
        ))
        .fit(req_id);
    };

    let map_id: u32 = map_id;

    player_rating::Entity::delete_many()
        .filter(player_rating::Column::MapId.eq(map_id))
        .exec(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    rating::Entity::delete_many()
        .filter(rating::Column::MapId.eq(map_id))
        .exec(&conn)
        .await
        .with_api_err()
        .fit(req_id)?;

    json(ResetRatingsResponse {
        admin_login: login,
        map_name,
        author_login,
    })
}
