// commands.rs
use anyhow::Result;
use serde_json::json;
use crate::safety::is_command_safe;
use std::io::{self, Write};

/// Extracts a command from <command>multi-line command here</command>
pub fn extract_command(response_text: &str) -> Option<String> {
    if let Some(start) = response_text.find("<command>") {
        if let Some(end) = response_text[start..].find("</command>") {
            let inner = &response_text[start + 9..start + end];
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
    let needs_sudo = command.trim().to_lowercase().starts_with("sudo ");
    let mut final_cmd = command.trim().to_string();

    if needs_sudo {
        print!("{}[SUDO] Enter sudo password: {}", crate::agent::YELLOW, crate::agent::RESET_COLOR);
        io::stdout().flush()?;
        let mut password = String::new();
        io::stdin().read_line(&mut password)?;
        let password = password.trim();

        final_cmd = format!(
            "echo '{}' | sudo -S {}",
            password.replace("'", "'\\''"),
            command.trim().strip_prefix("sudo ").unwrap_or(command.trim())
        );
    }

    // Execute
    let output_cmd = std::process::Command::new("sh")
        .arg("-c")
        .arg(&final_cmd)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute '{}': {}", command, e))?;

    let stdout = String::from_utf8_lossy(&output_cmd.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();

    let tool_content = format!(
        "Tool output from command '{}':\nSTDOUT:\n{}\nSTDERR:\n{}",
        command.trim(), stdout.trim(), stderr.trim()
    );

    // Only store in history, DO NOT print raw output ===
    agent.messages.push(json!({"role": "tool", "content": tool_content}));
    agent.messages.push(serde_json::json!({
        "role": "user",
        "content": "Summarize the tool result above and continue with the next step or final answer."
    }));
    //Log tool
    let summary = if tool_content.len() > 500 {
        format!("{}...", &tool_content[..497])
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
