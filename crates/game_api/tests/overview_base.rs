#[derive(Debug, PartialEq, serde::Deserialize)]
pub struct Row {
    pub rank: i32,
    pub login: String,
    pub nickname: String,
    pub time: i32,
}

#[derive(Debug, serde::Deserialize)]
pub struct Response {
    pub response: Vec<Row>,
}
