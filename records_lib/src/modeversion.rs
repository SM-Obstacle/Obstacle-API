use std::{fmt, str::FromStr};

use nom::{
    bytes::complete::{tag, take_while1},
    combinator::{map, map_res},
    sequence::tuple,
};

/// The error emitted by the parse of the [`ModeVersion`] type.
#[derive(thiserror::Error, Debug)]
#[error("invalid mode version (must respect x.y.z form)")]
pub struct ModeVersionParseErr;

/// The ShootMania Obstacle Mode version, like `2.7.4`.
pub struct ModeVersion {
    /// The MAJOR part.
    pub major: u8,
    /// The MINOR part.
    pub minor: u8,
    /// The PATCH part.
    pub patch: u8,
}

impl fmt::Display for ModeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl fmt::Debug for ModeVersion {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

fn parse_u8(input: &str) -> nom::IResult<&str, u8> {
    map_res(take_while1(|c: char| c.is_ascii_digit()), |input: &str| {
        input.parse()
    })(input)
}

fn parse_mode_version(input: &str) -> nom::IResult<&str, ModeVersion> {
    map(
        tuple((parse_u8, tag("."), parse_u8, tag("."), parse_u8)),
        |(major, _, minor, _, patch)| ModeVersion {
            major,
            minor,
            patch,
        },
    )(input)
}

impl FromStr for ModeVersion {
    type Err = ModeVersionParseErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_mode_version(s)
            .map(|(_, o)| o)
            .map_err(|_| ModeVersionParseErr)
    }
}
