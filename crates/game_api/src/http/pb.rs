use crate::{RecordsResult, RecordsResultExt};
use actix_web::web::Query;
use futures::StreamExt;
use records_lib::{MySqlPool, opt_event::OptEvent};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Deserialize)]
pub struct PbBody {
    pub map_uid: String,
}

pub type PbReq = Query<PbBody>;

#[derive(Serialize, Default)]
pub struct PbResponse {
    rs_count: i32,
    cps_times: Vec<PbCpTimesResponseItem>,
}

#[derive(FromRow)]
struct PbResponseItem {
    rs_count: i32,
    cp_num: u32,
    time: i32,
}

#[derive(Serialize, FromRow)]
struct PbCpTimesResponseItem {
    cp_num: u32,
    time: i32,
}

pub async fn pb(
    db: MySqlPool,
    player_login: &str,
    map_uid: &str,
    event: OptEvent<'_>,
) -> RecordsResult<PbResponse> {
    let builder = event.sql_frag_builder();

    let mut query = sqlx::QueryBuilder::new(
        "select r.respawn_count as rs_count, ct.cp_num as cp_num, ct.time as time
        from ",
    );
    builder
        .push_event_view_name(&mut query, "r")
        .push(
            " inner join maps m on m.id = r.map_id \
            inner join players p on r.record_player_id = p.id \
            inner join checkpoint_times ct on r.record_id = ct.record_id \
            where m.game_id = ",
        )
        .push_bind(map_uid)
        .push(" and p.login = ")
        .push_bind(player_login)
        .push(" ");
    let query = builder
        .push_event_filter(&mut query, "r")
        .build_query_as::<PbResponseItem>();

    let mut times = query.fetch(&db);

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
