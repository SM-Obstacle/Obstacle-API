use entity::checkpoint_times;
use sea_orm::FromQueryResult;

#[derive(FromQueryResult)]
pub struct CheckpointTime {
    #[sea_orm(nested)]
    pub inner: checkpoint_times::Model,
}

impl From<checkpoint_times::Model> for CheckpointTime {
    fn from(inner: checkpoint_times::Model) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl CheckpointTime {
    async fn cp_num(&self) -> u32 {
        self.inner.cp_num
    }

    async fn time(&self) -> i32 {
        self.inner.time
    }
}
