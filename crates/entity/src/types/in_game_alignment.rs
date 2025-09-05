use std::str::FromStr;

use sea_orm::entity::prelude::*;

/// The alignment of an item in the Titlepack menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(1))")]
#[repr(u8)]
pub enum InGameAlignment {
    /// The item is positioned on left.
    #[sea_orm(string_value = "L")]
    Left = b'L',
    /// The item is positioned on right.
    #[sea_orm(string_value = "R")]
    Right = b'R',
    /// The item is positioned on the center.
    #[sea_orm(string_value = "C")]
    Center = b'C',
}

impl InGameAlignment {
    /// Converts an alignment into its corresponding character.
    pub fn to_char(self) -> char {
        self as u8 as _
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InGameAlignmentParseError {
    #[error("Invalid char `{0}`. Expected 'L', 'R', or 'C'")]
    InvalidChar(char),
    #[error("Invalid input length ({0}). Expected 'L', 'R', or 'C'")]
    InvalidLen(usize),
}

impl FromStr for InGameAlignment {
    type Err = InGameAlignmentParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.chars().next() {
            Some(c) if s.len() == 1 => match c {
                'L' => Ok(Self::Left),
                'R' => Ok(Self::Right),
                'C' => Ok(Self::Center),
                _ => Err(InGameAlignmentParseError::InvalidChar(c)),
            },
            _ => Err(InGameAlignmentParseError::InvalidLen(s.len())),
        }
    }
}
