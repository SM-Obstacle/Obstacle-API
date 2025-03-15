//! This module contains the [`Time`] struct, used to format times.

use std::fmt;

/// The formatted version of a time.
pub struct Time(pub i32);

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == -1 {
            return f.write_str("invalid time");
        }

        let time = self.0 as f64;

        let (hor, min, sec, ms) = (
            (time / 3_600_000.).floor(),
            (time % 3_600_000. / 60_000.).floor(),
            (time % 60_000. / 1_000.).floor(),
            (time % 1_000. / 10.).floor(),
        );

        write!(f, "{hor:02}:{min:02}:{sec:02}.{ms:02}")
    }
}
