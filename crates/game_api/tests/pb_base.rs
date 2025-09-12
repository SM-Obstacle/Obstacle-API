#[derive(Debug, PartialEq, serde::Deserialize)]
pub struct ResponseItem {
    pub cp_num: u32,
    pub time: i32,
}

#[derive(serde::Deserialize)]
pub struct Response {
    pub rs_count: i32,
    pub cps_times: Vec<ResponseItem>,
}
