// commands.rs
use anyhow::Result;
use serde_json::json;
use crate::safety::is_command_safe;

/// Extracts a command from either:
/// - Old style: COMMAND: command here
/// - New style: <COMMAND>multi-line command here</COMMAND>
pub fn extract_command(response_text: &str) -> Option<String> {
    // First try the new tag style (supports multi-line)
    if let Some(start) = response_text.find("<COMMAND>") {
        if let Some(end) = response_text[start..].find("</COMMAND>") {
            let inner = &response_text[start + 9..start + end];
            return Some(inner.trim().to_string());
        }
    }

    // Fall back to old single-line COMMAND: style
    for line in response_text.lines() {
        let line = line.trim();
        if let Some(cmd) = line.strip_prefix("COMMAND:") {
            return Some(cmd.trim().to_string());
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
    } else {
        let output_cmd = std::process::Command::new("sh")
            .arg("-c")
            .arg(command.trim())
            .output()
            .expect("Failed to execute command");

        let stdout = String::from_utf8_lossy(&output_cmd.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();

        let tool_content = format!(
            "Tool output from command '{}':\nSTDOUT:\n{}\nSTDERR:\n{}",
            command.trim(), stdout, stderr
        );

        // Replace the COMMAND: line with neutral text so it doesn't trigger again
              agent.messages.push(json!({"role": "tool", "content": tool_content}));
    }

    Ok(())
}
