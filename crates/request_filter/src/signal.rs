use actix_web::{
    dev::{ConnectionInfo, RequestHead},
    error::InternalError,
    http::StatusCode,
};

use dsc_webhook::{FormattedRequestHead, WebhookBody, WebhookBodyEmbed, WebhookBodyEmbedField};

pub(super) async fn send_notif(
    client: reqwest::Client,
    head: RequestHead,
    connection_info: ConnectionInfo,
) -> Result<(), actix_web::Error> {
    client
        .post(crate::wh_url())
        .json(&WebhookBody {
            content: "Got an invalid request 🥷".to_owned(),
            embeds: vec![
                WebhookBodyEmbed {
                    title: "Request".to_owned(),
                    description: None,
                    color: 5814783,
                    fields: Some(vec![WebhookBodyEmbedField {
                        name: "Head".to_owned(),
                        value: format!("```\n{}\n```", FormattedRequestHead::new(&head)),
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
