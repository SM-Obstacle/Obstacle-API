use entity::event_edition;

/// Represents a type that has an expire date.
pub trait Expirable {
    /// Returns the UTC expire date.
    fn expire_date(&self) -> Option<chrono::NaiveDateTime>;

    /// Returns the number of seconds until the edition expires from now.
    ///
    /// If the edition doesn't expire (it hasn't a TTL), it returns `None`.
    fn expires_in(&self) -> Option<i64> {
        self.expire_date()
            .map(|d| (d - chrono::Utc::now().naive_utc()).num_seconds())
    }

    /// Returns whether the edition has expired or not.
    fn has_expired(&self) -> bool {
        self.expires_in().filter(|n| *n < 0).is_some()
    }
}

impl Expirable for event_edition::Model {
    fn expire_date(&self) -> Option<chrono::NaiveDateTime> {
        self.ttl.and_then(|ttl| {
            self.start_date
                .checked_add_signed(chrono::Duration::seconds(ttl as _))
        })
    }
}
