use actix_web::{web::Query, Responder};
use futures::StreamExt;
use records_lib::{models, Database, GetSqlFragments};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tracing_actix_web::RequestId;

use crate::{utils::json, FitRequestId, RecordsResponse, RecordsResultExt, Res};

#[derive(Deserialize)]
pub struct PbBody {
    pub map_uid: String,
}

pub type PbReq = Query<PbBody>;

#[derive(Serialize)]
struct PbResponse {
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
    login: String,
    req_id: RequestId,
    db: Res<Database>,
    Query(PbBody { map_uid }): PbReq,
    event: Option<(&models::Event, &models::EventEdition)>,
) -> RecordsResponse<impl Responder> {
    let (view_name, and_event) = event.get_view();

    let query = format!(
        "select r.respawn_count as rs_count, ct.cp_num as cp_num, ct.time as time
        from {view_name} r
        inner join maps m on m.id = r.map_id
        inner join players p on r.record_player_id = p.id
        inner join checkpoint_times ct on r.record_id = ct.record_id
        where m.game_id = ? and p.login = ?
        {and_event}"
    );

    let query = sqlx::query_as::<_, PbResponseItem>(&query)
        .bind(map_uid)
        .bind(login);

    let query = if let Some((event, edition)) = event {
        query.bind(event.id).bind(edition.id)
    } else {
        query
    };

    let mut times = query.fetch(&db.mysql_pool);

    let mut res = PbResponse {
        rs_count: 0,
        cps_times: Vec::with_capacity(times.size_hint().0),
    };

    while let Some(PbResponseItem {
        rs_count,
        cp_num,
        time,
    }) = times.next().await.transpose().with_api_err().fit(req_id)?
    {
        res.rs_count = rs_count;
        res.cps_times.push(PbCpTimesResponseItem { cp_num, time });
    }

    json(res)
}
