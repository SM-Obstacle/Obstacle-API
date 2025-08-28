use crate::{RecordsResult, RecordsResultExt};
use actix_web::web;
use entity::{checkpoint_times, global_event_records, global_records, maps, players};
use futures::StreamExt;
use records_lib::opt_event::OptEvent;
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect,
    StreamTrait,
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
    let mut times = match event.event {
        Some((ev, ed)) => global_event_records::Entity::find()
            .inner_join(maps::Entity)
            .inner_join(players::Entity)
            .join(
                sea_orm::JoinType::InnerJoin,
                global_event_records::Entity::belongs_to(checkpoint_times::Entity)
                    .from(global_event_records::Column::RecordId)
                    .to(checkpoint_times::Column::RecordId)
                    .into(),
            )
            .filter(
                maps::Column::GameId
                    .eq(map_uid)
                    .and(players::Column::Login.eq(player_login))
                    .and(global_event_records::Column::EventId.eq(ev.id))
                    .and(global_event_records::Column::EditionId.eq(ed.id)),
            )
            .select_only()
            .column_as(global_event_records::Column::RespawnCount, "rs_count")
            .column_as(checkpoint_times::Column::CpNum, "cp_num")
            .column_as(checkpoint_times::Column::Time, "time")
            .into_model::<PbResponseItem>()
            .stream(conn)
            .await
            .with_api_err()?,
        None => global_records::Entity::find()
            .inner_join(maps::Entity)
            .inner_join(players::Entity)
            .join(
                sea_orm::JoinType::InnerJoin,
                global_records::Entity::belongs_to(checkpoint_times::Entity)
                    .from(global_records::Column::RecordId)
                    .to(checkpoint_times::Column::RecordId)
                    .into(),
            )
            .filter(
                maps::Column::GameId
                    .eq(map_uid)
                    .and(players::Column::Login.eq(player_login)),
            )
            .select_only()
            .column_as(global_records::Column::RespawnCount, "rs_count")
            .column_as(checkpoint_times::Column::CpNum, "cp_num")
            .column_as(checkpoint_times::Column::Time, "time")
            .into_model::<PbResponseItem>()
            .stream(conn)
            .await
            .with_api_err()?,
    };

    let mut res = PbResponse {
        rs_count: 0,
        cps_times: Vec::with_capacity(times.size_hint().0),
    };

    while let Some(PbResponseItem {
        rs_count,
        cp_num,
        time,
    }) = times.next().await.transpose().with_api_err()?
    {
        res.rs_count = rs_count;
        res.cps_times.push(PbCpTimesResponseItem { cp_num, time });
    }

    Ok(res)
}
