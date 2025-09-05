use core::fmt;
use std::str::FromStr;

use nom::{
    Parser as _,
    bytes::complete::{tag, take_while1},
    combinator::{map, map_res},
};
use sea_orm::{
    ColIdx, ColumnType, DbErr, QueryResult, TryGetError, TryGetable,
    sea_query::{ArrayType, Nullable, ValueType, ValueTypeErr},
};

/// The error emitted by the parse of the [`ModeVersion`] type.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[error("invalid mode version (must respect x.y.z form)")]
pub struct ModeVersionParseErr;

/// The ShootMania Obstacle Mode version, like `2.7.4`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ModeVersion {
    /// The MAJOR part.
    pub major: u8,
    /// The MINOR part.
    pub minor: u8,
    /// The PATCH part.
    pub patch: u8,
}

impl From<ModeVersion> for sea_orm::Value {
    fn from(value: ModeVersion) -> Self {
        sea_orm::Value::String(Some(Box::new(value.to_string())))
    }
}

impl TryGetable for ModeVersion {
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError> {
        let string = <String as TryGetable>::try_get_by(res, index)?;
        let parsed = string.parse().map_err(|e| {
            TryGetError::DbErr(DbErr::Type(format!(
                "error when parsing column for mode version: {e}"
            )))
        })?;

        Ok(parsed)
    }
}

impl ValueType for ModeVersion {
    fn try_from(v: sea_orm::Value) -> Result<Self, ValueTypeErr> {
        let string = <String as ValueType>::try_from(v)?;
        let parsed = string.parse().map_err(|_| ValueTypeErr)?;
        Ok(parsed)
    }

    fn type_name() -> String {
        "ModeVersion".to_owned()
    }

    #[inline]
    fn array_type() -> ArrayType {
        <String as ValueType>::array_type()
    }

    #[inline]
    fn column_type() -> ColumnType {
        <String as ValueType>::column_type()
    }
}

impl Nullable for ModeVersion {
    #[inline]
    fn null() -> sea_orm::Value {
        <String as Nullable>::null()
    }
}

impl ModeVersion {
    /// Returns a mode version from the major, minor, and patch parts.
    #[inline]
    pub fn new(major: u8, minor: u8, patch: u8) -> Self {
        Self {
            major,
            minor,
            patch,
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

fn parse_u8(input: &str) -> nom::IResult<&str, u8> {
    map_res(take_while1(|c: char| c.is_ascii_digit()), |input: &str| {
        input.parse()
    })
    .parse(input)
}

fn parse_patch_or_0(input: &str) -> nom::IResult<&str, u8> {
    let out = (tag("."), parse_u8)
        .parse(input)
        .map(|(input, (_, patch))| (input, patch))
        .unwrap_or((input, 0));
    Ok(out)
}

fn parse_mode_version(input: &str) -> nom::IResult<&str, ModeVersion> {
    map(
        (parse_u8, tag("."), parse_u8, parse_patch_or_0),
        |(major, _, minor, patch)| ModeVersion::new(major, minor, patch),
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

#[cfg(test)]
mod tests {
    use super::ModeVersion;

    #[test]
    fn parse_2_7_4() {
        assert_eq!(Ok(ModeVersion::new(2, 7, 4)), "2.7.4".parse());
    }

    #[test]
    fn parse_2_8() {
        assert_eq!(Ok(ModeVersion::new(2, 8, 0)), "2.8".parse());
    }
}
