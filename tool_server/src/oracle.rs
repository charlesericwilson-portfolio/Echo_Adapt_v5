use anyhow::Result;
use serde_json::{json, Value};
use std::time::Duration;

use crate::config::EndpointConfig;
use crate::providers;

pub async fn oracle(
    prompt: &str,
    config: &EndpointConfig,
) -> Result<String> {

    let messages = vec![
        json!({
            "role": "user",
            "content": prompt
        })
    ];

    let payload = providers::build_payload(
        config,
        &messages,
    )?;

    let client = reqwest::Client::new();

    let mut request = client
        .post(&config.url)
        .timeout(Duration::from_secs(120))
        .json(&payload);

    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(&config.api_key);
    }

    let response = request
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;

    providers::extract_response(config, &response)
}
