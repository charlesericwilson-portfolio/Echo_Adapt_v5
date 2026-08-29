use std::path::PathBuf;
use tokio::process::Command;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde_json::json;
use std::time::Instant;

use crate::summary::summarize_output;
use crate::safety::is_command_safe;
use crate::config::ToolTagsConfig;
use crate::log::save_chat_log_message;

fn tmux_session_name(name: &str) -> String {
    let safe_name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    format!("adapt_{}_{}", std::process::id(), safe_name)
}

pub async fn start_or_reuse_session(
    home_dir: PathBuf,
    active_sessions: &Arc<Mutex<HashMap<String, (String, Instant)>>>,
    name: &str,
    _command: &str,
) -> Result<()> {
    {
        let mut sessions = active_sessions.lock().await;

        sessions
            .entry(name.to_string())
            .and_modify(|(_, last_used)| {
                *last_used = Instant::now();
            })
            .or_insert_with(|| {
                (String::new(), Instant::now())
            });
    }

    let tmux_name = tmux_session_name(name);

    let check = Command::new("tmux")
        .args(["has-session", "-t", &tmux_name])
        .status()
        .await?;

    if !check.success() {
        let mut tmux_command = Command::new("tmux");

        tmux_command
            .args(["new-session", "-d", "-s", &tmux_name])
            .current_dir(&home_dir);

        // If this user has an Adapt-managed Python venv, expose it
        // transparently to the new tmux session.
        let venv_path = home_dir.join(".venv");
        let venv_bin = venv_path.join("bin");

        if venv_bin.join("python").is_file() {
            let current_path = std::env::var("PATH").unwrap_or_default();
            let session_path = format!("{}:{}", venv_bin.display(), current_path);

            tmux_command
                .arg("-e")
                .arg(format!("VIRTUAL_ENV={}", venv_path.display()))
                .arg("-e")
                .arg(format!("PATH={}", session_path));
        }

        let status = tmux_command.status().await?;

        if !status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create tmux session '{}'",
                name
            ));
        }

        println!("Created tmux session: {} -> {}", name, tmux_name);
    } else {
        println!("Reusing existing tmux session: {}", name);
    }

    Ok(())
}

/// Dynamically extracts session command based on configured tags
pub fn extract_session_command(response_text: &str, tags: &ToolTagsConfig) -> Option<(String, String)> {
    if let Some(start) = response_text.find(&tags.session_open) {
        let after = &response_text[start + tags.session_open.len()..];

        if let Some(name_end) = after.find('"') {
            let session_name = after[..name_end].to_string();

            if let Some(tag_close) = response_text[start..].find('>') {
                let content_start = start + tag_close + 1;

                if let Some(end) = response_text[content_start..].find(&tags.session_close) {
                    let command = response_text[content_start..content_start + end]
                        .trim()
                        .to_string();

                    return Some((session_name, command));
                }
            }
        }
    }
    None
}

/// Dynamically extracts end session command based on configured tags
pub fn extract_end_command(response_text: &str, tags: &ToolTagsConfig) -> Option<String> {
    if let Some(start) = response_text.find(&tags.end_session_open) {
        let after = &response_text[start + tags.end_session_open.len()..];

        if let Some(name_end) = after.find('"') {
            let session_name = after[..name_end].to_string();
            return Some(session_name);
        }
    }
    None
}

