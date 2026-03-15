//! Module related to new records notification.

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
};

use chrono::{DateTime, Utc};
use futures::Stream;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};

/// The type representing the player in a [`NewRecordEvent`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NewRecordPlayer {
    /// The login of the player.
    pub login: String,
    /// The name of the player.
    pub name: String,
}

/// The type representing the map in a [`NewRecordEvent`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NewRecordMap {
    /// The UID of the map.
    pub map_uid: String,
    /// The name of the map.
    pub name: String,
}

/// The type yielded when a new record is notified.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NewRecordEvent {
    /// The record ID.
    pub record_id: u32,
    /// The rank of the record in the map.
    pub rank: i32,
    /// The player who made the record.
    pub player: NewRecordPlayer,
    /// The map on which the record is set.
    pub map: NewRecordMap,
    /// The time of the run.
    pub time: i32,
    /// The date of the record.
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
            Some(Err(BroadcastStreamRecvError::Lagged(_))) => {
                // According to the broadcast receiver documentation, attempting to receive again will
                // return the oldest message still retained in the channel. So the next poll is ready.
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            None => Poll::Ready(None),
        }
    }
}

/// Shared state between the notifier and its subscriptions.
#[derive(Clone)]
struct Shared {
    tx: Arc<broadcast::Sender<NewRecordEvent>>,
}

/// Represents the source that will send the notifications of the new records.
#[derive(Clone, Default)]
pub struct RecordsNotifier {
    inner: Shared,
}

impl Default for Shared {
    fn default() -> Self {
        let (tx, _rx) = broadcast::channel(10);
        Self { tx: Arc::new(tx) }
    }
}

impl RecordsNotifier {
    /// Returns a subscription from this source, to be able to listen for record notifications.
    pub fn get_subscription(&self) -> LatestRecordsSubscription {
        LatestRecordsSubscription {
            inner: self.inner.clone(),
        }
    }

    /// Notifies to every subscriptions that a new record happened.
    pub fn notify_new_record(&self, event: NewRecordEvent) {
        // We ignore if any receiver received the event or not.
        let _ = self.inner.tx.send(event);
    }
}

/// Represents a listener for a new record notification.
#[derive(Clone)]
pub struct LatestRecordsSubscription {
    inner: Shared,
}

impl LatestRecordsSubscription {
    /// Returns a stream from this subscription, yielding each new record.
    ///
    /// The stream returns `None` if this subscription and the notifier from which it was created
    /// are both dropped.
    pub fn subscribe_new_client(&self) -> impl Stream<Item = NewRecordEvent> + 'static {
        let rx = self.inner.tx.subscribe();
        LatestRecordsStream { inner: rx.into() }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use chrono::{TimeZone, Utc};
    use tokio::{sync::mpsc, task, time};
    use tokio_stream::StreamExt;

    use crate::records_notifier::{
        LatestRecordsSubscription, NewRecordEvent, NewRecordMap, NewRecordPlayer, RecordsNotifier,
    };

    async fn timeout<F, T>(f: F) -> Option<T>
    where
        F: Future<Output = T>,
    {
        time::timeout(Duration::from_secs(1), f).await.ok()
    }

    struct Listener {
        rx: mpsc::Receiver<()>,
        task: task::JoinHandle<()>,
        results: Arc<Mutex<Vec<NewRecordEvent>>>,
    }

    impl Listener {
        fn from_subscription(subscription: &LatestRecordsSubscription) -> Self {
            let (tx, rx) = mpsc::channel(1);
            let stream = subscription.subscribe_new_client();
            let results = Arc::new(Mutex::new(Vec::new()));
            let results_in_task = Arc::clone(&results);
            let task = task::spawn(async move {
                let mut stream = stream;
                while let Some(record) = stream.next().await {
                    results_in_task.lock().unwrap().push(record);
                    tx.send(()).await.unwrap();
                }
            });

            Self { rx, task, results }
        }
    }

    #[tokio::test]
    async fn test_notifier() {
        let notifier = RecordsNotifier::default();
        let subscription = notifier.get_subscription();

        let mut listener1 = Listener::from_subscription(&subscription);

        let record1 = NewRecordEvent {
            player: NewRecordPlayer {
                login: "foo_player_login".to_owned(),
                name: "foo_player_name".to_owned(),
            },
            map: NewRecordMap {
                map_uid: "foo_map_uid".to_owned(),
                name: "foo_map_name".to_owned(),
            },
            rank: 1,
            record_date: Utc.with_ymd_and_hms(2026, 3, 15, 15, 0, 0).unwrap(),
            record_id: 1,
            time: 15000,
        };

        notifier.notify_new_record(record1.clone());
        assert!(timeout(listener1.rx.recv()).await.is_some());
        itertools::assert_equal(listener1.results.lock().unwrap().iter(), [&record1]);

        let mut listener2 = Listener::from_subscription(&subscription);

        let record2 = NewRecordEvent {
            player: NewRecordPlayer {
                login: "bar_player_login".to_owned(),
                name: "bar_player_name".to_owned(),
            },
            map: NewRecordMap {
                map_uid: "bar_map_uid".to_owned(),
                name: "bar_map_name".to_owned(),
            },
            rank: 2,
            record_date: Utc.with_ymd_and_hms(2025, 3, 15, 15, 0, 0).unwrap(),
            record_id: 2,
            time: 14000,
        };
        let record3 = NewRecordEvent {
            player: NewRecordPlayer {
                login: "foobar_player_login".to_owned(),
                name: "foobar_player_name".to_owned(),
            },
            map: NewRecordMap {
                map_uid: "foobar_map_uid".to_owned(),
                name: "foobar_map_name".to_owned(),
            },
            rank: 3,
            record_date: Utc.with_ymd_and_hms(2024, 3, 15, 15, 0, 0).unwrap(),
            record_id: 3,
            time: 13000,
        };

        notifier.notify_new_record(record2.clone());
        notifier.notify_new_record(record3.clone());

        assert!(timeout(listener1.rx.recv()).await.is_some());
        assert!(timeout(listener2.rx.recv()).await.is_some());
        assert!(timeout(listener1.rx.recv()).await.is_some());
        assert!(timeout(listener2.rx.recv()).await.is_some());

        itertools::assert_equal(
            listener1.results.lock().unwrap().iter(),
            [&record1, &record2, &record3],
        );
        itertools::assert_equal(
            listener2.results.lock().unwrap().iter(),
            [&record2, &record3],
        );

        drop(subscription);
        drop(notifier);

        assert!(timeout(listener1.task).await.is_some());
        assert!(timeout(listener2.task).await.is_some());
    }
}
