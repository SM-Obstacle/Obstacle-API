use records_lib::event;

pub struct MedalTimes {
    pub inner: event::MedalTimes,
}

impl From<event::MedalTimes> for MedalTimes {
    fn from(inner: event::MedalTimes) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl MedalTimes {
    async fn bronze_time(&self) -> i32 {
        self.inner.bronze_time
    }

    async fn silver_time(&self) -> i32 {
        self.inner.silver_time
    }

    async fn gold_time(&self) -> i32 {
        self.inner.gold_time
    }

    async fn champion_time(&self) -> i32 {
        self.inner.champion_time
    }
}
