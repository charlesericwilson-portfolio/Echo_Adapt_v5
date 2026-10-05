//! Blocking wait support for background tool execution.
//!
//! `<wait/>` means:
//! Block until the most recently backgrounded tool invocation completes.
//!
//! It does not wait for all background work and does not expose internal
//! runtime identifiers to the model.

use anyhow::Result;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::log::save_chat_log_message;
use crate::sessions::handle_completed_session_event;
use crate::summary::summarize_output;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitTarget {
    Command {
        invocation: String,
    },

    Session {
        name: String,
        marker_id: i64,
    },

    Json {
        tool_name: String,
        invocation: String,
    },
}

#[derive(Debug, Clone)]
pub struct PendingBackgroundOutput {
    pub target: WaitTarget,
    pub content: String,
}

/// Block until the most recently backgrounded tool completes.
///
/// Unrelated background completions are preserved in
/// `pending_background_output` and are not injected into the model merely
/// because `<wait/>` was called.
pub async fn handle_wait(
    agent: &mut crate::agent::EchoAgent,
) -> Result<()> {
    let Some(target) = agent.last_background_target.clone() else {
        let tool_content =
            "Wait requested, but there is no backgrounded tool to wait for.";

        push_wait_result(agent, tool_content).await?;
        return Ok(());
    };

    println!(
        "{}Echo: Waiting for background tool result...{}",
        crate::agent::YELLOW,
        crate::agent::RESET_COLOR
    );

    loop {
        // The target may already have completed before Echo emitted <wait/>.
        if let Some(position) = agent
            .pending_background_output
            .iter()
            .position(|pending| pending.target == target)
        {
            let completed =
                agent.pending_background_output.remove(position);

            push_wait_result(agent, &completed.content).await?;

            agent.last_background_target = None;

            return Ok(());
        }

        // Collect completed session work.
        //
        // Session completions live inside SessionState rather than the
        // general ToolSupervisor channel.
        let mut completed_session_events = Vec::new();

        {
            let mut sessions = agent.active_sessions.lock().await;

            for state in sessions.values_mut() {
                while let Some(event) = state.take_pending() {
                    completed_session_events.push(event);
                }
            }
        }

        for event in completed_session_events {
            handle_completed_session_event(agent, event).await?;
        }

        // Collect completed command / JSON work.
        while let Some(event) = agent.tool_supervisor.take_pending() {
            let output_for_model =
                match summarize_output(&event.output, &agent.config).await {
                    Ok(output) => output,

                    Err(error) => {
                        eprintln!(
                            "{}Echo: [BACKGROUND SUMMARY ERROR] {}. Using raw output.{}",
                            crate::agent::YELLOW,
                            error,
                            crate::agent::RESET_COLOR
                        );

                        event.output.clone()
                    }
                };

            let tool_content = format!(
                "Background tool completed.\n\
                 Tool: {}\n\
                 Invocation: {}\n\
                 Status: COMPLETED\n\
                 Output:\n{}",
                event.tool_name,
                event.invocation,
                output_for_model
            );

            let event_target = if event.tool_name == "command" {
                WaitTarget::Command {
                    invocation: event.invocation.clone(),
                }
            } else {
                WaitTarget::Json {
                    tool_name: event.tool_name.clone(),
                    invocation: event.invocation.clone(),
                }
            };

            agent.pending_background_output.push(
                PendingBackgroundOutput {
                    target: event_target,
                    content: tool_content.clone(),
                }
            );

            save_chat_log_message(
                &agent.home_dir,
                &agent.config.messages.tool_role_name,
                &tool_content,
            )
            .await?;

            let db_summary: String =
                event.output.chars().take(500).collect();

            if let Err(error) = agent.db.log_tool_call(
                &event.tool_name,
                &event.invocation,
                &db_summary,
            ) {
                eprintln!(
                    "{}Echo: [BACKGROUND DB ERROR] Failed to log '{}' completion: {}{}",
                    crate::agent::YELLOW,
                    event.tool_name,
                    error,
                    crate::agent::RESET_COLOR
                );
            }
        }

        // Ctrl-\ already sets this flag in the normal agent runtime.
        // Honor it here too so <wait/> cannot make Adapt impossible to interrupt.
        if agent.stop_generation.load(Ordering::SeqCst) {
            agent.stop_generation.store(false, Ordering::SeqCst);

            let tool_content =
                "Wait interrupted by the human operator.";

            push_wait_result(agent, tool_content).await?;

            return Ok(());
        }

        // Don't burn a CPU core while waiting for completion.
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Inject only the result associated with `<wait/>`.
///
/// Deliberately does not use EchoAgent::push_tool_result(), because that
/// helper also attaches every pending background result. `<wait/>` must
/// return only the background operation Echo explicitly chose to wait for.
async fn push_wait_result(
    agent: &mut crate::agent::EchoAgent,
    content: &str,
) -> Result<()> {
    agent.messages.push(json!({
        "role": &agent.config.messages.tool_role_name,
        "content": content
    }));

    save_chat_log_message(
        &agent.home_dir,
        &agent.config.messages.tool_role_name,
        content,
    )
    .await?;

    Ok(())
}
