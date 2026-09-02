use serde_json::{Value, json};
use anyhow::Result;
use crate::config::Config;

pub async fn summarize_output(raw_output: &str, config: &Config) -> Result<String> {
    if !config.summarizer.enabled {
        println!("{}Echo: [SUMMARIZER] Disabled in config — skipping{}",
                 crate::agent::YELLOW, crate::agent::RESET_COLOR);
        return Ok(raw_output.to_string());   // return original
    }

    if raw_output.chars().count() <= config.summarizer.max_raw_output_chars {
        return Ok(raw_output.to_string());
    }

    println!("{}Echo: [SUMMARIZER] Summarizing tool output...{}",
             crate::agent::YELLOW, crate::agent::RESET_COLOR);
    let tool_summarizer_prompt =
    match tokio::fs::read_to_string(&config.prompts.summarizer).await {
        Ok(prompt) => prompt,
        Err(e) => {
            eprintln!(
                "{}Echo: [SUMMARIZER ERROR] Failed to read prompt '{}': {}. Using original tool output.{}",
                crate::agent::YELLOW,
                config.prompts.summarizer,
                e,
                crate::agent::RESET_COLOR
            );

            return Ok(raw_output.to_string());
        }
    };

    let payload = json!({
        "model": &config.summarizer.model,
        "messages": [
            {
                "role": "system",
                "content": tool_summarizer_prompt
            },
            {
                "role": "user",
                "content": raw_output
            }
        ],
        "temperature": 0.2,
        "max_tokens": 1500
    });

    let response = match reqwest::Client::new()
        .post(&config.summarizer.url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(response) => response,

        Err(e) => {
            eprintln!(
                "{}Echo: [SUMMARIZER ERROR] Failed to contact summarizer endpoint: {}. Using original tool output.{}",
                crate::agent::YELLOW,
                e,
                crate::agent::RESET_COLOR
            );

            return Ok(raw_output.to_string());
        }
    };

    let response = match response.error_for_status() {
        Ok(response) => response,

        Err(e) => {
            eprintln!(
                "{}Echo: [SUMMARIZER ERROR] Summarizer endpoint returned an error: {}. Using original tool output.{}",
                crate::agent::YELLOW,
                e,
                crate::agent::RESET_COLOR
            );

            return Ok(raw_output.to_string());
        }
    };

    let parsed: Value = match response.json().await {
        Ok(parsed) => parsed,

        Err(e) => {
            eprintln!(
                "{}Echo: [SUMMARIZER ERROR] Invalid response from summarizer: {}. Using original tool output.{}",
                crate::agent::YELLOW,
                e,
                crate::agent::RESET_COLOR
            );

            return Ok(raw_output.to_string());
        }
    };

    let summary = match parsed
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        Some(summary) if !summary.trim().is_empty() => summary.trim(),

        _ => {
            eprintln!(
                "{}Echo: [SUMMARIZER ERROR] Summarizer returned no usable content. Using original tool output.{}",
                crate::agent::YELLOW,
                crate::agent::RESET_COLOR
            );

            return Ok(raw_output.to_string());
        }
    };

    Ok(summary.to_string())
}

pub async fn summarize_context(messages: &mut Vec<Value>, config: &Config) -> Result<()> {
    if messages.is_empty() {
        return Ok(());
    }

    let summary_prompt = "Summarize the entire conversation so far in a concise way. Keep key facts, decisions, and important details. Output ONLY the summary, nothing else.";

    // Build new message list with the summary instruction
    let mut summary_messages = vec![
        json!({
            "role": "system",
            "content": summary_prompt
        })
    ];

    // Add the recent conversation history (skip the original system prompt)
    summary_messages.extend(messages.iter().skip(1).cloned());

    let payload = json!({
        "model": &config.endpoint.model,
        "messages": summary_messages,
        "temperature": 0.3,
        "max_tokens": 1024
    });

    // Call the model
    let response = match reqwest::Client::new()
        .post(&config.endpoint.url)
        .json(&payload)
        .send()
        .await
    {
        Ok(response) => response,

        Err(e) => {
            eprintln!(
                "{}Echo: [CONTEXT SUMMARY ERROR] Failed to contact model endpoint: {}. Keeping existing context.{}",
                crate::agent::YELLOW,
                e,
                crate::agent::RESET_COLOR
            );

            return Ok(());
        }
    };

    let response = match response.error_for_status() {
        Ok(response) => response,

        Err(e) => {
            eprintln!(
                "{}Echo: [CONTEXT SUMMARY ERROR] Model endpoint returned an error: {}. Keeping existing context.{}",
                crate::agent::YELLOW,
                e,
                crate::agent::RESET_COLOR
            );

            return Ok(());
        }
    };

    let response_json: Value = match response.json().await {
        Ok(json) => json,

        Err(e) => {
            eprintln!(
                "{}Echo: [CONTEXT SUMMARY ERROR] Invalid response from model endpoint: {}. Keeping existing context.{}",
                crate::agent::YELLOW,
                e,
                crate::agent::RESET_COLOR
            );

            return Ok(());
        }
    };

    let summary_text = match response_json
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        Some(summary) if !summary.trim().is_empty() => summary.trim().to_string(),

        _ => {
            eprintln!(
                "{}Echo: [CONTEXT SUMMARY ERROR] Model returned no usable summary. Keeping existing context.{}",
                crate::agent::YELLOW,
                crate::agent::RESET_COLOR
            );

            return Ok(());
        }
    };

    // === FIX: Preserve the original system prompt (messages[0]) ===
    let system_prompt = messages[0].clone();
    let last_turns: Vec<Value> = messages.iter().rev().take(4).cloned().collect();

    let mut new_messages = vec![system_prompt];
    new_messages.push(json!({
        "role": "system",
        "content": format!("Previous conversation summary:\n{}", summary_text)
    }));
    new_messages.extend(last_turns.into_iter().rev());

    *messages = new_messages;
    Ok(())
}
