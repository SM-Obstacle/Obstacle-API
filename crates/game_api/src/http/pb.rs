use crate::{RecordsResult, RecordsResultExt};
use actix_web::web;
use entity::{checkpoint_times, global_event_records, global_records, maps, players, records};
use futures::{Stream as _, StreamExt, TryStreamExt};
use records_lib::opt_event::OptEvent;
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, FromQueryResult, StatementBuilder, StreamTrait,
    prelude::Expr, sea_query::Query,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PbBody {
    pub map_uid: String,
}

pub type PbReq = web::Query<PbBody>;

#[derive(Serialize, Default)]
pub struct PbResponse {
    rs_count: i32,
    cps_times: Vec<PbCpTimesResponseItem>,
}

#[derive(FromQueryResult)]
struct PbResponseItem {
    rs_count: i32,
    cp_num: u32,
    time: i32,
}

#[derive(Serialize)]
struct PbCpTimesResponseItem {
    cp_num: u32,
    time: i32,
}

pub async fn pb<C: ConnectionTrait + StreamTrait>(
    conn: &C,
    player_login: &str,
    map_uid: &str,
    event: OptEvent<'_>,
) -> RecordsResult<PbResponse> {
    let mut select = Query::select();

    match event.event {
        Some((ev, ed)) => {
            select.from_as(global_event_records::Entity, "r").and_where(
                Expr::col(("r", global_event_records::Column::EventId))
                    .eq(ev.id)
                    .and(Expr::col(("r", global_event_records::Column::EditionId)).eq(ed.id)),
            );
        }
        None => {
            select.from_as(global_records::Entity, "r");
        }
    }

    let times = select
        .inner_join(
            maps::Entity,
            maps::Column::Id
                .into_expr()
                .eq(Expr::col(("r", records::Column::MapId))),
        )
        .inner_join(
            players::Entity,
            players::Column::Id
                .into_expr()
                .eq(Expr::col(("r", records::Column::RecordPlayerId))),
        )
        .inner_join(
            checkpoint_times::Entity,
            checkpoint_times::Column::RecordId
                .into_expr()
                .eq(Expr::col(("r", records::Column::RecordId))),
        )
        .and_where(
            maps::Column::GameId
                .eq(map_uid)
                .and(players::Column::Login.eq(player_login)),
        )
        .expr_as(Expr::col(("r", records::Column::RespawnCount)), "rs_count")
        .expr_as(
            Expr::col((checkpoint_times::Entity, checkpoint_times::Column::CpNum)),
            "cp_num",
        )
        .expr_as(
            Expr::col((checkpoint_times::Entity, checkpoint_times::Column::Time)),
            "time",
        );

    let stmt = StatementBuilder::build(&*times, &conn.get_database_backend());
    let times = conn
        .stream(stmt)
        .await
        .with_api_err()?
        .map_ok(|result| PbResponseItem::from_query_result(&result, ""))
        .map(Result::flatten);

    let mut res = PbResponse {
        rs_count: 0,
        cps_times: Vec::with_capacity(times.size_hint().0),
    };

    futures::pin_mut!(times);

    while let Some(PbResponseItem {
        rs_count,
        cp_num,
        time,
    }) = times.try_next().await.with_api_err()?
    {
        res.rs_count = rs_count;
        res.cps_times.push(PbCpTimesResponseItem { cp_num, time });
    }

    Ok(res)
}
