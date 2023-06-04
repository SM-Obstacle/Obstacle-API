use actix_web::{HttpResponse, Responder};
use serde::Serialize;

pub fn json<T: Serialize, E>(obj: T) -> Result<impl Responder, E> {
    Ok(HttpResponse::Ok().json(obj))
}

pub fn escaped(input: &str) -> String {
    let mut out = String::new();

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

pub fn format_map_key(map_id: u32) -> String {
    format!("mlb:{map_id}")
}

pub fn format_token_key(login: &str) -> String {
    format!("pt:{login}")
}
