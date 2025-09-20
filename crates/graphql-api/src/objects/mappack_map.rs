use async_graphql::SimpleObject;

use crate::objects::map::Map;

#[derive(SimpleObject)]
pub struct MappackMap {
    pub rank: i32,
    pub last_rank: i32,
    pub map: Map,
}
