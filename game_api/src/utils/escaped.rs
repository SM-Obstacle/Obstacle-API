use std::ops::{Deref, DerefMut};

use async_graphql::{Scalar, ScalarType};
use serde::{Deserialize, Serialize};
use sqlx::{Database, Decode};

fn escape(s: String) -> String {
    let mut out = String::new();
    let pile_o_bits = &s;
    let mut last = 0;
    for (i, ch) in s.bytes().enumerate() {
        if let '<' | '>' | '&' | '\'' | '"' = ch as char {
            out.push_str(&pile_o_bits[last..i]);
            let s = match ch as char {
                '>' => "&gt;",
                '<' => "&lt;",
                '&' => "&amp;",
                '\'' => "&#39;",
                '"' => "&quot;",
                _ => unreachable!(),
            };
            out.push_str(s);
            last = i + 1;
        }
    }

    if last < s.len() {
        out.push_str(&pile_o_bits[last..]);
    }

    out
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(from = "String")]
pub struct Escaped(pub String);

impl<'r, DB: Database> Decode<'r, DB> for Escaped
where
    String: Decode<'r, DB>,
{
    fn decode(
        value: <DB as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <String as Decode<DB>>::decode(value)?;
        Ok(value.into())
    }
}

#[Scalar]
impl ScalarType for Escaped {
    fn parse(value: async_graphql::Value) -> async_graphql::InputValueResult<Self> {
        <String as ScalarType>::parse(value)
            .map(|s| s.into())
            .map_err(|e| e.propagate())
    }

    fn to_value(&self) -> async_graphql::Value {
        <String as ScalarType>::to_value(&self.0)
    }
}

impl From<String> for Escaped {
    fn from(value: String) -> Self {
        Self(escape(value))
    }
}

impl From<Escaped> for String {
    fn from(value: Escaped) -> Self {
        escape(value.0)
    }
}

impl Deref for Escaped {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Escaped {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
