use entity::event_category;
use sea_orm::FromQueryResult;

#[derive(FromQueryResult)]
pub struct EventCategory {
    #[sea_orm(nested)]
    pub inner: event_category::Model,
}

impl From<event_category::Model> for EventCategory {
    fn from(inner: event_category::Model) -> Self {
        Self { inner }
    }
}

#[async_graphql::Object]
impl EventCategory {
    async fn handle(&self) -> &str {
        &self.inner.handle
    }

    async fn name(&self) -> &str {
        &self.inner.name
    }

    async fn banner_img_url(&self) -> Option<&str> {
        self.inner.banner_img_url.as_deref()
    }

    async fn hex_color(&self) -> Option<&str> {
        self.inner.hex_color.as_deref()
    }
}
