pub mod expr_tuple;
pub mod query_builder;
pub mod query_trait;

use std::str;

use async_graphql::{ID, connection::CursorType};
use base64::{Engine as _, prelude::BASE64_URL_SAFE};
use chrono::{DateTime, Utc};
use hmac::Mac;
use mkenv::Layer;
use sea_orm::{
    Value,
    sea_query::{IntoValueTuple, SimpleExpr, ValueTuple},
};

use crate::{
    cursors::expr_tuple::{ExprTuple, IntoExprTuple},
    error::{CursorDecodeError, CursorDecodeErrorKind},
};

fn decode_base64(s: &str) -> Result<String, CursorDecodeError> {
    let decoded = BASE64_URL_SAFE
        .decode(s)
        .map_err(|_| CursorDecodeError::from(CursorDecodeErrorKind::NotBase64))?;
    // TODO: replace this with `slice::split_once` once it's stabilized
    // https://github.com/rust-lang/rust/issues/112811
    let idx = decoded
        .iter()
        .position(|b| *b == b'$')
        .ok_or(CursorDecodeError::from(CursorDecodeErrorKind::NoSignature))?;
    let (content, signature) = (&decoded[..idx], &decoded[idx + 1..]);

    let mut mac = crate::config().cursor_secret_key.cursor_secret_key.get();
    mac.update(content);
    mac.verify_slice(signature)
        .map_err(|e| CursorDecodeError::from(CursorDecodeErrorKind::InvalidSignature(e)))?;

    let content = str::from_utf8(content)
        .map_err(|_| CursorDecodeError::from(CursorDecodeErrorKind::NotUtf8))?;

    Ok(content.to_owned())
}

fn encode_base64(s: String) -> String {
    let mut mac = crate::config().cursor_secret_key.cursor_secret_key.get();
    mac.update(s.as_bytes());
    let signature = mac.finalize().into_bytes();

    let mut output = s.into_bytes();
    output.push(b'$');
    output.extend_from_slice(&signature);

    BASE64_URL_SAFE.encode(&output)
}

mod datetime_serde {
    use std::fmt;

    use chrono::{DateTime, Utc};
    use serde::de::{Unexpected, Visitor};

    pub fn serialize<S>(datetime: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let time = datetime.timestamp_millis();
        serializer.serialize_i64(time)
    }

    pub fn deserialize<'de, D>(deser: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct TimestampVisitor;
        impl<'de> Visitor<'de> for TimestampVisitor {
            type Value = DateTime<Utc>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an integer")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_i64(v as _)
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                DateTime::from_timestamp_millis(v).ok_or_else(|| {
                    serde::de::Error::invalid_value(Unexpected::Signed(v), &"a valid timestamp")
                })
            }
        }

        deser.deserialize_i64(TimestampVisitor)
    }
}

#[cold]
#[inline(never)]
fn serialization_failed<T>(_obj: &T, e: serde_json::Error) -> ! {
    panic!(
        "serialization of `{}` failed: {e}",
        std::any::type_name::<T>()
    )
}

fn encode_cursor<T>(prefix: &str, obj: &T) -> String
where
    T: serde::Serialize,
{
    encode_base64(format!(
        "{prefix}:{}",
        serde_json::to_string(obj).unwrap_or_else(|e| serialization_failed(obj, e))
    ))
}

fn decode_cursor<T>(prefix: &str, input: &str) -> Result<T, CursorDecodeError>
where
    T: serde::de::DeserializeOwned,
{
    let decoded = decode_base64(input)?;

    let Some((input_prefix, input)) = decoded.split_once(':') else {
        return Err(CursorDecodeErrorKind::MissingPrefix.into());
    };

    if input_prefix != prefix {
        return Err(CursorDecodeErrorKind::InvalidPrefix.into());
    }

    serde_json::from_str(input).map_err(|_| CursorDecodeErrorKind::InvalidData.into())
}

#[derive(PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecordDateCursor<T = u32> {
    #[serde(with = "datetime_serde")]
    pub record_date: DateTime<Utc>,
    pub data: T,
}

impl<T> CursorType for RecordDateCursor<T>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    type Error = CursorDecodeError;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        decode_cursor("record_date", s)
    }

    fn encode_cursor(&self) -> String {
        encode_cursor("record_date", self)
    }
}

