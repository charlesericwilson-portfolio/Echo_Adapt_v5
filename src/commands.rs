use anyhow::Result;
use crate::safety::is_command_safe;
use crate::log::save_chat_log_message;
use crate::summary::summarize_output;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command as TokioCommand;
use crate::supervisor::ToolEvent;

pub async fn handle_command(
    agent: &mut crate::agent::EchoAgent,
    _user_input: &str,
    command: &str,
) -> Result<()> {

    if let Err(e) = is_command_safe(command, &agent.config) {
        println!(
            "{}Safety block: {}{}",
            crate::agent::YELLOW,
            e,
            crate::agent::RESET_COLOR
        );

        let tool_content = format!("Safety block: {}", e);

        agent.push_tool_result(&tool_content);

        save_chat_log_message(
            &agent.home_dir,
            &agent.config.messages.tool_role_name,
            &tool_content,
        ).await?;

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
            let tool_content = "Tool error: sudo authentication failed.";

            agent.push_tool_result(tool_content);

            save_chat_log_message(
                &agent.home_dir,
                &agent.config.messages.tool_role_name,
                tool_content,
            ).await?;

            return Ok(());
        }
    }

    // Execute
    let child = TokioCommand::new("sh")
        .arg("-c")
        .arg(command.trim())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!(
            "Failed to execute '{}': {}",
            command,
            e
        ))?;

    let mut wait_for_output = Box::pin(child.wait_with_output());

    match tokio::time::timeout(
        Duration::from_secs(5),
        &mut wait_for_output,
    )
    .await
    {
        // Finished inside the 5-second foreground window.
        Ok(output_result) => {
            let output_cmd = output_result.map_err(|e| {
                anyhow::anyhow!(
                    "Failed while waiting for '{}': {}",
                    command,
                    e
                )
            })?;

            let stdout =
                String::from_utf8_lossy(&output_cmd.stdout).to_string();

            let stderr =
                String::from_utf8_lossy(&output_cmd.stderr).to_string();

            let raw_tool_content = format!(
                "Tool output from command '{}':\nSTDOUT:\n{}\nSTDERR:\n{}",
                command.trim(),
                stdout.trim(),
                stderr.trim()
            );

            let model_tool_content = summarize_output(
                &raw_tool_content,
                &agent.config
            ).await?;

            agent.push_tool_result(&model_tool_content);

            save_chat_log_message(
                &agent.home_dir,
                &agent.config.messages.tool_role_name,
                &raw_tool_content,
            ).await?;

            let summary = if raw_tool_content.len() > 500 {
                let mut end = 497.min(raw_tool_content.len());

                while end > 0
                    && !raw_tool_content.is_char_boundary(end)
                {
                    end -= 1;
                }

                format!("{}...", &raw_tool_content[..end])
            } else {
                raw_tool_content.clone()
            };

            if let Err(e) =
                agent.db.log_tool_call("command", command, &summary)
            {
                println!(
                    "{}Warning: Failed to log command to DB: {}{}",
                    crate::agent::YELLOW,
                    e,
                    crate::agent::RESET_COLOR
                );
            }

            println!(
                "{}[Tool executed — logged to database]{}",
                crate::agent::YELLOW,
                crate::agent::RESET_COLOR
            );
        }

        // Still running after 5 seconds.
        Err(_) => {
            agent.last_background_target = Some(
                crate::wait::WaitTarget::Command {
                    invocation: command.trim().to_string(),
                }
            );

            let sender = agent.tool_supervisor.sender();
            let background_command = command.trim().to_string();
            let background_ready = agent.background_ready.clone();

            let event_epoch = agent
                .background_epoch
                .load(std::sync::atomic::Ordering::SeqCst);

            tokio::spawn(async move {
                let event = match wait_for_output.await {
                    Ok(output_cmd) => {
                        let stdout =
                            String::from_utf8_lossy(&output_cmd.stdout)
                                .to_string();

                        let stderr =
                            String::from_utf8_lossy(&output_cmd.stderr)
                                .to_string();

                        ToolEvent {
                            tool_name: "command".to_string(),
                            invocation: background_command.clone(),
                            output: format!(
                                "STDOUT:\n{}\nSTDERR:\n{}",
                                stdout.trim(),
                                stderr.trim()
                            ),
                            epoch: event_epoch,
                        }
                    }

                    Err(error) => ToolEvent {
                        tool_name: "command".to_string(),
                        invocation: background_command.clone(),
                        output: format!(
                            "Command execution error: {}",
                            error
                        ),
                        epoch: event_epoch,
                    },
                };

                let _ = sender.send(event);

                background_ready.store(
                    true,
                    std::sync::atomic::Ordering::SeqCst,
                );
            });

            let tool_content = format!(
                "The command is still running in the background.\n\
                Command: {}\n\
                Status: RUNNING\n\
                Do not repeat this command again.\n\
                Do not repeat or replace this command while it is running.\n\
                You can call <wait/> if you need this data to continue. \n\
                Either call <wait/> update the user or go to the next available step in the task.",
                command.trim()
            );

            println!(
                "{}[Command continuing in background]{}",
                crate::agent::YELLOW,
                crate::agent::RESET_COLOR
            );

            agent.push_tool_result(&tool_content);

            save_chat_log_message(
                &agent.home_dir,
                &agent.config.messages.tool_role_name,
                &tool_content,
            ).await?;
        }
    }

    return Ok(());

}
