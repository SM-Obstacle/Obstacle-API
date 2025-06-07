//! Optional event management.
//!
//! See the [`OptEvent`] type for more information.

use std::fmt;

use crate::models;

/// Optional event instances.
///
/// Operations involving events often follow the same patterns than operations without events.
/// This is why these operations generally take a value of this type as a parameter, if needed.
///
/// For example, a player's record on a map might not be the same when made in an event.
///
/// This type is created with event instances if available, with the [`OptEvent::new`] method.
/// Otherwise, use the [`Default`] implementation.
#[derive(Clone, Copy, Default)]
pub struct OptEvent<'a> {
    /// The optional held event instances.
    pub event: Option<(&'a models::Event, &'a models::EventEdition)>,
}

impl<'a> OptEvent<'a> {
    /// Creates a new optional event instances with the provided event instances,
    /// in a more convenient way.
    pub fn new(event: &'a models::Event, edition: &'a models::EventEdition) -> Self {
        Self {
            event: Some((event, edition)),
        }
    }
}

impl fmt::Debug for OptEvent<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.event {
            Some((ev, ed)) => {
                write!(f, "Some({}/{})", ev.handle, ed.id)
            }
            None => write!(f, "None"),
        }
    }
}

impl OptEvent<'_> {
    /// Returns the SQL fragment builder based on the current held optional event instances.
    ///
    /// This is used to build SQL queries depending on whether an event is given or not.
    pub fn sql_frag_builder(&self) -> SqlFragmentBuilder<'_, '_> {
        SqlFragmentBuilder { event: self }
    }
}

/// The SQL fragment builder, based on the current held optional event instances.
///
/// This type is returned by the [`OptEvent::sql_frag_builder`] method.
pub struct SqlFragmentBuilder<'a, 'b> {
    event: &'a OptEvent<'b>,
}

impl SqlFragmentBuilder<'_, '_> {
    /// Pushes the SQL view name to the provided query builder, based on the optional
    /// event the context has.
    ///
    /// It bounds the provided `label` argument to the view name.
    ///
    /// This is used because records made in an event context are retrieved from a different view
    /// than normal records.
    pub fn push_event_view_name<'a, 'args, DB: sqlx::Database>(
        &self,
        qb: &'a mut sqlx::QueryBuilder<'args, DB>,
        label: &str,
    ) -> &'a mut sqlx::QueryBuilder<'args, DB> {
        match self.event.event {
            Some(_) => qb.push("global_event_records "),
            None => qb.push("global_records "),
        }
        .push(label)
    }

    /// Pushes the SQL join fragment to the provided query builder, based on the optional
    /// event the context has.
    ///
    /// If the context has an event, it adds a join clause to the query with the table
    /// containing records made in an event context. The `label` argument is the label bound to this
    /// table, and `records_label` is the label that was previously bound to the normal records table.
    ///
    /// Otherwise, it doesn't do anything.
    #[inline(always)]
    pub fn push_event_join<'a, 'args, DB: sqlx::Database>(
        &self,
        qb: &'a mut sqlx::QueryBuilder<'args, DB>,
        label: &str,
        records_label: &str,
    ) -> &'a mut sqlx::QueryBuilder<'args, DB> {
        match self.event.event {
            Some(_) => qb
                .push("inner join event_edition_records ")
                .push(label)
                .push(" on ")
                .push(label)
                .push(".record_id = ")
                .push(records_label)
                .push(".record_id"),
            None => qb,
        }
    }

    /// Pushes an SQL condition filtering the results based on the optional event the context has.
    ///
    /// If the context has an event, it adds an SQL condition, starting with `AND ...`, that checks
    /// if the event ID and edition ID corresponds to those in the context.
    ///
    /// Otherwise, it doesn't do anything.
    #[inline(always)]
    pub fn push_event_filter<'a, 'args, DB: sqlx::Database>(
        &self,
        qb: &'a mut sqlx::QueryBuilder<'args, DB>,
        label: &str,
    ) -> &'a mut sqlx::QueryBuilder<'args, DB>
    where
        u32: sqlx::Encode<'args, DB> + sqlx::Type<DB>,
    {
        match self.event.event.map(|(ev, ed)| (ev.id, ed.id)) {
            Some((ev, ed)) => qb
                .push("and (")
                .push(label)
                .push(".event_id = ")
                .push_bind(ev)
                .push(" and ")
                .push(label)
                .push(".edition_id = ")
                .push_bind(ed)
                .push(")"),
            None => qb,
        }
    }
}
