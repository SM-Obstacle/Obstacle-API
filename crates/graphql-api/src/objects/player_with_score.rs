use async_graphql::SimpleObject;

use crate::objects::player::Player;

#[derive(SimpleObject, Debug, Clone)]
pub struct PlayerWithScore {
    pub rank: i32,
    pub player: Player,
}
