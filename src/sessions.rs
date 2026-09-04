use std::path::PathBuf;
use tokio::process::Command;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde_json::json;

use crate::supervisor::{SessionEvent, SessionState};
use crate::summary::summarize_output;
use crate::safety::is_command_safe;
use crate::config::ToolTagsConfig;
use crate::log::save_chat_log_message;

const SESSION_FOREGROUND_WAIT_MS: u64 = 2_000;
const SESSION_POLL_INTERVAL_MS: u64 = 500;

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
    active_sessions: &Arc<Mutex<HashMap<String, SessionState>>>,
    name: &str,
    _command: &str,
) -> Result<()> {
    {
        let mut sessions = active_sessions.lock().await;

        sessions
            .entry(name.to_string())
            .and_modify(|state| state.touch())
            .or_insert_with(SessionState::new);
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

pub enum SessionExecution {
    Completed(String),
    Background(i64),
}

pub async fn handle_completed_session_event(
    agent: &mut crate::agent::EchoAgent,
    event: SessionEvent,
) -> Result<()> {
    let summary = match summarize_output(&event.output, &agent.config).await {
        Ok(s) => s,
        Err(e) => format!("(Summarizer failed: {})", e),
    };

    let tool_content = format!(
        "Background SESSION '{}' completed.\nMarker: {}\nOutput summary: {}",
        event.session_name,
        event.marker_id,
        summary
    );

    agent.messages.push(json!({
        "role": &agent.config.messages.tool_role_name,
        "content": &tool_content
    }));

    save_chat_log_message(
        &agent.home_dir,
        &agent.config.messages.tool_role_name,
        &tool_content,
    ).await?;

    Ok(())
}

pub async fn execute_in_session(
    _home_dir: PathBuf,
    active_sessions: &Arc<Mutex<HashMap<String, SessionState>>>,
    name: &str,
    command: String,
) -> Result<SessionExecution> {
    let tmux_name = tmux_session_name(name);

    let marker_id = chrono::Local::now().timestamp_millis();
    let marker_start = format!("===ECHO_START_{}===", marker_id);
    let marker_end = format!("===ECHO_END_{}===", marker_id);

    {
        let mut sessions = active_sessions.lock().await;

        if let Some(state) = sessions.get_mut(name) {
            state.mark_running(marker_id);
        }
    }

    // 1. Send START marker as its own command.
    Command::new("tmux")
        .args([
            "send-keys",
            "-t",
            &tmux_name,
            &format!("echo '{}'", marker_start),
            "Enter",
        ])
        .status()
        .await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // 2. Send the actual payload as its own command.
    Command::new("tmux")
        .args([
            "send-keys",
            "-t",
            &tmux_name,
            command.trim(),
            "Enter",
        ])
        .status()
        .await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // 3. Send END marker as its own command.
    //
    // This intentionally preserves interactive-session behavior
    // such as msfconsole. Foreground commands that consume stdin
    // may prevent this marker from executing; those should use
    // the normal command tool instead of session mode.
    Command::new("tmux")
        .args([
            "send-keys",
            "-t",
            &tmux_name,
            &format!("echo '{}'", marker_end),
            "Enter",
        ])
        .status()
        .await?;

    println!(
        "{}[Session] Waiting for command to finish...{}",
        crate::agent::YELLOW,
        crate::agent::RESET_COLOR
    );

    let foreground_start = std::time::Instant::now();

    loop {
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-p",
                "-S",
                "-",
                "-t",
                &tmux_name,
            ])
            .output()
            .await?;

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let lines: Vec<&str> = raw.lines().collect();

        // Command finished during the foreground window.
        if let Some(end_idx) = lines
            .iter()
            .rposition(|line| line.trim() == marker_end)
        {
            if let Some(start_idx) = lines[..end_idx]
                .iter()
                .rposition(|line| line.trim() == marker_start)
            {
                let captured = lines[start_idx + 1..end_idx]
                    .join("\n")
                    .trim()
                    .to_string();

                // This command is no longer running.
                {
                    let mut sessions = active_sessions.lock().await;

                    if let Some(state) = sessions.get_mut(name) {
                        state.running_marker = None;
                        state.touch();
                    }
                }

                return Ok(SessionExecution::Completed(captured));
            }
        }

        // Give short commands a chance to complete normally.
        if foreground_start.elapsed()
            < std::time::Duration::from_millis(SESSION_FOREGROUND_WAIT_MS)
        {
            tokio::time::sleep(
                tokio::time::Duration::from_millis(SESSION_POLL_INTERVAL_MS)
            )
            .await;

            continue;
        }

        // The command exceeded the foreground window.
        // Clone everything the background task must own.
        let sessions = Arc::clone(active_sessions);
        let background_name = name.to_string();
        let background_tmux_name = tmux_name.clone();
        let background_marker_start = marker_start.clone();
        let background_marker_end = marker_end.clone();

        tokio::spawn(async move {
            loop {
                let output = Command::new("tmux")
                    .args([
                        "capture-pane",
                        "-p",
                        "-S",
                        "-",
                        "-t",
                        &background_tmux_name,
                    ])
                    .output()
                    .await;

                match output {
                    Ok(output) => {
                        let raw =
                            String::from_utf8_lossy(&output.stdout).to_string();

                        let lines: Vec<&str> = raw.lines().collect();

                        if let Some(end_idx) = lines
                            .iter()
                            .rposition(|line| line.trim() == background_marker_end)
                        {
                            if let Some(start_idx) = lines[..end_idx]
                                .iter()
                                .rposition(|line| {
                                    line.trim() == background_marker_start
                                })
                            {
                                let captured = lines[start_idx + 1..end_idx]
                                    .join("\n")
                                    .trim()
                                    .to_string();

                                let mut session_map = sessions.lock().await;

                                if let Some(state) =
                                    session_map.get_mut(&background_name)
                                {
                                    state.push_completed(
                                        &background_name,
                                        marker_id,
                                        captured,
                                    );
                                }

                                break;
                            }
                        }
                    }

                    Err(error) => {
                        eprintln!(
                            "[Session supervisor] Failed to capture session '{}': {}",
                            background_name,
                            error
                        );

                        break;
                    }
                }

                tokio::time::sleep(
                    tokio::time::Duration::from_millis(
                        SESSION_POLL_INTERVAL_MS
                    )
                )
                .await;
            }
        });

        return Ok(SessionExecution::Background(marker_id));
    }
}

