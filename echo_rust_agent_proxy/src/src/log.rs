use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct LogEntry {
    messages: Vec<Message>,
}

pub async fn save_chat_log_entry(
    log_dir: &PathBuf,
    user_message: &str,
    assistant_response: &str,
    from: &str,
) -> Result<()> {
    tokio::fs::create_dir_all(log_dir).await?;

    let file_path = log_dir.join("echo_chat.jsonl");

    let mut messages = Vec::new();

    if !user_message.is_empty() {
        messages.push(Message {
            role: "user".to_string(),
            content: user_message.trim().to_string(),
        });
    }

    if !assistant_response.is_empty() {
        let content = if from.contains("SESSION_START") {
            "=== SESSION START ===".to_string()
        } else if from.contains("SESSION_END") {
            "=== SESSION END ===".to_string()
        } else if !from.is_empty() && from != "main" && from != "assistant" && from != "user" {
            format!("Session: {}", from)
        } else {
            assistant_response.trim().to_string()
        };

        messages.push(Message {
            role: "assistant".to_string(),
            content,
        });
    }

    let log_entry = LogEntry { messages };

    let log_line = serde_json::to_string(&log_entry)
        .map_err(|e| anyhow::anyhow!("Failed to serialize log: {}", e))?;

    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&file_path)
        .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", file_path.display(), e))?;

    writeln!(file, "{}", log_line)
        .map_err(|e| anyhow::anyhow!("Failed to write log: {}", e))?;

    Ok(())
}
