use core::fmt;

use actix_web::{dev::RequestHead, http::header};

pub struct FormattedRequestHead<'a> {
    head: &'a RequestHead,
}

impl<'a> FormattedRequestHead<'a> {
    pub fn new(head: &'a actix_web::dev::RequestHead) -> Self {
        Self { head }
    }
}

struct FormattedHeaderValue<'a> {
    inner: Result<&'a str, &'a [u8]>,
}

impl<'a> FormattedHeaderValue<'a> {
    fn new(val: &'a [u8]) -> Self {
        Self {
            inner: std::str::from_utf8(val).map_err(|_| val),
        }
    }
}

impl fmt::Display for FormattedHeaderValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            Ok(s) => f.write_str(s),
            Err(b) => {
                let fmt_list = fmt::from_fn(|f| {
                    let mut list = f.debug_list();
                    list.entries(&b[..b.len().min(100)]);
                    if b.len() > 100 {
                        list.finish_non_exhaustive()
                    } else {
                        list.finish()
                    }
                });
                write!(f, "Invalid UTF-8: {}", fmt_list)
            }
        }
    }
}

impl fmt::Display for FormattedRequestHead<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{method} {uri} {version:?}",
            method = self.head.method,
            uri = self.head.uri,
            version = self.head.version,
        )?;

        for (name, value) in self.head.headers.iter() {
            let value =
                if [header::AUTHORIZATION, header::COOKIE, header::SET_COOKIE].contains(name) {
                    b"***"
                } else {
                    value.as_bytes()
                };
            let value = FormattedHeaderValue::new(value);

            writeln!(f, "{name}: {value}")?;
        }

        Ok(())
    }
}
