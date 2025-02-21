//! Module which provides some functions to check if a request is valid or not, and to notify such case.

use actix_web::{
    dev::{ConnectionInfo, RequestHead, ServiceRequest},
    error::InternalError,
    http::{header, StatusCode},
};

use crate::discord_webhook::{WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};

fn is_known_agent(value: &[u8]) -> bool {
    let Ok((_, parsed)) = super::parse_agent(value) else {
        return false;
    };

    match parsed.client {
        b"Win64" | b"Win32" | b"Linux" => (),
        _ => return false,
    }

    match parsed.maniaplanet_version {
        // Please let me know when Nadeo releases a new version of ManiaPlanet... :(
        (..3, ..) | (3, ..3, _) | (3, 3, 0) => (),
        _ => return false,
    }

    match parsed.rv {
        (..2019, ..)
        | (2019, ..11, ..)
        | (2019, 11, ..19, ..)
        | (2019, 11, 19, ..18, _)
        | (2019, 11, 19, 18, ..=50) => (),
        _ => return false,
    }

    match parsed.context {
        b"none" => (),
        _ => return false,
    }

    true
}

pub(crate) async fn flag_invalid_req(
    client: reqwest::Client,
    head: RequestHead,
    connection_info: ConnectionInfo,
) -> Result<(), actix_web::Error> {
    let fields = vec![
        WebhookBodyEmbedField {
            name: "Request head".to_owned(),
            value: format!("```{}```", super::FormattedRequestHead { head: &head }),
            inline: None,
        },
        WebhookBodyEmbedField {
            name: "Connection info".to_owned(),
            value: format!(
                "```{}```",
                super::FormattedConnectionInfo {
                    connection_info: &connection_info
                }
            ),
            inline: None,
        },
    ];

    client
        .post(&crate::env().wh_invalid_req_url)
        .json(&WebhookBody {
            content: "Got an invalid request 🥷".to_owned(),
            embeds: vec![WebhookBodyEmbed {
                title: "Info".to_owned(),
                description: None,
                color: 5814783,
                fields: Some(fields),
                url: None,
            }],
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

pub(crate) fn is_request_valid(req: &ServiceRequest) -> bool {
    req.headers()
        .get(header::USER_AGENT)
        .filter(|value| is_known_agent(value.as_bytes()))
        .is_some()
}
