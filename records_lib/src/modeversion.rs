use std::{fmt, str::FromStr};

use nom::{
    Parser as _,
    bytes::complete::{tag, take_while1},
    combinator::{map, map_res},
};

/// The error emitted by the parse of the [`ModeVersion`] type.
#[derive(thiserror::Error, Debug)]
#[error("invalid mode version (must respect x.y.z form)")]
pub struct ModeVersionParseErr;

/// The ShootMania Obstacle Mode version, like `2.7.4`.
#[derive(Clone, Copy)]
pub struct ModeVersion {
    /// The MAJOR part.
    pub major: u8,
    /// The MINOR part.
    pub minor: u8,
    /// The PATCH part.
    pub patch: u8,

    /// The total length of the string of the version.
    ///
    /// This is used to encode the version in a column in the database.
    len: u8,
}

impl ModeVersion {
    /// Returns a mode version from the major, minor, and patch parts.
    pub fn new(major: u8, minor: u8, patch: u8) -> Self {
        // We cap the length to 3 because the total length can't exceed 11 = 9 + 2 (the dots).
        #[inline]
        fn part_len(x: u8) -> u8 {
            match x {
                0..=9 => 1,
                10..=99 => 2,
                _ => 3,
            }
        }

        Self {
            major,
            minor,
            patch,
            len: part_len(major) + part_len(minor) + part_len(patch) + 2,
        }
    }
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

impl<'a> sqlx::Encode<'a, sqlx::MySql> for ModeVersion {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::MySql as sqlx::Database>::ArgumentBuffer<'a>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        use std::io::Write as _;

        buf.push(self.len);
        write!(buf, "{self}")?;

        Ok(sqlx::encode::IsNull::No)
    }
}

impl sqlx::Type<sqlx::MySql> for ModeVersion {
    #[inline(always)]
    fn type_info() -> <sqlx::MySql as sqlx::Database>::TypeInfo {
        <str as sqlx::Type<sqlx::MySql>>::type_info()
    }

    #[inline(always)]
    fn compatible(ty: &<sqlx::MySql as sqlx::Database>::TypeInfo) -> bool {
        <str as sqlx::Type<sqlx::MySql>>::compatible(ty)
    }
}

fn parse_u8(input: &str) -> nom::IResult<&str, u8> {
    map_res(take_while1(|c: char| c.is_ascii_digit()), |input: &str| {
        input.parse()
    })
    .parse(input)
}

fn parse_mode_version(input: &str) -> nom::IResult<&str, ModeVersion> {
    map(
        (parse_u8, tag("."), parse_u8, tag("."), parse_u8),
        |(major, _, minor, _, patch)| ModeVersion::new(major, minor, patch),
    )
    .parse(input)
}

impl FromStr for ModeVersion {
    type Err = ModeVersionParseErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_mode_version(s)
            .map(|(_, o)| o)
            .map_err(|_| ModeVersionParseErr)
    }
}
