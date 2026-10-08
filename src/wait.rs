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

    let spinner = crate::ui::spinner("Waiting for tool... 0s");
    let started = std::time::Instant::now();

    loop {
        // The target may already have completed before Echo emitted <wait/>.
        if let Some(position) = agent
            .pending_background_output
            .iter()
            .position(|pending| pending.target == target)
        {
            let completed =
                agent.pending_background_output.remove(position);

            spinner.finish_and_clear();

            push_wait_result(agent, &completed.content).await?;

            agent.last_background_target = None;

            return Ok(());
        }

        agent.collect_background_completions().await?;

        // Ctrl-\ already sets this flag in the normal agent runtime.
        // Honor it here too so <wait/> cannot make Adapt impossible to interrupt.
        if agent.stop_generation.load(Ordering::SeqCst) {
            agent.stop_generation.store(false, Ordering::SeqCst);

            spinner.finish_and_clear();

            let tool_content =
                "Wait interrupted by the human operator.";

            push_wait_result(agent, tool_content).await?;

            return Ok(());
        }

        spinner.set_message(format!(
            "Waiting for tool... {}s",
            started.elapsed().as_secs()
        ));

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
