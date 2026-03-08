use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
};

use chrono::{DateTime, Utc};
use futures::Stream;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};

#[derive(Clone, Debug, serde::Serialize)]
pub struct NewRecordPlayer {
    pub login: String,
    pub name: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NewRecordMap {
    pub map_uid: String,
    pub name: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NewRecordEvent {
    pub record_id: u32,
    pub rank: i32,
    pub player: NewRecordPlayer,
    pub map: NewRecordMap,
    pub time: i32,
    pub record_date: DateTime<Utc>,
}

struct LatestRecordsStream {
    inner: BroadcastStream<NewRecordEvent>,
}

impl Stream for LatestRecordsStream {
    type Item = NewRecordEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // SAFETY: we're retrieving a field of `Self`
        let rx = unsafe { self.map_unchecked_mut(|f| &mut f.inner) };
        match ready!(rx.poll_next(cx)) {
            Some(Ok(record)) => Poll::Ready(Some(record)),
            Some(Err(BroadcastStreamRecvError::Lagged(_))) => Poll::Pending,
            None => Poll::Ready(None),
        }
    }
}

#[derive(Clone)]
struct Inner {
    tx: Arc<broadcast::Sender<NewRecordEvent>>,
}

#[derive(Clone, Default)]
pub struct RecordsNotifier {
    inner: Inner,
}

impl Default for Inner {
    fn default() -> Self {
        let (tx, _rx) = broadcast::channel(10);
        Self { tx: Arc::new(tx) }
    }
}

impl RecordsNotifier {
    pub fn get_subscription(&self) -> LatestRecordsSubscription {
        LatestRecordsSubscription {
            inner: self.inner.clone(),
        }
    }

    pub async fn notify_new_record(&self, event: NewRecordEvent) {
        // We ignore if any receiver received the event or not.
        let _ = self.inner.tx.send(event);
    }
}

#[derive(Clone)]
pub struct LatestRecordsSubscription {
    inner: Inner,
}

impl LatestRecordsSubscription {
    pub fn subscribe_new_client(&self) -> impl Stream<Item = NewRecordEvent> {
        let rx = self.inner.tx.subscribe();
        LatestRecordsStream { inner: rx.into() }
    }
}
