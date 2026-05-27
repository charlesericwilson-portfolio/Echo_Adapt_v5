// commands.rs
use anyhow::Result;
use serde_json::json;
use crate::log::save_chat_log_entry;
use crate::safety::is_command_safe;

pub fn extract_command(response_text: &str) -> Option<String> {
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
    user_input: &str,
    command: &str,
) -> Result<()> {
    //println!("{}Echo: Executing command: {}{}", crate::agent::LIGHT_BLUE, command.trim(), crate::agent::RESET_COLOR);
    if let Err(e) = is_command_safe(command, &agent.config) {
        println!("{}Safety block: {}{}", crate::agent::YELLOW, e, crate::agent::RESET_COLOR);
        save_chat_log_entry(&agent.home_dir, user_input, &format!("Blocked: {}", e), "assistant").await?;
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
            "Tool output from COMMAND '{}':\nSTDOUT:\n{}\nSTDERR:\n{}",
            command.trim(), stdout, stderr
        );

        agent.messages.push(json!({"role": "tool", "content": tool_content}));
    }

    Ok(())
}