pub async fn end_session(
    _home_dir: PathBuf,
    active_sessions: &Arc<Mutex<HashMap<String, SessionState>>>,
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
    active_sessions: Arc<Mutex<HashMap<String, SessionState>>>,
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
                .filter(|(_, state)| now.duration_since(state.last_used) > timeout)
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
    _active_sessions: &Arc<Mutex<HashMap<String, SessionState>>>
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
            println!(
                "{}Safety block: {}{}",
                crate::agent::YELLOW,
                e,
                crate::agent::RESET_COLOR
            );

            agent.messages.push(json!({
                "role": "assistant",
                "content": format!("Safety block: {}", e)
            }));

            return Ok(());
        }

        // Do not send another command into a session
        // that already has background work running.
        {
            let sessions = agent.active_sessions.lock().await;

            if let Some(state) = sessions.get(session_name) {
                if state.is_running() {
                    drop(sessions);

                    let tool_content = format!(
                        "SESSION '{}' already has a command running in the background. \
                        Wait for its completion or use a different uniquely named session \
                        for parallel work.",
                        session_name
                    );

                    println!(
                        "{}[Session '{}' already has background work running]{}",
                        crate::agent::YELLOW,
                        session_name,
                        crate::agent::RESET_COLOR
                    );

                    agent.messages.push(json!({
                        "role": &agent.config.messages.tool_role_name,
                        "content": &tool_content
                    }));

                    save_chat_log_message(
                        &agent.home_dir,
                        &agent.config.messages.tool_role_name,
                        &tool_content,
                    ).await?;

                    return Ok(());
                }
            }
        }

        start_or_reuse_session(
            agent.home_dir.clone(),
            &agent.active_sessions,
            session_name,
            cmd
        ).await?;

        let execution = execute_in_session(
            agent.home_dir.clone(),
            &agent.active_sessions,
            session_name,
            cmd.to_string()
        ).await?;

        match execution {
            SessionExecution::Completed(output) => {
                let summary = match summarize_output(&output, &agent.config).await {
                    Ok(s) => s,
                    Err(e) => format!("(Summarizer failed: {})", e),
                };

                agent.db.log_tool_call(session_name, cmd, &summary)?;

                let tool_content = format!(
                    "Tool output from SESSION '{}':\nRaw summary: {}",
                    session_name, summary
                );

                println!(
                    "{}[Session tool executed — Echo will summarize]{}",
                    crate::agent::YELLOW,
                    crate::agent::RESET_COLOR
                );

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
            }

            SessionExecution::Background(marker_id) => {
                let tool_content = format!(
                    "SESSION '{}' is still running in the background.\n\
                    Marker: {}\n\
                    Continue reasoning from the current task. \
                    Do not repeat this command while this session is still running.",
                    session_name,
                    marker_id
                );

                println!(
                    "{}[Session '{}' continuing in background, marker {}]{}",
                    crate::agent::YELLOW,
                    session_name,
                    marker_id,
                    crate::agent::RESET_COLOR
                );

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
        }

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
