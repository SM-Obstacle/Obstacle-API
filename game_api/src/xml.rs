pub mod reply {

    use serde::{ser::StdError, Serialize};
    use serde_xml_rs::ser::Serializer;
    use std::fmt;
    use std::io::prelude::*;
    use warp::{
        http::{header, HeaderValue, StatusCode},
        reply::{Reply, Response},
    };

    pub fn xml<T>(val: &T) -> Xml
    where
        T: Serialize,
    {
        // 16KiB buffer should be enough
        let mut buffer = Vec::with_capacity(16 * 1024);
        let mut valid = true;

        if let Err(e) = buffer.write(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>") {
            tracing::error!("reply::xml error: {}", e);
            valid = false;
        }
        let mut ser = Serializer::new(&mut buffer);
        if let Err(e) = val.serialize(&mut ser) {
            tracing::error!("reply::xml error: {}", e);
            valid = false
        }

        Xml {
            inner: if valid { Ok(buffer) } else { Err(()) },
        }
    }

    pub fn xml_elements<T>(val: &[T]) -> Xml
    where
        T: Serialize,
    {
        // 16KiB buffer should be enough
        let mut buffer = Vec::with_capacity(16 * 1024);
        let mut valid = true;

        if let Err(e) = buffer.write(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>") {
            tracing::error!("reply::xml error: {}", e);
            valid = false;
        }
        if let Err(e) = buffer.write(b"<response>") {
            tracing::error!("reply::xml error: {}", e);
            valid = false;
        }
        let mut ser = Serializer::new(&mut buffer);
        for element in val {
            if let Err(e) = element.serialize(&mut ser) {
                tracing::error!("reply::xml error: {}", e);
                valid = false;
                break;
            }
        }
        if let Err(e) = buffer.write(b"</response>") {
            tracing::error!("reply::xml error: {}", e);
            valid = false;
        }

        Xml {
            inner: if valid { Ok(buffer) } else { Err(()) },
        }
    }

    /// A XML formatted reply.
    #[allow(missing_debug_implementations)]
    pub struct Xml {
        inner: Result<Vec<u8>, ()>,
    }

    impl Reply for Xml {
        #[inline]
        fn into_response(self) -> Response {
            match self.inner {
                Ok(body) => {
                    let mut res = Response::new(body.into());
                    res.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/xml; charset=utf-8"),
                    );
                    res
                }
                Err(()) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
    }

    #[derive(Debug)]
    pub(crate) struct ReplyXmlError;

    impl fmt::Display for ReplyXmlError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("warp::reply::xml() failed")
        }
    }

    impl StdError for ReplyXmlError {}
}
