use std::{
    fmt,
    str::{self, FromStr},
};

use async_graphql::{ID, connection::CursorType};
use base64::{Engine as _, prelude::BASE64_URL_SAFE};
use chrono::{DateTime, Utc};
use hmac::Mac;
use mkenv::Layer;
use sea_orm::{
    Value,
    sea_query::{IntoValueTuple, ValueTuple},
};

use crate::error::CursorDecodeErrorKind;

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

fn check_prefix<I, S>(prefix: &str, splitted: I) -> Result<(), CursorDecodeErrorKind>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    match splitted.into_iter().next().filter(|s| s.as_ref() == prefix) {
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

fn check_finished<I>(splitted: I) -> Result<(), CursorDecodeErrorKind>
where
    I: IntoIterator,
{
    match splitted.into_iter().next() {
        Some(_) => Err(CursorDecodeErrorKind::TooLong),
        None => Ok(()),
    }
}

fn check_data<I, S, T>(splitted: I) -> Result<T, CursorDecodeErrorKind>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    T: FromStr,
{
    splitted
        .into_iter()
        .next()
        .and_then(|t| t.as_ref().replace("\\:", ":").parse().ok())
        .ok_or(CursorDecodeErrorKind::MissingData)
}

#[derive(PartialEq, Debug)]
pub struct RecordDateCursor<T = u32>(pub DateTime<Utc>, pub T);

impl<T> CursorType for RecordDateCursor<T>
where
    T: FromStr + fmt::Display,
{
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        let decoded = decode_base64(s)?;
        let mut splitted = decoded.split(':');
        check_prefix("record_date", &mut splitted)?;
        let record_date = check_timestamp(&mut splitted)?;
        let data = check_data(&mut splitted)?;
        check_finished(&mut splitted)?;
        Ok(Self(record_date, data))
    }

    fn encode_cursor(&self) -> String {
        let timestamp = self.0.timestamp_millis();
        let escaped_data = self.1.to_string().replace(':', "\\:");
        encode_base64(format!("record_date:{}:{escaped_data}", timestamp))
    }
}

impl<T> IntoValueTuple for &RecordDateCursor<T>
where
    T: Into<Value> + Clone,
{
    fn into_value_tuple(self) -> ValueTuple {
        (self.0, self.1.clone()).into_value_tuple()
    }
}

#[derive(Debug, PartialEq)]
pub struct RecordRankCursor<T = u32> {
    pub record_date: DateTime<Utc>,
    pub time: i32,
    pub data: T,
}

impl<T> CursorType for RecordRankCursor<T>
where
    T: FromStr + fmt::Display,
{
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        let decoded = decode_base64(s)?;
        let mut splitted = decoded.split(':');
        check_prefix("record_rank", &mut splitted)?;
        let record_date = check_timestamp(&mut splitted)?;
        let time = check_time(&mut splitted)?;
        let data = check_data(&mut splitted)?;
        check_finished(&mut splitted)?;
        Ok(Self {
            record_date,
            time,
            data,
        })
    }

    fn encode_cursor(&self) -> String {
        let timestamp = self.record_date.timestamp_millis();
        let escaped_data = self.data.to_string().replace(':', "\\:");
        encode_base64(format!(
            "record_rank:{timestamp}:{}:{escaped_data}",
            self.time,
        ))
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

pub struct TextCursor<T = u32>(pub String, pub T);

impl<T> CursorType for TextCursor<T>
where
    T: FromStr + fmt::Display,
{
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        let decoded = decode_base64(s)?;
        let mut splitted = decoded.split(':');
        check_prefix("text", &mut splitted)?;
        let text = splitted
            .next()
            .ok_or(CursorDecodeErrorKind::MissingText)?
            .replace("\\:", ":");
        let data = check_data(&mut splitted)?;
        check_finished(&mut splitted)?;
        Ok(Self(text.to_owned(), data))
    }

    fn encode_cursor(&self) -> String {
        let escaped_txt = self.0.replace(':', "\\:");
        let escaped_data = self.1.to_string().replace(':', "\\:");
        encode_base64(format!("text:{escaped_txt}:{escaped_data}"))
    }
}

impl<T> IntoValueTuple for &TextCursor<T>
where
    T: Into<Value> + Clone,
{
    fn into_value_tuple(self) -> ValueTuple {
        (self.0.clone(), self.1.clone()).into_value_tuple()
    }
}

pub struct F64Cursor<T = u32>(pub f64, pub T);

impl<T> CursorType for F64Cursor<T>
where
    T: FromStr + fmt::Display,
{
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        let decoded = decode_base64(s)?;
        let mut splitted = decoded.split(':');
        check_prefix("score", &mut splitted)?;
        let parsed = splitted
            .next()
            .ok_or(CursorDecodeErrorKind::MissingScore)?
            .parse()
            .map_err(|_| CursorDecodeErrorKind::NoScore)?;
        let data = check_data(&mut splitted)?;
        check_finished(&mut splitted)?;
        Ok(Self(parsed, data))
    }

    fn encode_cursor(&self) -> String {
        let escaped_data = self.1.to_string().replace(':', "\\:");
        encode_base64(format!("score:{}:{escaped_data}", self.0))
    }
}

impl<T> IntoValueTuple for &F64Cursor<T>
where
    T: Into<Value> + Clone,
{
    fn into_value_tuple(self) -> ValueTuple {
        (self.0, self.1.clone()).into_value_tuple()
    }
}

#[derive(Default)]
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
        let cursor = RecordDateCursor(now, 0).encode_cursor();

        let decoded = BASE64_URL_SAFE
            .decode(&cursor)
            .expect("cursor should be encoded as base64");
        assert!(
            decoded.starts_with(format!("record_date:{}:0$", now.timestamp_millis()).as_bytes())
        );
    }

    #[test]
    fn decode_date_cursor() {
        setup();

        let now = Utc::now();
        let cursor = RecordDateCursor(now, 0).encode_cursor();

        let decoded = RecordDateCursor::decode_cursor(&cursor);
        assert_eq!(decoded, Ok(RecordDateCursor(now.trunc_subsecs(3), 0)));
    }

    #[test]
    fn decode_date_cursor_errors() {
        setup();

        let decoded = <RecordDateCursor>::decode_cursor("foobar").err();
        assert_eq!(decoded, Some(CursorDecodeErrorKind::NotBase64));

        let encoded = BASE64_URL_SAFE.encode("foobar");
        let decoded = <RecordDateCursor>::decode_cursor(&encoded).err();
        assert_eq!(decoded, Some(CursorDecodeErrorKind::NoSignature));

        let encoded = BASE64_URL_SAFE.encode("foobar$signature");
        let decoded = <RecordDateCursor>::decode_cursor(&encoded).err();
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
            data: 24,
        }
        .encode_cursor();

        let decoded = BASE64_URL_SAFE
            .decode(&cursor)
            .expect("cursor should be encoded as base64");
        assert!(
            decoded
                .starts_with(format!("record_rank:{}:1000:24$", now.timestamp_millis()).as_bytes())
        );
    }

    #[test]
    fn decode_rank_cursor() {
        setup();

        let now = Utc::now();
        let cursor = RecordRankCursor {
            record_date: now,
            time: 2000,
            data: 34,
        }
        .encode_cursor();

        let decoded = RecordRankCursor::decode_cursor(&cursor);
        assert_eq!(
            decoded,
            Ok(RecordRankCursor {
                record_date: now.trunc_subsecs(3),
                time: 2000,
                data: 34
            })
        );
    }
}
