//! EchoAgent - The core orchestrator for the Echo Rust Agent Framework.
//!
//! This module contains the main agent logic:
//! - `EchoAgent` struct: owns configuration, message history, active tmux sessions,
//!   the tool database, and generation control flags.
//! - `new()`: Initializes the agent (loads system prompt + optional context file).
//! - `run()`: Main interactive loop that reads user input and calls `process_turn()`.
//! - `process_turn()`: The heart of the agent. It loops between calling the LLM
//!   and executing tools until the model produces a final answer.
//!
//! Tool detection happens via simple prefix matching on the model's response:
//! - `JSON_TOOL:`     → structured tool calls (see json.rs)
//! - `SESSION:`       → persistent tmux session commands (see sessions.rs)
//! - `END_SESSION:`   → terminate a tmux session
//! - `COMMAND:`       → one-shot command execution (see commands.rs)
//!
//! After a tool runs, we append the result (as a "tool" role message) and loop
//! back to the model. This enables autonomous multi-step tool use.

use std::path::PathBuf;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde_json::{Value, json};
use anyhow::Result;
use std::collections::HashMap;
use dirs_next as dirs;
use std::sync::atomic::Ordering;

use crate::sessions::start_session_cleanup_task;
use crate::config::Config;
use crate::db::ToolDatabase;
use crate::summary::summarize_context;
use crate::sessions::{extract_session_command, extract_end_command, clean_up_sessions};
use crate::log::save_chat_log_entry;
use crate::commands::extract_command;
use crate::json::extract_json_tool;

// Terminal color helpers
pub const LIGHT_BLUE: &str = "\x1b[94m";
pub const YELLOW: &str = "\x1b[33m";
pub const RESET_COLOR: &str = "\x1b[0m";

/// The main agent struct.
///
/// Holds all persistent state for a single agent session:
/// - `config`: Endpoint, model, prompts, paths, etc.
/// - `messages`: Full chat history (system + user + assistant + tool messages)
/// - `db`: SQLite database for logging tool calls
/// - `home_dir`: Working directory for tmux sessions
/// - `active_sessions`: Currently open tmux sessions (name → metadata)
/// - `stop_generation`: Atomic flag used by the Ctrl+\ signal handler
pub struct EchoAgent {
    pub config: Config,
    pub messages: Vec<Value>,
    pub db: ToolDatabase,
    pub home_dir: PathBuf,
    pub active_sessions: Arc<Mutex<HashMap<String, (String, std::time::Instant)>>>,
    pub stop_generation: Arc<std::sync::atomic::AtomicBool>,
}

impl EchoAgent {
    /// Create a new EchoAgent.
    ///
    /// This does several important things:
    /// 1. Determines the home directory (from config or fallback).
    /// 2. Loads an optional context file (long-term memory / instructions).
    /// 3. Loads the main system prompt and combines it with the context.
    /// 4. Initializes the tool call database.
    pub async fn new(config: Config) -> Result<Self> {
        let home_dir = match &config.paths.home_dir {
            Some(path) if !path.trim().is_empty() => PathBuf::from(path),
            _ => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user/Documents")),
        };

        let context_path = if config.paths.context_file.starts_with('/') {
            PathBuf::from(&config.paths.context_file)
        } else {
            home_dir.join(&config.paths.context_file)
        };

        let db_path = if config.paths.database.starts_with('/') {
            PathBuf::from(&config.paths.database)
        } else {
            home_dir.join(&config.paths.database)
        };

        let db = ToolDatabase::new(db_path)?;

        let mut messages = vec![];
        let mut context_content = String::new();

        if tokio::fs::metadata(&context_path).await.is_ok() {
            context_content = tokio::fs::read_to_string(&context_path).await.unwrap_or_default();
            println!("✅ Loaded context file: {}", context_path.display());
        } else {
            println!("⚠️ Context file not found at: {}", context_path.display());
        }

        let main_prompt = tokio::fs::read_to_string(&config.prompts.main_system)
            .await
            .expect("Failed to read main system prompt");

        let full_system_prompt = format!("{}\n\n{}", main_prompt.trim(), context_content.trim());
        messages.push(json!({"role": "system", "content": full_system_prompt}));

        let active_sessions = Arc::new(Mutex::new(HashMap::new()));

        // Create the agent first
        let agent = Self {
            config,
            messages,
            db,
            home_dir,
            active_sessions: active_sessions.clone(),
            stop_generation: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        // Now start the background cleanup task (this is a function call, not a field)
        start_session_cleanup_task(active_sessions).await;

        Ok(agent)
    }

    /// Main interactive loop.
    ///
    /// - Prints the prompt and reads user input.
    /// - Handles `quit` / `exit`.
    /// - Sets up Ctrl+\ (SIGQUIT) handler for interrupting generation.
    /// - Calls `process_turn()` for each user message.
    /// - Cleans up tmux sessions on exit.
    pub async fn run(&mut self) -> Result<()> {
        println!("Echo: Ready. Type 'quit' or 'exit' to end session.\n");

        // Set up Ctrl+\ interrupt handler
        let mut quit = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::quit())
            .expect("Failed to set up SIGQUIT handler");

        let stop_flag = self.stop_generation.clone();

        tokio::spawn(async move {
            while quit.recv().await.is_some() {
                stop_flag.store(true, Ordering::SeqCst);
                println!("\n[Generation interrupted by Ctrl+\\]");
            }
        });

        loop {
            print!("You: ");
            std::io::stdout().flush()?;

            let mut user_input = String::new();
            std::io::stdin().read_line(&mut user_input)?;
            let trimmed_input = user_input.trim();

            if trimmed_input.eq_ignore_ascii_case("quit") || trimmed_input.eq_ignore_ascii_case("exit") {
                println!("Session ended.");
                save_chat_log_entry(&self.home_dir, "", "", "SESSION_END").await?;
                break;
            }

            self.messages.push(json!({"role": "user", "content": trimmed_input}));

            let final_response = self.process_turn(trimmed_input).await?;

            println!("{}Echo:\n{}\n{}", LIGHT_BLUE, final_response.trim(), RESET_COLOR);
        }

        clean_up_sessions(&self.active_sessions).await?;
        Ok(())
    }

