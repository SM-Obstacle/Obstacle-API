use entity::ranking_period;
use sea_orm::{ConnectionTrait, EntityTrait as _, QueryOrder as _, QuerySelect as _};

pub mod connection_input;
pub mod page_input;
pub mod pagination_result;
pub mod records_filter;

pub async fn get_last_period_id<C: ConnectionTrait>(conn: &C) -> Option<i32> {
    ranking_period::Entity::find()
        .select_only()
        .column(ranking_period::Column::PeriodId)
        .order_by_desc(ranking_period::Column::PeriodDate)
        .limit(1)
        .into_tuple()
        .one(conn)
        .await
        .ok()
        .flatten()
}
