use std::{ops::RangeInclusive, str};

use async_graphql::{ID, connection::CursorType};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};

use crate::error::CursorDecodeErrorKind;

pub const CURSOR_LIMIT_RANGE: RangeInclusive<usize> = 1..=100;
pub const CURSOR_DEFAULT_LIMIT: usize = 50;

fn decode_base64(s: &str) -> Result<String, CursorDecodeErrorKind> {
    let decoded = BASE64
        .decode(s)
        .map_err(|_| CursorDecodeErrorKind::NotBase64)?;

    String::from_utf8(decoded).map_err(|_| CursorDecodeErrorKind::NotUtf8)
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
        BASE64.encode(format!("record:{}", timestamp))
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
        BASE64.encode(format!("record:{timestamp}:{}", self.time))
    }
}

pub struct TextCursor(pub String);

impl CursorType for TextCursor {
    type Error = CursorDecodeErrorKind;

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        decode_base64(s).map(Self)
    }

    fn encode_cursor(&self) -> String {
        BASE64.encode(&self.0)
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
        BASE64.encode(self.0.to_string())
    }
}

pub struct ConnectionParameters {
    pub before: Option<ID>,
    pub after: Option<ID>,
    pub first: Option<usize>,
    pub last: Option<usize>,
}
