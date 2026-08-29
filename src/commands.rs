use anyhow::Result;
use serde_json::json;
use crate::safety::is_command_safe;
use crate::config::ToolTagsConfig;
use crate::log::save_chat_log_message;

/// Extracts a command dynamically based on configured tags
pub fn extract_command(response_text: &str, tags: &ToolTagsConfig) -> Option<String> {
    if let Some(start) = response_text.find(&tags.command_open) {
        let content_start = start + tags.command_open.len();
        if let Some(end) = response_text[content_start..].find(&tags.command_close) {
            let inner = &response_text[content_start..content_start + end];
            return Some(inner.trim().to_string());
        }
    }
    None
}

pub async fn handle_command(
    agent: &mut crate::agent::EchoAgent,
    _user_input: &str,
    command: &str,
) -> Result<()> {
    println!("{}Echo: Executing COMMAND → {}{}",
             crate::agent::YELLOW, command, crate::agent::RESET_COLOR);

    if let Err(e) = is_command_safe(command, &agent.config) {
        println!("{}Safety block: {}{}", crate::agent::YELLOW, e, crate::agent::RESET_COLOR);
        agent.messages.push(json!({"role": "assistant", "content": format!("Safety block: {}", e)}));
        return Ok(());
    }

    // === SUDO SUPPORT ===
    // Adapt never handles the user's password directly.
    // sudo performs authentication through the user's terminal.
    let needs_sudo = command.trim().to_lowercase().starts_with("sudo ");

    if needs_sudo {
        println!(
            "{}[SUDO] This command requires elevated privileges.{}",
            crate::agent::YELLOW,
            crate::agent::RESET_COLOR
        );

        let status = std::process::Command::new("sudo")
            .arg("-v")
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to request sudo authentication: {}", e))?;

        if !status.success() {
            agent.messages.push(json!({
                "role": &agent.config.messages.tool_role_name,
                "content": "Tool error: sudo authentication failed."
            }));
            return Ok(());
        }
    }

    // Execute
    let output_cmd = std::process::Command::new("sh")
        .arg("-c")
        .arg(command.trim())
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute '{}': {}", command, e))?;

    let stdout = String::from_utf8_lossy(&output_cmd.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();

    let tool_content = format!(
        "Tool output from command '{}':\nSTDOUT:\n{}\nSTDERR:\n{}",
        command.trim(), stdout.trim(), stderr.trim()
    );

    // Store in live model context.
    agent.messages.push(json!({
        "role": &agent.config.messages.tool_role_name,
        "content": &tool_content
    }));

    // Store the same tool result in the persistent transcript.
    save_chat_log_message(
        &agent.home_dir,
        &agent.config.messages.tool_role_name,
        &tool_content,
    ).await?;

    // Log tool
    let summary = if tool_content.len() > 500 {
        let mut end = 497.min(tool_content.len());

        while end > 0 && !tool_content.is_char_boundary(end) {
            end -= 1;
        }

        format!("{}...", &tool_content[..end])
    } else {
        tool_content.clone()
    };

    if let Err(e) = agent.db.log_tool_call("command", command, &summary) {
        println!("{}Warning: Failed to log command to DB: {}{}",
                 crate::agent::YELLOW, e, crate::agent::RESET_COLOR);
    }

    println!("{}[Tool executed — logged to database]{}",
             crate::agent::YELLOW, crate::agent::RESET_COLOR);

    Ok(())
}
