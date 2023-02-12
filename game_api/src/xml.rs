pub mod reply {

    use actix_web::{
        body::{BodySize, MessageBody},
        web,
    };
    use serde::{ser::StdError, Serialize};
    use serde_xml_rs::ser::Serializer;
    use std::io::prelude::*;
    use std::{
        fmt,
        pin::Pin,
        task::{Context, Poll},
    };
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

    impl MessageBody for Xml {
        type Error = String;

        fn size(&self) -> BodySize {
            match self.inner {
                Ok(ref body) => BodySize::Sized(body.len() as u64),
                Err(_) => BodySize::Sized(0),
            }
        }

        fn poll_next(
            mut self: Pin<&mut Self>, cx: &mut Context<'_>,
        ) -> Poll<Option<Result<web::Bytes, Self::Error>>> {
            if let Ok(ref mut body) = self.inner {
                <Vec<u8> as MessageBody>::poll_next(Pin::new(body), cx).map_err(|_| "".to_owned())
            } else {
                Poll::Ready(Some(Err("error generating XML".to_owned())))
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
