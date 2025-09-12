//! Optional event management.
//!
//! See the [`OptEvent`] type for more information.

use std::fmt;

use entity::{event, event_edition};

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
    pub event: Option<(&'a event::Model, &'a event_edition::Model)>,
}

impl<'a> OptEvent<'a> {
    /// Creates a new optional event instances with the provided event instances,
    /// in a more convenient way.
    pub fn new(event: &'a event::Model, edition: &'a event_edition::Model) -> Self {
        Self {
            event: if edition.is_transparent != 0 {
                None
            } else {
                Some((event, edition))
            },
        }
    }

    /// Returns the current event.
    pub fn get(&self) -> Option<(&'a event::Model, &'a event_edition::Model)> {
        self.event.filter(|(_, ed)| ed.is_transparent == 0)
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
