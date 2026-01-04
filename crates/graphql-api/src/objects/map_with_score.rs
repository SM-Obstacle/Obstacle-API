use async_graphql::SimpleObject;

use crate::objects::map::Map;

#[derive(SimpleObject, Debug, Clone)]
pub struct MapWithScore {
    pub rank: i32,
    pub map: Map,
}
