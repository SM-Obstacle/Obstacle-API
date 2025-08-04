use serde::Serialize;

#[derive(Serialize)]
pub struct WebhookBodyEmbedField {
    pub name: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline: Option<bool>,
}

#[derive(Serialize)]
pub struct WebhookBodyEmbed {
    pub title: String,
    pub description: Option<String>,
    pub color: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub fields: Option<Vec<WebhookBodyEmbedField>>,
}

#[derive(Serialize)]
pub struct WebhookBody {
    pub content: String,
    pub embeds: Vec<WebhookBodyEmbed>,
}
