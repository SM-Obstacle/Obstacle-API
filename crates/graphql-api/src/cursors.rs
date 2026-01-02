use std::str;

use async_graphql::{ID, connection::CursorType};
use base64::{Engine as _, prelude::BASE64_URL_SAFE};
use chrono::{DateTime, Utc};
use hmac::Mac;
use mkenv::Layer;
use sea_orm::sea_query::IntoValueTuple;

use crate::error::CursorDecodeErrorKind;

pub const CURSOR_MAX_LIMIT: usize = 100;
pub const CURSOR_DEFAULT_LIMIT: usize = 50;

fn decode_base64(s: &str) -> Result<String, CursorDecodeErrorKind> {
    let decoded = BASE64_URL_SAFE
        .decode(s)
        .map_err(|_| CursorDecodeErrorKind::NotBase64)?;
    let idx = decoded
        .iter()
        .position(|b| *b == b'$')
        .ok_or(CursorDecodeErrorKind::NoSignature)?;
    let (content, signature) = (&decoded[..idx], &decoded[idx + 1..]);

    let mut mac = crate::config().cursor_secret_key.cursor_secret_key.get();
    mac.update(content);
    mac.verify_slice(signature)
        .map_err(CursorDecodeErrorKind::InvalidSignature)?;

    let content = str::from_utf8(content).map_err(|_| CursorDecodeErrorKind::NotUtf8)?;

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

fn check_prefix<I, S>(splitted: I) -> Result<(), CursorDecodeErrorKind>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    match splitted
        .into_iter()
        .next()
        .filter(|s| s.as_ref() == "record")
    {
        Some(_) => Ok(()),
        None => Err(CursorDecodeErrorKind::WrongPrefix),
    }
}

fn check_timestamp<I, S>(splitted: I) -> Result<DateTime<Utc>, CursorDecodeErrorKind>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    splitted
        .into_iter()
        .next()
        .and_then(|t| t.as_ref().parse().ok())
        .ok_or(CursorDecodeErrorKind::NoTimestamp)
        .and_then(|t| {
            DateTime::from_timestamp_millis(t).ok_or(CursorDecodeErrorKind::InvalidTimestamp(t))
        })
}

fn check_time<I, S>(splitted: I) -> Result<i32, CursorDecodeErrorKind>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    splitted
        .into_iter()
        .next()
        .and_then(|t| t.as_ref().parse().ok())
        .ok_or(CursorDecodeErrorKind::NoTime)
}

pub struct RecordDateCursor(pub DateTime<Utc>);

impl CursorType for RecordDateCursor {
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        let decoded = decode_base64(s)?;
        let mut splitted = decoded.split(':');
        check_prefix(&mut splitted)?;
        let record_date = check_timestamp(&mut splitted)?;
        Ok(Self(record_date))
    }

    fn encode_cursor(&self) -> String {
        let timestamp = self.0.timestamp_millis();
        encode_base64(format!("record:{}", timestamp))
    }
}

impl IntoValueTuple for &RecordDateCursor {
    fn into_value_tuple(self) -> sea_orm::sea_query::ValueTuple {
        self.0.into_value_tuple()
    }
}

pub struct RecordRankCursor {
    pub record_date: DateTime<Utc>,
    pub time: i32,
}

impl CursorType for RecordRankCursor {
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        let decoded = decode_base64(s)?;
        let mut splitted = decoded.split(':');
        check_prefix(&mut splitted)?;
        let record_date = check_timestamp(&mut splitted)?;
        let time = check_time(&mut splitted)?;
        Ok(Self { record_date, time })
    }

    fn encode_cursor(&self) -> String {
        let timestamp = self.record_date.timestamp_millis();
        encode_base64(format!("record:{timestamp}:{}", self.time))
    }
}

impl IntoValueTuple for &RecordRankCursor {
    fn into_value_tuple(self) -> sea_orm::sea_query::ValueTuple {
        (self.time, self.record_date).into_value_tuple()
    }
}

pub struct TextCursor(pub String);

impl CursorType for TextCursor {
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        decode_base64(s).map(Self)
    }

    fn encode_cursor(&self) -> String {
        encode_base64(self.0.clone())
    }
}

pub struct F64Cursor(pub f64);

impl CursorType for F64Cursor {
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        let decoded = decode_base64(s)?;
        let parsed = decoded
            .parse()
            .map_err(|_| CursorDecodeErrorKind::NoScore)?;
        Ok(Self(parsed))
    }

    fn encode_cursor(&self) -> String {
        encode_base64(self.0.to_string())
    }
}

pub struct ConnectionParameters {
    pub before: Option<ID>,
    pub after: Option<ID>,
    pub first: Option<usize>,
    pub last: Option<usize>,
}

#[cfg(test)]
mod tests {
    use async_graphql::connection::CursorType;
    use base64::{Engine as _, prelude::BASE64_URL_SAFE};
    use chrono::{SubsecRound, Utc};
    use sha2::digest::MacError;

    use crate::{config::InitError, error::CursorDecodeErrorKind};

    use super::{RecordDateCursor, RecordRankCursor};

    fn setup() {
        match crate::init_config() {
            Ok(_) | Err(InitError::ConfigAlreadySet) => (),
            Err(InitError::Config(e)) => {
                panic!("test setup error: {e}");
            }
        }
    }

    #[test]
    fn encode_date_cursor() {
        setup();

        let now = Utc::now();
        let cursor = RecordDateCursor(now).encode_cursor();

        let decoded = BASE64_URL_SAFE
            .decode(&cursor)
            .expect("cursor should be encoded as base64");
        assert!(decoded.starts_with(format!("record:{}$", now.timestamp_millis()).as_bytes()));
    }

    #[test]
    fn decode_date_cursor() {
        setup();

        let now = Utc::now();
        let cursor = RecordDateCursor(now).encode_cursor();

        let decoded = RecordDateCursor::decode_cursor(&cursor);
        assert_eq!(decoded.map(|c| c.0), Ok(now.trunc_subsecs(3)));
    }

    #[test]
    fn decode_date_cursor_errors() {
        setup();

        let decoded = RecordDateCursor::decode_cursor("foobar").err();
        assert_eq!(decoded, Some(CursorDecodeErrorKind::NotBase64));

        let encoded = BASE64_URL_SAFE.encode("foobar");
        let decoded = RecordDateCursor::decode_cursor(&encoded).err();
        assert_eq!(decoded, Some(CursorDecodeErrorKind::NoSignature));

        let encoded = BASE64_URL_SAFE.encode("foobar$signature");
        let decoded = RecordDateCursor::decode_cursor(&encoded).err();
        assert_eq!(
            decoded,
            Some(CursorDecodeErrorKind::InvalidSignature(MacError))
        );
    }

    #[test]
    fn encode_rank_cursor() {
        setup();

        let now = Utc::now();
        let cursor = RecordRankCursor {
            record_date: now,
            time: 1000,
        }
        .encode_cursor();

        let decoded = BASE64_URL_SAFE
            .decode(&cursor)
            .expect("cursor should be encoded as base64");
        assert!(decoded.starts_with(format!("record:{}:1000$", now.timestamp_millis()).as_bytes()));
    }

    #[test]
    fn decode_rank_cursor() {
        setup();

        let now = Utc::now();
        let cursor = RecordRankCursor {
            record_date: now,
            time: 2000,
        }
        .encode_cursor();

        let decoded = RecordRankCursor::decode_cursor(&cursor);
        assert_eq!(
            decoded.map(|c| (c.record_date, c.time)),
            Ok((now.trunc_subsecs(3), 2000))
        );
    }
}
