use actix_web::{
    dev::{ConnectionInfo, RequestHead},
    error::InternalError,
    http::StatusCode,
};
use core::fmt;

use crate::discord_webhook::{WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
struct FormattedHeaderValue<'a> {
    inner: Result<&'a str, &'a [u8]>,
}

impl<'a> FormattedHeaderValue<'a> {
    #[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
    fn new(val: &'a [u8]) -> Self {
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

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
struct FormattedRequestHead<'a> {
    head: &'a RequestHead,
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

#[cfg_attr(not(feature = "request_filter"), allow(dead_code))]
pub(super) async fn send_notif(
    client: reqwest::Client,
    head: RequestHead,
    connection_info: ConnectionInfo,
) -> Result<(), actix_web::Error> {
    client
        .post(&crate::env().wh_invalid_req_url)
        .json(&WebhookBody {
            content: "Got an invalid request 🥷".to_owned(),
            embeds: vec![
                WebhookBodyEmbed {
                    title: "Request".to_owned(),
                    description: None,
                    color: 5814783,
                    fields: Some(vec![WebhookBodyEmbedField {
                        name: "Head".to_owned(),
                        value: format!("```{}```", FormattedRequestHead { head: &head }),
                        inline: None,
                    }]),
                    url: None,
                },
                WebhookBodyEmbed {
                    title: "Connection info".to_owned(),
                    description: None,
                    color: 5814783,
                    fields: Some(vec![
                        WebhookBodyEmbedField {
                            name: "Host".to_owned(),
                            value: connection_info.host().to_owned(),
                            inline: None,
                        },
                        WebhookBodyEmbedField {
                            name: "Peer address".to_owned(),
                            value: connection_info.peer_addr().unwrap_or("Unknown").to_owned(),
                            inline: None,
                        },
                        WebhookBodyEmbedField {
                            name: "Real IP remote address".to_owned(),
                            value: connection_info
                                .realip_remote_addr()
                                .unwrap_or("Unknown")
                                .to_owned(),
                            inline: None,
                        },
                        WebhookBodyEmbedField {
                            name: "Scheme".to_owned(),
                            value: connection_info.scheme().to_owned(),
                            inline: None,
                        },
                    ]),
                    url: None,
                },
            ],
        })
        .send()
        .await
        .map_err(|e| {
            actix_web::Error::from(InternalError::new(
                format!("Unknown error: {e}"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        })?;

    Ok(())
}