impl<T> IntoExprTuple for &RecordDateCursor<T>
where
    T: Into<SimpleExpr> + Clone,
{
    fn into_expr_tuple(self) -> ExprTuple {
        (self.record_date, self.data.clone()).into_expr_tuple()
    }
}

impl<T> IntoValueTuple for &RecordDateCursor<T>
where
    T: Into<Value> + Clone,
{
    fn into_value_tuple(self) -> ValueTuple {
        (self.record_date, self.data.clone()).into_value_tuple()
    }
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordRankCursor<T = u32> {
    #[serde(with = "datetime_serde")]
    pub record_date: DateTime<Utc>,
    pub time: i32,
    pub data: T,
}

impl<T> CursorType for RecordRankCursor<T>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    type Error = CursorDecodeError;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        decode_cursor("record_rank", s)
    }

    fn encode_cursor(&self) -> String {
        encode_cursor("record_rank", self)
    }
}

impl<T> IntoExprTuple for &RecordRankCursor<T>
where
    T: Into<SimpleExpr> + Clone,
{
    fn into_expr_tuple(self) -> ExprTuple {
        (self.time, self.record_date, self.data.clone()).into_expr_tuple()
    }
}

impl<T> IntoValueTuple for &RecordRankCursor<T>
where
    T: Into<Value> + Clone,
{
    fn into_value_tuple(self) -> sea_orm::sea_query::ValueTuple {
        (self.time, self.record_date, self.data.clone()).into_value_tuple()
    }
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextCursor<T = u32> {
    pub text: String,
    pub data: T,
}

impl<T> CursorType for TextCursor<T>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    type Error = CursorDecodeError;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        decode_cursor("text", s)
    }

    fn encode_cursor(&self) -> String {
        encode_cursor("text", self)
    }
}

impl<T> IntoExprTuple for &TextCursor<T>
where
    T: Into<SimpleExpr> + Clone,
{
    fn into_expr_tuple(self) -> ExprTuple {
        (self.text.clone(), self.data.clone()).into_expr_tuple()
    }
}

impl<T> IntoValueTuple for &TextCursor<T>
where
    T: Into<Value> + Clone,
{
    fn into_value_tuple(self) -> ValueTuple {
        (self.text.clone(), self.data.clone()).into_value_tuple()
    }
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct F64Cursor<T = u32> {
    pub score: f64,
    pub data: T,
}

impl<T> CursorType for F64Cursor<T>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    type Error = CursorDecodeError;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        decode_cursor("score", s)
    }

    fn encode_cursor(&self) -> String {
        encode_cursor("score", self)
    }
}

impl<T> IntoExprTuple for &F64Cursor<T>
where
    T: Into<SimpleExpr> + Clone,
{
    fn into_expr_tuple(self) -> ExprTuple {
        (self.score, self.data.clone()).into_expr_tuple()
    }
}

impl<T> IntoValueTuple for &F64Cursor<T>
where
    T: Into<Value> + Clone,
{
    fn into_value_tuple(self) -> ValueTuple {
        (self.score, self.data.clone()).into_value_tuple()
    }
}

pub struct ConnectionParameters<C = ID> {
    pub before: Option<C>,
    pub after: Option<C>,
    pub first: Option<usize>,
    pub last: Option<usize>,
}

