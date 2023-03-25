use std::io::Write;

use actix_web::{HttpResponse, Responder};
use serde::Serialize;
use serde_xml_rs::{to_string, Serializer};

use crate::RecordsResult;

pub fn wrap_xml_seq<T: Serialize>(s: &[T]) -> RecordsResult<impl Responder> {
    let mut buf = Vec::with_capacity(16 * 1024);
    buf.write(br#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    buf.write(b"<response>")?;
    let mut ser = Serializer::new(&mut buf);
    for elem in s {
        elem.serialize(&mut ser)?;
    }
    buf.write(b"</response>")?;
    Ok(HttpResponse::Ok()
        .content_type("application/xml; charset=utf-8")
        .body(String::from_utf8(buf).unwrap()))
}

pub fn wrap_xml<T: Serialize>(s: &T) -> RecordsResult<impl Responder> {
    let out = to_string(s)?;
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
