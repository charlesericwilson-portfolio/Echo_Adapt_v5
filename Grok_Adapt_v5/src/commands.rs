// commands.rs
use anyhow::Result;
use serde_json::json;
use crate::safety::is_command_safe;

/// Extracts a command from either:
/// - New style: <command>multi-line command here</command>
pub fn extract_command(response_text: &str) -> Option<String> {
    // First try the new tag style (supports multi-line)
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
    println!("{}Echo: Executing COMMAND → {}{}", crate::agent::YELLOW, command, crate::agent::RESET_COLOR);

    if let Err(e) = is_command_safe(command, &agent.config) {
        println!("{}Safety block: {}{}", crate::agent::YELLOW, e, crate::agent::RESET_COLOR);
        agent.messages.push(json!({"role": "assistant", "content": format!("Safety block: {}", e)}));
        return Ok(());
    }

    let output_cmd = std::process::Command::new("sh")
        .arg("-c")
        .arg(command.trim())
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute command '{}': {}", command, e))?;

        let stdout = String::from_utf8_lossy(&output_cmd.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();

        let tool_content = format!(
            "Tool output from command '{}':\nSTDOUT:\n{}\nSTDERR:\n{}",
            command.trim(), stdout, stderr
        );

        // Replace the COMMAND: line with neutral text so it doesn't trigger again
              agent.messages.push(json!({"role": "tool", "content": tool_content}));

    Ok(())
}