pub async fn execute_in_session(
    _home_dir: PathBuf,
    _active_sessions: &Arc<Mutex<HashMap<String, (String, std::time::Instant)>>>,
    name: &str,
    command: String,
) -> Result<String> {
    let tmux_name = tmux_session_name(name);

    let timestamp = chrono::Local::now().timestamp();
    let marker_start = format!("===ECHO_START_{}===", timestamp);
    let marker_end = format!("===ECHO_END_{}===", timestamp);

    Command::new("tmux")
        .args(["send-keys", "-t", &tmux_name, &format!("echo '{}'", marker_start), "Enter"])
        .status().await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let chained_command = format!("{}; echo '{}'", command.trim(), marker_end);

    Command::new("tmux")
        .args(["send-keys", "-t", &tmux_name, &chained_command, "Enter"])
        .status().await?;

    println!("{}[Session] Waiting for command to finish...{}", crate::agent::YELLOW, crate::agent::RESET_COLOR);

    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(300);

    loop {
        if start_time.elapsed() > timeout {
            return Err(anyhow::anyhow!("Timeout waiting for markers in session {}", name));
        }

        let output = Command::new("tmux")
            .args(["capture-pane", "-p", "-S", "-", "-t", &tmux_name])
            .output().await?;

        let raw = String::from_utf8_lossy(&output.stdout).to_string();

        if let (Some(start_idx), Some(end_idx)) = (raw.rfind(&marker_start), raw.rfind(&marker_end)) {
            if end_idx > start_idx {
                let captured = raw[start_idx + marker_start.len()..end_idx].trim().to_string();
                if !captured.is_empty() || captured.contains('\n') {
                    return Ok(captured);
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

pub async fn end_session(
    _home_dir: PathBuf,
    active_sessions: &Arc<Mutex<HashMap<String, (String, std::time::Instant)>>>,
    name: &str,
) -> Result<()> {
    let mut sessions = active_sessions.lock().await;
    sessions.remove(name);
    drop(sessions);

    let tmux_name = tmux_session_name(name);

    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &tmux_name])
        .status()
        .await;
    Ok(())
}

pub async fn start_session_cleanup_task(
    active_sessions: Arc<Mutex<HashMap<String, (String, Instant)>>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

        loop {
            interval.tick().await;

            let mut sessions = active_sessions.lock().await;
            let now = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(3600);

            let to_remove: Vec<String> = sessions
                .iter()
                .filter(|(_, (_, last_used))| now.duration_since(*last_used) > timeout)
                .map(|(name, _)| name.clone())
                .collect();

            for name in to_remove {
                let tmux_name = tmux_session_name(&name);

                println!(
                    "Auto-killing inactive tmux session: {} -> {}",
                    name,
                    tmux_name
                );

                let _ = Command::new("tmux")
                    .args(["kill-session", "-t", &tmux_name])
                    .status()
                    .await;

                sessions.remove(&name);
            }
        }
    });
}

/// Intentionally does not terminate tmux sessions on Adapt shutdown.
///
/// Sessions are allowed to survive the owning Adapt process so that a
/// restarted chat can recover session state from the database and reconnect
/// to existing tmux sessions. Inactive sessions are handled separately by
/// the session cleanup task and expire after the configured inactivity period.
pub async fn clean_up_sessions(
    _active_sessions: &Arc<Mutex<HashMap<String, (String, std::time::Instant)>>>
) -> Result<()> {
    Ok(())
}

pub async fn handle_session_command(
    agent: &mut crate::agent::EchoAgent,
    _user_input: &str,
    session_name: &str,
    command: Option<&str>,
) -> Result<()> {
    if let Some(cmd) = command {
        if let Err(e) = is_command_safe(cmd, &agent.config) {
            println!("{}Safety block: {}{}", crate::agent::YELLOW, e, crate::agent::RESET_COLOR);
            agent.messages.push(json!({"role": "assistant", "content": format!("Safety block: {}", e)}));
            return Ok(());
        }

        start_or_reuse_session(agent.home_dir.clone(), &agent.active_sessions, session_name, cmd).await?;

        let raw_output = execute_in_session(
            agent.home_dir.clone(),
            &agent.active_sessions,
            session_name,
            cmd.to_string()
        ).await?;

        let summary = match summarize_output(&raw_output, &agent.config).await {
            Ok(s) => s,
            Err(e) => format!("(Summarizer failed: {})", e),
        };

        agent.db.log_tool_call(session_name, cmd, &summary)?;

        let tool_content = format!(
            "Tool output from SESSION '{}':\nRaw summary: {}",
            session_name, summary
        );

        println!("{}[Session tool executed — Echo will summarize]{}",
                 crate::agent::YELLOW, crate::agent::RESET_COLOR);

        agent.messages.push(json!({
            "role": "assistant",
            "content": format!("Executed command in session '{}'", session_name)
        }));

        agent.messages.push(json!({
            "role": &agent.config.messages.tool_role_name,
            "content": &tool_content
        }));

        save_chat_log_message(
            &agent.home_dir,
            &agent.config.messages.tool_role_name,
            &tool_content,
        ).await?;

    } else {
        println!("{}Echo: Ending session {}{}", crate::agent::YELLOW, session_name, crate::agent::RESET_COLOR);
        let _ = end_session(agent.home_dir.clone(), &agent.active_sessions, session_name).await;
        let tool_content = format!("Session '{}' has been terminated.", session_name);

        agent.messages.push(json!({
            "role": &agent.config.messages.tool_role_name,
            "content": &tool_content
        }));

        save_chat_log_message(
            &agent.home_dir,
            &agent.config.messages.tool_role_name,
            &tool_content,
        ).await?;
    }

    Ok(())
}
