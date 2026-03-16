use async_graphql::Subscription;
use entity::records;
use futures::{Stream, StreamExt as _};
use records_lib::{internal, records_notifier::LatestRecordsSubscription};
use sea_orm::{DbConn, EntityTrait};

use crate::{error::GqlResult, objects::ranked_record::RankedRecord};

pub struct SubscriptionRoot {
    records_sub: LatestRecordsSubscription,
}

impl SubscriptionRoot {
    pub fn new(records_sub: LatestRecordsSubscription) -> Self {
        Self { records_sub }
    }
}

#[Subscription]
impl SubscriptionRoot {
    async fn latest_records(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> impl Stream<Item = GqlResult<RankedRecord>> {
        let db = ctx.data_unchecked::<DbConn>();
        self.records_sub
            .subscribe_new_client()
            .then(move |new_record| async move {
                let record = records::Entity::find_by_id(new_record.record_id)
                    .one(db)
                    .await?
                    .ok_or_else(|| {
                        internal!(
                            "new record yielded by stream returned an invalid record id: {}",
                            new_record.record_id
                        )
                    })?;

                Ok(RankedRecord {
                    inner: records::RankedRecord {
                        rank: new_record.rank,
                        record,
                    },
                })
            })
    }
}
