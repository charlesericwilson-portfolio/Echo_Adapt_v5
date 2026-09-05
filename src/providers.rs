use serde_json::Value;
use anyhow::{Result, bail};

use crate::config::EndpointConfig;

/// Build the request payload expected by the configured model provider.
///
/// `local` preserves Adapt's existing OpenAI-compatible Chat Completions format.
/// Additional provider-specific formats are handled here so the rest of the
/// agent does not need to know how each backend represents requests.
pub fn build_payload(
    config: &EndpointConfig,
    messages: &[Value],
) -> Result<Value> {
    build_payload_with_settings(
        config,
        messages,
        config.temperature,
        config.max_tokens,
    )
}

pub fn build_payload_with_settings(
    config: &EndpointConfig,
    messages: &[Value],
    temperature: f32,
    max_tokens: u32,
) -> Result<Value> {
    match config.provider.to_lowercase().as_str() {
        "local" => Ok(serde_json::json!({
            "model": &config.model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens
        })),

        "grok" => Ok(serde_json::json!({
            "model": &config.model,
            "input": messages,
            "temperature": temperature
        })),

        provider => bail!("Unsupported model provider: {}", provider),
    }
}

/// Extract assistant text from the provider-specific response format.
///
/// This keeps provider response differences out of the agent loop.
/// The rest of Adapt only receives normalized assistant text.
pub fn extract_response(
    config: &EndpointConfig,
    response: &Value,
) -> Result<String> {
    let text = match config.provider.to_lowercase().as_str() {
        "local" => response
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str),

        "grok" => response
            .get("output")
            .and_then(Value::as_array)
            .and_then(|output| {
                output.iter().find_map(|item| {
                    item.get("content")
                        .and_then(Value::as_array)
                        .and_then(|content| {
                            content.iter().find_map(|part| {
                                part.get("text").and_then(Value::as_str)
                            })
                        })
                })
            })
            .or_else(|| {
                response
                    .get("output_text")
                    .and_then(Value::as_str)
            }),

        provider => {
            anyhow::bail!("Unsupported model provider: {}", provider);
        }
    };

    text
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Model endpoint returned an unexpected response format: {}",
                response
            )
        })
}