    /// Core agent loop — this is where the magic happens.
    ///
    /// The loop works like this:
    /// 1. Send current message history to the LLM.
    /// 2. Get a response.
    /// 3. Check if the response contains a tool call (JSON_TOOL, SESSION, END_SESSION, COMMAND).
    /// 4. If yes → execute the tool, append the result as a "tool" message, and loop.
    /// 5. If no  → this is the final answer. Optionally summarize context, then return.
    ///
    /// This design enables autonomous multi-step tool use without the user
    /// having to intervene between every tool call.
    #[allow(unused_assignments)]
    async fn process_turn(&mut self, user_input: &str) -> Result<String> {
        loop {
            // Build the request payload
            let payload = json!({
                "model": self.config.endpoint.model,
                "messages": &self.messages,
                "temperature": self.config.endpoint.temperature,
                "max_tokens": self.config.endpoint.max_tokens
            });

            // Check for user interrupt (Ctrl+\)
            if self.stop_generation.load(Ordering::SeqCst) {
                self.stop_generation.store(false, Ordering::SeqCst);
                return Ok("[Generation stopped by user]".to_string());
            }

            // Call the LLM
            let response_text = reqwest::Client::new()
                .post(&self.config.endpoint.url)
                .json(&payload)
                .send()
                .await?
                .json::<Value>()
                .await?["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_string();

            // === Check for tool calls FIRST, before pushing anything to history ===
            if let Some(command) = extract_command(&response_text) {
                // Push neutral placeholder instead of the raw COMMAND: line
                self.messages.push(json!({"role": "assistant", "content": "command executed"}));
                crate::commands::handle_command(self, user_input, &command).await?;
                continue;

            } else if let Some((session_name, command)) = extract_session_command(&response_text) {
                self.messages.push(json!({"role": "assistant", "content": "command executed"}));
                crate::sessions::handle_session_command(self, user_input, &session_name, Some(&command)).await?;
                continue;

            } else if let Some(session_name) = extract_end_command(&response_text) {
                self.messages.push(json!({"role": "assistant", "content": "command executed"}));
                crate::sessions::handle_session_command(self, user_input, &session_name, None).await?;
                continue;

            } else if let Some(json_content) = extract_json_tool(&response_text) {
                self.messages.push(json!({"role": "assistant", "content": "json tool executed"}));
                crate::json::handle_json_tool(self, user_input, &response_text, &json_content).await?;
                continue;

            } else {
                // No tool flag found → this is the final answer
                save_chat_log_entry(&self.home_dir, user_input, &response_text, "assistant").await?;
                self.messages.push(json!({"role": "assistant", "content": &response_text}));

                // Auto-summarize if context is getting too long
                let total_chars: usize = self.messages.iter()
                    .map(|m| m["content"].as_str().unwrap_or("").len())
                    .sum();

                if total_chars > self.config.context.summarize_threshold {
                    summarize_context(&mut self.messages, &self.config).await?;
                }

                return Ok(response_text);
            }
        }
    }
}
