use crate::{RecordsResult, RecordsResultExt};
use actix_web::web::Query;
use futures::StreamExt;
use records_lib::context::{HasMapUid, HasMySqlPool, HasPlayerLogin};
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

pub async fn pb<C>(ctx: C) -> RecordsResult<PbResponse>
where
    C: HasPlayerLogin + HasMapUid + HasMySqlPool,
{
    let builder = ctx.sql_frag_builder();

    let mut query = sqlx::QueryBuilder::new(
        "select r.respawn_count as rs_count, ct.cp_num as cp_num, ct.time as time
        from ",
    );
    builder.push_event_view_name(&mut query, "r").push(
        " inner join maps m on m.id = r.map_id \
            inner join players p on r.record_player_id = p.id \
            inner join checkpoint_times ct on r.record_id = ct.record_id \
            where m.game_id = ? and p.login = ? ",
    );
    let query = builder
        .push_event_filter(&mut query, "r")
        .build_query_as::<PbResponseItem>();

    let pool = ctx.get_mysql_pool();
    let mut times = query.fetch(&pool);

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