impl<C> Default for ConnectionParameters<C> {
    fn default() -> Self {
        Self {
            before: Default::default(),
            after: Default::default(),
            first: Default::default(),
            last: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use async_graphql::connection::CursorType;
    use base64::{Engine as _, prelude::BASE64_URL_SAFE};
    use chrono::{DateTime, SubsecRound, Utc};
    use sha2::digest::MacError;

    use crate::{
        config::InitError,
        cursors::{F64Cursor, TextCursor},
        error::{CursorDecodeError, CursorDecodeErrorKind},
    };

    use super::{RecordDateCursor, RecordRankCursor};

    fn setup() {
        match crate::init_config() {
            Ok(_) | Err(InitError::ConfigAlreadySet) => (),
            Err(InitError::Config(e)) => {
                panic!("test setup error: {e}");
            }
        }
    }

    fn test_cursor_round_trip<C>(source: &C, expected: &C)
    where
        C: CursorType<Error: fmt::Debug + PartialEq> + fmt::Debug + PartialEq,
    {
        let cursor = source.encode_cursor();

        let decoded = C::decode_cursor(&cursor);
        assert_eq!(decoded.as_ref(), Ok(expected));
    }

    fn test_decode_cursor_errors<C>()
    where
        C: CursorType<Error = CursorDecodeError>,
    {
        let decoded = <C>::decode_cursor("foobar").err();
        assert_eq!(
            decoded.map(|e| e.kind),
            Some(CursorDecodeErrorKind::NotBase64)
        );

        let encoded = BASE64_URL_SAFE.encode("foobar");
        let decoded = <C>::decode_cursor(&encoded).err();
        assert_eq!(
            decoded.map(|e| e.kind),
            Some(CursorDecodeErrorKind::NoSignature)
        );

        let encoded = BASE64_URL_SAFE.encode("foobar$signature");
        let decoded = <C>::decode_cursor(&encoded).err();
        assert_eq!(
            decoded.map(|e| e.kind),
            Some(CursorDecodeErrorKind::InvalidSignature(MacError))
        );
    }

    fn test_encode_cursor<C: CursorType>(cursor: &C, expected: &str) {
        let cursor = cursor.encode_cursor();

        let decoded = BASE64_URL_SAFE
            .decode(&cursor)
            .expect("cursor should be encoded as base64");
        let expected = format!("{expected}$");
        assert!(decoded.starts_with(expected.as_bytes()));
    }

    #[test]
    fn encode_date_cursor() {
        setup();

        test_encode_cursor(
            &RecordDateCursor {
                record_date: DateTime::from_timestamp_millis(10).unwrap(),
                data: 0,
            },
            r#"record_date:{"record_date":10,"data":0}"#,
        );
    }

    #[test]
    fn date_cursor_round_trip() {
        setup();

        let now = Utc::now();
        test_cursor_round_trip(
            &RecordDateCursor {
                record_date: now,
                data: 0,
            },
            &RecordDateCursor {
                record_date: now.trunc_subsecs(3),
                data: 0,
            },
        );
    }

    #[test]
    fn decode_date_cursor_errors() {
        setup();
        test_decode_cursor_errors::<RecordDateCursor>();
    }

    #[test]
    fn decode_rank_cursor_errors() {
        setup();
        test_decode_cursor_errors::<RecordRankCursor>();
    }

    #[test]
    fn decode_text_cursor_errors() {
        setup();
        test_decode_cursor_errors::<TextCursor>();
    }

    #[test]
    fn decode_score_cursor_errors() {
        setup();
        test_decode_cursor_errors::<F64Cursor>();
    }

    #[test]
    fn encode_rank_cursor() {
        setup();

        test_encode_cursor(
            &RecordRankCursor {
                record_date: DateTime::from_timestamp_millis(26).unwrap(),
                time: 1000,
                data: 24,
            },
            r#"record_rank:{"record_date":26,"time":1000,"data":24}"#,
        );
    }

    #[test]
    fn rank_cursor_round_trip() {
        setup();

        let now = Utc::now();
        test_cursor_round_trip(
            &RecordRankCursor {
                record_date: now,
                time: 2000,
                data: 34,
            },
            &RecordRankCursor {
                record_date: now.trunc_subsecs(3),
                time: 2000,
                data: 34,
            },
        );
    }

    #[test]
    fn encode_text_cursor() {
        setup();
        test_encode_cursor(
            &TextCursor {
                text: "hello".to_owned(),
                data: 123,
            },
            r#"text:{"text":"hello","data":123}"#,
        );
    }

    #[test]
    fn text_cursor_round_trip() {
        setup();
        test_cursor_round_trip(
            &TextCursor {
                text: "booga".to_owned(),
                data: 232,
            },
            &TextCursor {
                text: "booga".to_owned(),
                data: 232,
            },
        );
    }

    #[test]
    fn encode_score_cursor() {
        setup();
        test_encode_cursor(
            &F64Cursor {
                score: 12.34,
                data: 2445,
            },
            r#"score:{"score":12.34,"data":2445}"#,
        );
    }

    #[test]
    fn decode_score_cursor() {
        setup();
    }

    #[test]
    fn score_cursor_round_trip() {
        setup();
        test_cursor_round_trip(
            &F64Cursor {
                score: 24.6,
                data: 123,
            },
            &F64Cursor {
                score: 24.6,
                data: 123,
            },
        );
    }
}
