use core::fmt;

use actix_web::dev::RequestHead;

pub(super) struct FormattedHeaderValue<'a> {
    inner: Result<&'a str, &'a [u8]>,
}

impl<'a> FormattedHeaderValue<'a> {
    pub(super) fn new(val: &'a [u8]) -> Self {
        Self {
            inner: std::str::from_utf8(val).map_err(|_| val),
        }
    }
}

impl fmt::Display for FormattedHeaderValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct List<'a> {
            inner: &'a [u8],
        }

        impl fmt::Display for List<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut list = f.debug_list();
                list.entries(&self.inner[..self.inner.len().min(100)]);
                if self.inner.len() > 100 {
                    list.finish_non_exhaustive()
                } else {
                    list.finish()
                }
            }
        }

        match &self.inner {
            Ok(s) => f.write_str(s),
            Err(b) => write!(f, "Invalid UTF-8: {}", List { inner: b }),
        }
    }
}

pub(super) struct FormattedRequestHead<'a> {
    pub(super) head: &'a RequestHead,
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
            let value = if name == "Authorization" {
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
