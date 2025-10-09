use async_graphql::ID;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use crate::error::CursorDecodeErrorKind;

/// Encode a cursor from record_date timestamp
pub(crate) fn encode_cursor(record_date: &chrono::DateTime<chrono::Utc>) -> String {
    let timestamp = record_date.timestamp_millis();
    BASE64.encode(format!("record:{}", timestamp))
}

/// Decode a cursor to get the record_date timestamp
pub(crate) fn decode_cursor(cursor: &str) -> Result<i64, CursorDecodeErrorKind> {
    let decoded = BASE64
        .decode(cursor)
        .map_err(|_| CursorDecodeErrorKind::NotBase64)?;

    let decoded_str = String::from_utf8(decoded).map_err(|_| CursorDecodeErrorKind::NotUtf8)?;

    if !decoded_str.starts_with("record:") {
        return Err(CursorDecodeErrorKind::WrongPrefix);
    }

    let timestamp_str = &decoded_str[7..];
    timestamp_str
        .parse::<i64>()
        .map_err(|_| CursorDecodeErrorKind::NotTimestamp)
}

pub struct ConnectionParameters {
    pub before: Option<ID>,
    pub after: Option<ID>,
    pub first: Option<usize>,
    pub last: Option<usize>,
}
