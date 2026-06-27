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

    // starts or reuses and existing session
pub async fn start_or_reuse_session(
    home_dir: PathBuf,
    active_sessions: &Arc<Mutex<HashMap<String, (String, Instant)>>>,
    name: &str,
    command: &str,
) -> Result<()> {
    let mut sessions = active_sessions.lock().await;
    sessions.insert(name.to_string(), (String::new(), Instant::now()));
    drop(sessions);

    // Check if session exists
    let check = Command::new("tmux")
        .args(["has-session", "-t", name])
        .status().await?;

    if !check.success() {
        // Create new detached session
        Command::new("tmux")
            .args(["new-session", "-d", "-s", name])
            .current_dir(&home_dir)
            .status().await?;
        println!("Created new tmux session: {}", name);
    } else {
        println!("Reusing existing tmux session: {}", name);
    }

    // Send the command
    Command::new("tmux")
        .args(["send-keys", "-t", name, command, "Enter"])
        .status().await?;

    Ok(())
}
    // Extraction logic
pub fn extract_session_command(response_text: &str) -> Option<(String, String)> {
    // <session name="foo">command here</session>
    if let Some(start) = response_text.find("<session name=\"") {
        let after = &response_text[start + 15..]; // skip past <session name="

        if let Some(name_end) = after.find('"') {
            let session_name = after[..name_end].to_string();

            if let Some(tag_close) = response_text[start..].find('>') {
                let content_start = start + tag_close + 1;

                if let Some(end) = response_text[content_start..].find("</session>") {
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
    // Extract end command
    /// Extracts an end session command in the format:
/// <end_session name="session_name"/>
pub fn extract_end_command(response_text: &str) -> Option<String> {
    if let Some(start) = response_text.find("<end_session name=\"") {
        let after = &response_text[start + 19..]; // length of `<end_session name="`

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
    let timestamp = chrono::Local::now().timestamp();
    let marker_start = format!("===ECHO_START_{}===", timestamp);
    let marker_end = format!("===ECHO_END_{}===", timestamp);

    // Send start marker
    Command::new("tmux")
        .args(["send-keys", "-t", name, &format!("echo '{}'", marker_start), "Enter"])
        .status().await?;

    // Send the actual command
    Command::new("tmux")
        .args(["send-keys", "-t", name, &command, "Enter"])
        .status().await?;

    // Send end marker
    Command::new("tmux")
        .args(["send-keys", "-t", name, &format!("echo '{}'", marker_end), "Enter"])
        .status().await?;

    // Active polling loop (~300ms)
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(60);

    loop {
        if start_time.elapsed() > timeout {
            return Err(anyhow::anyhow!("Timeout waiting for command output in session '{}'", name));
        }

        // Capture current pane output
        let output = Command::new("tmux")
            .args(["capture-pane", "-p", "-S", "-10000", "-t", name])
            .output().await?;

        let raw = String::from_utf8_lossy(&output.stdout).to_string();

        // Check if we have both markers and end is after start
        if let (Some(start_idx), Some(end_idx)) = (raw.rfind(&marker_start), raw.rfind(&marker_end)) {
            if end_idx > start_idx {
                let clean_output = raw[start_idx + marker_start.len()..end_idx]
                    .trim()
                    .to_string();
                return Ok(clean_output);
            }
        }

        // Poll again in ~300ms
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
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

    let _ = Command::new("tmux").args(["kill-session", "-t", name]).status().await;
    Ok(())
}

pub async fn start_session_cleanup_task(
    active_sessions: Arc<Mutex<HashMap<String, (String, Instant)>>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60)); // check every minute

        loop {
            interval.tick().await;

            let mut sessions = active_sessions.lock().await;
            let now = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(3600); // 1 hour

            let to_remove: Vec<String> = sessions
                .iter()
                .filter(|(_, (_, last_used))| now.duration_since(*last_used) > timeout)
                .map(|(name, _)| name.clone())
                .collect();

            for name in to_remove {
                println!("Auto-killing inactive tmux session: {}", name);
                let _ = Command::new("tmux").args(["kill-session", "-t", &name]).status();
                sessions.remove(&name);
            }
        }
    });
}
/// Intentionally a no-op by design.
///
/// Tmux sessions are kept alive after the chat ends so that:
/// - Active shells, listeners, or tools persist across crashes/restarts
/// - The agent can resume a previous engagement by reviewing the tool
///   database (echo_tools.db) and reconnecting to existing sessions
///
/// Sessions are auto-reaped by the background cleanup task after 1 hour
/// of inactivity (see `start_session_cleanup_task`).
///
/// To kill all sessions on exit instead, iterate `active_sessions` here
/// and call `tmux kill-session -t <name>` for each.
pub async fn clean_up_sessions(
    _active_sessions: &Arc<Mutex<HashMap<String, (String, std::time::Instant)>>>
) -> Result<()> {
    // ... your existing code
    Ok(())
}

// === High-level handler that covers ALL session cases ===
pub async fn handle_session_command(
    agent: &mut crate::agent::EchoAgent,
    _user_input: &str,
    session_name: &str,
    command: Option<&str>,
) -> Result<()> {
    if let Some(cmd) = command {
    println!("{}Echo: Executing in SESSION '{}' → {}{}",
        crate::agent::YELLOW, session_name, cmd, crate::agent::RESET_COLOR);
    }
    if let Some(cmd) = command {
        if let Err(e) = is_command_safe(cmd, &agent.config) {
            println!("{}Safety block: {}{}", crate::agent::YELLOW, e, crate::agent::RESET_COLOR);
            agent.messages.push(json!({"role": "assistant", "content": format!("Safety block: {}", e)}));
        } else {
            start_or_reuse_session(agent.home_dir.clone(), &agent.active_sessions, session_name, cmd).await?;

            let raw_output = execute_in_session(agent.home_dir.clone(), &agent.active_sessions, session_name, cmd.to_string()).await?;

            let summary = match summarize_output(&raw_output, &agent.config).await {
                Ok(s) => s,
                Err(e) => format!("(Summarizer failed: {})", e),
            };

            agent.db.log_tool_call(session_name, cmd, &summary)?;

            let tool_content = format!("Tool output from SESSION '{}':\n{}", session_name, summary);
                println!("{}[Tool Summary]:\n{}{}", crate::agent::YELLOW, summary, crate::agent::RESET_COLOR);
                agent.messages.push(json!({"role": "assistant", "content": format!("Executed in session '{}'", session_name)}));
                agent.messages.push(json!({"role": "tool", "content": tool_content}));
        }
    } else {
        // END_SESSION case
       println!("{}Echo: Ending session {}{}", crate::agent::YELLOW, session_name, crate::agent::RESET_COLOR);

        let _ = end_session(agent.home_dir.clone(), &agent.active_sessions, session_name).await;

        let tool_content = format!("Session '{}' has been terminated.", session_name);
        agent.messages.push(json!({"role": "tool", "content": tool_content}));
    }

    Ok(())
}
