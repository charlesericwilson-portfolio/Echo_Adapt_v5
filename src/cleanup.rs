//! Cleanup tool module for EchoAgent.
//!
//! Handles detecting and executing the `<cleanup/>` tag to purge
//! temporary scratchpad artifacts in `./workspace/temp/` relative to
//! the agent's current working directory.

use anyhow::Result;
use std::fs;
use serde_json::json;
use crate::EchoAgent;
use crate::log::save_chat_log_entry;

/// Check if the model response contains a `<cleanup>` or `<cleanup/>` tag.
pub fn extract_cleanup(text: &str) -> Option<()> {
    if text.contains("<cleanup>") || text.contains("<cleanup/>") {
        Some(())
    } else {
        None
    }
}

/// Execute the relative workspace cleanup and append the result to `agent.messages`.
pub async fn handle_cleanup(agent: &mut EchoAgent, user_input: &str) -> Result<()> {
    // Dynamically get the current working directory where the process was executed
    let current_dir = std::env::current_dir().unwrap_or_else(|_| agent.home_dir.clone());
    let temp_dir = current_dir.join("workspace").join("temp");

    let output = if temp_dir.exists() {
        let mut count = 0;
        let mut err_msg = None;

        match fs::read_dir(&temp_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let res = if path.is_dir() {
                        fs::remove_dir_all(&path)
                    } else {
                        fs::remove_file(&path)
                    };

                    if let Err(e) = res {
                        err_msg = Some(e.to_string());
                        break;
                    }
                    count += 1;
                }

                if let Some(e) = err_msg {
                    format!("<tool_output>Error during cleanup in {}: {}</tool_output>", temp_dir.display(), e)
                } else {
                    format!("<tool_output>Successfully purged {} ({} items removed).</tool_output>", temp_dir.display(), count)
                }
            }
            Err(e) => format!("<tool_output>Failed to read {}: {}</tool_output>", temp_dir.display(), e),
        }
    } else {
        format!("<tool_output>Directory {} does not exist or is already empty.</tool_output>", temp_dir.display())
    };

    println!("🧹 Executed Cleanup: {}", output);

    save_chat_log_entry(&agent.home_dir, user_input, &output, "cleanup_tool").await?;
    agent.messages.push(json!({"role": "tool", "content": output}));

    Ok(())
}
