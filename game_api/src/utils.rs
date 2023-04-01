use std::io::Write;

use actix_web::{HttpResponse, Responder};
use serde::Serialize;
use serde_xml_rs::{to_writer, Serializer};

use crate::RecordsResult;

const XML_BUF_CAP: usize = 16 * 1024;

pub fn xml_seq<T: Serialize>(field_name: Option<&str>, s: &[T]) -> RecordsResult<String> {
    let mut buf = Vec::with_capacity(XML_BUF_CAP);
    if let Some(field_name) = field_name {
        write!(buf, "<{}>", field_name)?;
    }
    let mut ser = Serializer::new(&mut buf);
    for elem in s {
        elem.serialize(&mut ser)?;
    }
    if let Some(field_name) = field_name {
        write!(buf, "</{}>", field_name)?;
    }
    Ok(String::from_utf8(buf).unwrap())
}

pub fn wrap_xml<T: Serialize>(s: &T) -> RecordsResult<impl Responder> {
    let mut buf = Vec::with_capacity(XML_BUF_CAP);
    buf.write(br#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    to_writer(&mut buf, s)?;
    let out = String::from_utf8(buf).expect("xml serializing produced non utf8 chars");
    Ok(HttpResponse::Ok()
        .content_type("application/xml; charset=utf-8")
        .body(out))
}

pub fn escaped(input: &str) -> String {
    let mut out = input.to_owned();

    let pile_o_bits = input;
    let mut last = 0;
    for (i, ch) in input.bytes().enumerate() {
        match ch as char {
            '<' | '>' | '&' | '\'' | '"' => {
                out.push_str(&pile_o_bits[last..i]);
                let s = match ch as char {
                    // Because the internet is always right, turns out there's not that many
                    // characters to escape: http://stackoverflow.com/questions/7381974
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
            _ => {}
        }
    }

    if last < input.len() {
        out.push_str(&pile_o_bits[last..]);
    }

    out
}

pub fn any_repeated<T: PartialEq>(slice: &[T]) -> bool {
    for (i, t) in slice.iter().enumerate() {
        if slice.split_at(i + 1).1.iter().any(|x| x == t) {
            return true;
        }
    }
    false
}
