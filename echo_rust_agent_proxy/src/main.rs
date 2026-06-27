use anyhow::Result;

/// Main entry point for the Echo Rust Agent.
///
/// This file is intentionally minimal. All the real logic lives in `EchoAgent`.
/// The agent handles:
/// - Loading configuration
/// - Managing chat state and tool execution
/// - Running the main interaction loop (`process_turn`)
/// - Coordinating sessions, commands, and JSON tools
mod config;
mod db;
mod log;
mod safety;
mod commands;
mod sessions;
mod json;
mod summary;
mod agent;
mod memory;

use agent::EchoAgent;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Echo Rust Agent v5 – Starting...\n");

    // Load configuration from config.toml
    // This includes endpoint URL, model name, prompts, paths, etc.
    let config = config::load_config("config.toml")
        .expect("Failed to load config.toml");

    // Create the main agent instance.
    // EchoAgent owns:
    // - The LLM client / message history
    // - Active tmux sessions
    // - Tool database
    // - Safety + summarization logic
    let mut agent = EchoAgent::new(config).await?;

    // Run the main interactive loop.
    // This is where user input is processed and tool calls happen.
    agent.run().await?;

    println!("\nEcho session ended normally. Goodbye!");
    Ok(())
}
