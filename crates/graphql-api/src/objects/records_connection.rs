use async_graphql::SimpleObject;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use crate::objects::ranked_record::RankedRecord;

#[derive(SimpleObject)]
pub struct PageInfo {
    pub has_next_page: bool,
    pub has_previous_page: bool,
    pub start_cursor: Option<String>,
    pub end_cursor: Option<String>,
}

#[derive(SimpleObject)]
pub struct RankedRecordEdge {
    pub node: RankedRecord,
    pub cursor: String,
}

#[derive(SimpleObject)]
pub struct RecordsConnection {
    pub edges: Vec<RankedRecordEdge>,
    pub page_info: PageInfo,
}

/// Encode a cursor from record_date timestamp
pub fn encode_cursor(record_date: &chrono::DateTime<chrono::Utc>) -> String {
    let timestamp = record_date.timestamp_millis();
    BASE64.encode(format!("record:{}", timestamp))
}

/// Decode a cursor to get the record_date timestamp
pub fn decode_cursor(cursor: &str) -> Result<i64, String> {
    let decoded = BASE64
        .decode(cursor)
        .map_err(|_| "Invalid cursor: not valid base64".to_string())?;
    
    let decoded_str = String::from_utf8(decoded)
        .map_err(|_| "Invalid cursor: not valid UTF-8".to_string())?;
    
    if !decoded_str.starts_with("record:") {
        return Err("Invalid cursor: wrong prefix".to_string());
    }
    
    let timestamp_str = &decoded_str[7..];
    timestamp_str
        .parse::<i64>()
        .map_err(|_| "Invalid cursor: not a valid timestamp".to_string())
}

impl RecordsConnection {
    pub fn new(
        records: Vec<RankedRecord>,
        requested_limit: usize,
        has_previous_page: bool,
    ) -> Self {
        let has_next_page = records.len() > requested_limit;
        
        // Trim to requested limit if we fetched one extra to check for next page
        let records = if has_next_page {
            records.into_iter().take(requested_limit).collect()
        } else {
            records
        };
        
        let start_cursor = records
            .first()
            .map(|r| encode_cursor(&r.inner.record.record_date.and_utc()));
        
        let end_cursor = records
            .last()
            .map(|r| encode_cursor(&r.inner.record.record_date.and_utc()));
        
        let edges = records
            .into_iter()
            .map(|record| {
                let cursor = encode_cursor(&record.inner.record.record_date.and_utc());
                RankedRecordEdge {
                    node: record,
                    cursor,
                }
            })
            .collect();
        
        RecordsConnection {
            edges,
            page_info: PageInfo {
                has_next_page,
                has_previous_page,
                start_cursor,
                end_cursor,
            },
        }
    }
}
