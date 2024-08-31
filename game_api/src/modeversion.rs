use std::{
    fmt,
    future::{ready, Ready},
};

use actix_web::{dev::Payload, FromRequest, HttpRequest, HttpResponse, ResponseError};

const OBS_MODE_VERS_HEADER: &str = "ObstacleModeVersion";

#[derive(thiserror::Error, Debug)]
pub enum ModeVersionExtractErr {
    #[error("invalid `{OBS_MODE_VERS_HEADER} header: {0}")]
    ParseErr(records_lib::ModeVersionParseErr),
    #[error("missing `{OBS_MODE_VERS_HEADER}` header")]
    MissingHeader,
    #[error("invalid `{OBS_MODE_VERS_HEADER}` header encoding: {0}")]
    InvalidHeaderEncoding(actix_web::http::header::ToStrError),
}

impl ResponseError for ModeVersionExtractErr {
    #[inline]
    fn error_response(&self) -> HttpResponse {
        HttpResponse::BadRequest().body(self.to_string())
    }
}

pub struct ModeVersion(pub records_lib::ModeVersion);

impl fmt::Display for ModeVersion {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromRequest for ModeVersion {
    type Error = ModeVersionExtractErr;

    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let header = req
            .headers()
            .get(OBS_MODE_VERS_HEADER)
            .ok_or(ModeVersionExtractErr::MissingHeader)
            .and_then(|h| {
                h.to_str()
                    .map_err(ModeVersionExtractErr::InvalidHeaderEncoding)
                    .and_then(|s| {
                        s.parse()
                            .map(ModeVersion)
                            .map_err(ModeVersionExtractErr::ParseErr)
                    })
            });

        ready(header)
    }
}
