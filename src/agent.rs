//! EchoAgent - The core orchestrator for the Echo Rust Agent Framework.
//!
//! This module contains the main agent logic:
//! - `EchoAgent` struct: owns configuration, message history, active tmux sessions,
//!   the tool database, and generation control flags.
//! - `new()`: Initializes the agent (loads system prompt + optional context file).
//! - `run()`: Main interactive loop that reads user input and calls `process_turn()`.
//! - `process_turn()`: The heart of the agent. It loops between calling the LLM
//!   and executing tools until the model produces a final answer.

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
use crate::commands::extract_command;
use crate::json::extract_json_tool;
use crate::cleanup::{extract_cleanup, handle_cleanup};
use crate::hotkeys::{self, InputAction};
use crate::log::{save_chat_log_entry, save_chat_log_message};

// Terminal color helpers
pub const LIGHT_BLUE: &str = "\x1b[94m";
pub const YELLOW: &str = "\x1b[33m";
pub const RESET_COLOR: &str = "\x1b[0m";

pub struct EchoAgent {
    pub config: Config,
    pub messages: Vec<Value>,
    pub db: ToolDatabase,
    pub home_dir: PathBuf,
    pub max_turns_counter: u32,
    pub active_sessions: Arc<Mutex<HashMap<String, (String, std::time::Instant)>>>,
    pub stop_generation: Arc<std::sync::atomic::AtomicBool>,
}

impl EchoAgent {
    pub async fn new(config: Config) -> Result<Self> {
        let home_dir = match &config.paths.home_dir {
    Some(path) if !path.trim().is_empty() => PathBuf::from(path),

        _ => dirs::home_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "Unable to determine the current user's home directory. \
                Set paths.home_dir explicitly in config.toml."
            )
        })?,
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

        let initial_counter: u32 = 0;
        let active_sessions = Arc::new(Mutex::new(HashMap::new()));

        let agent = Self {
            config,
            messages,
            db,
            home_dir,
            max_turns_counter: initial_counter,
            active_sessions: active_sessions.clone(),
            stop_generation: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        start_session_cleanup_task(active_sessions).await;

        Ok(agent)
    }

    pub async fn run(&mut self) -> Result<()> {
        println!("Echo: Ready. Type 'quit' or 'exit' to end session.\n");

        self.max_turns_counter = 0;

        let mut quit = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::quit())
            .expect("Failed to set up SIGQUIT handler");

        let stop_flag = self.stop_generation.clone();

        tokio::spawn(async move {
            while quit.recv().await.is_some() {
                stop_flag.store(true, Ordering::SeqCst);
                println!("\n[Generation interrupted by Ctrl \\]");
            }
        });

        loop {
            print!("You: ");
            std::io::stdout().flush()?;

            let user_input = match hotkeys::read_user_input()? {
            InputAction::NewTab => {
                hotkeys::spawn_new_adapt_tab()?;
                continue;
            }

            InputAction::Exit => {
            println!("Session ended.");
            save_chat_log_entry(&self.home_dir, "", "", "SESSION_END").await?;
            break;
        }


            InputAction::Submit(input) => input,
        };

let trimmed_input = user_input.trim();

            if trimmed_input.eq_ignore_ascii_case("quit") || trimmed_input.eq_ignore_ascii_case("exit") {
                println!("Session ended.");
                save_chat_log_entry(&self.home_dir, "", "", "SESSION_END").await?;
                break;
            }

            self.max_turns_counter = 0;

            self.messages.push(json!({
                "role": "user",
                "content": trimmed_input
            }));

            save_chat_log_message(
                &self.home_dir,
                "user",
                trimmed_input,
            ).await?;

            let final_response = self.process_turn(trimmed_input).await?;
            println!("{}Echo:\n{}\n{}", LIGHT_BLUE, final_response.trim(), RESET_COLOR);
        }

        clean_up_sessions(&self.active_sessions).await?;
        Ok(())
    }

    #[allow(unused_assignments)]
    async fn process_turn(&mut self, user_input: &str) -> Result<String> {
        loop {
            let payload = json!({
                "model": self.config.endpoint.model,
                "messages": &self.messages,
                "temperature": self.config.endpoint.temperature,
                "max_tokens": self.config.endpoint.max_tokens
            });

            if self.stop_generation.load(Ordering::SeqCst) {
                self.stop_generation.store(false, Ordering::SeqCst);
                return Ok("[Generation stopped by user]".to_string());
            }

            self.max_turns_counter = self.max_turns_counter.saturating_add(1);

            if self.max_turns_counter >= self.config.context.max_turns {
                let _ = self.handle_max_trigger().await;
                return Ok("[Triggered inactivity pause]".to_string());
            }

            let response = reqwest::Client::new()
                .post(&self.config.endpoint.url)
                .json(&payload)
                .send()
                .await?
                .error_for_status()?;

            let response_json = response.json::<Value>().await?;

            let response_text = response_json
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Model endpoint returned an unexpected response format: {}",
                        response_json
                    )
                })?
                .trim()
                .to_string();

            save_chat_log_message(
                &self.home_dir,
                "assistant",
                &response_text,
            ).await?;

            let tags = self.config.tool_tags.clone();

            // 1. Check for command execution
            if let Some(command) = extract_command(&response_text, &tags) {
                let block = format!("{}{}{}", tags.command_open, command, tags.command_close);
                let cleaned = response_text.replace(&block, "").trim().to_string();

                self.messages.push(json!({"role": "assistant", "content": cleaned}));
                if !cleaned.trim().is_empty() {
                    println!("{}Echo:\n{}\n{}", LIGHT_BLUE, cleaned.trim(), RESET_COLOR);
                }
                crate::commands::handle_command(self, user_input, &command).await?;
                continue;

            // 2. Check for tmux session command
            } else if let Some((session_name, command)) = extract_session_command(&response_text, &tags) {
                let full_open = format!("{}{}\">", tags.session_open, session_name);
                let block = format!("{}{}{}", full_open, command, tags.session_close);
                let cleaned = response_text.replace(&block, "").trim().to_string();

                self.messages.push(json!({"role": "assistant", "content": cleaned}));
                if !cleaned.trim().is_empty() {
                    println!("{}Echo:\n{}\n{}", LIGHT_BLUE, cleaned.trim(), RESET_COLOR);
                }
                crate::sessions::handle_session_command(self, user_input, &session_name, Some(&command)).await?;
                continue;

            // 3. Check for end session command
            } else if let Some(session_name) = extract_end_command(&response_text, &tags) {
                let full_tag = format!("{}{}\"/>", tags.end_session_open, session_name);
                let fallback_tag = format!("{}{}\">", tags.end_session_open, session_name);
                let cleaned = response_text
                    .replace(&full_tag, "")
                    .replace(&fallback_tag, "")
                    .trim()
                    .to_string();

                self.messages.push(json!({"role": "assistant", "content": cleaned}));
                if !cleaned.trim().is_empty() {
                    println!("{}Echo:\n{}\n{}", LIGHT_BLUE, cleaned.trim(), RESET_COLOR);
                }
                crate::sessions::handle_session_command(self, user_input, &session_name, None).await?;
                continue;

            // 4. Check for JSON tool call
            } else if let Some(json_content) = extract_json_tool(&response_text, &tags) {
                let block = format!("{}{}{}", tags.json_open, json_content, tags.json_close);
                let cleaned = response_text.replace(&block, "").trim().to_string();

                self.messages.push(json!({"role": "assistant", "content": cleaned}));
                if !cleaned.trim().is_empty() {
                    println!("{}Echo:\n{}\n{}", LIGHT_BLUE, cleaned.trim(), RESET_COLOR);
                }
                crate::json::handle_json_tool(self, user_input, &response_text, &json_content).await?;
                continue;

            // 5. Cleanup tool check
            } else if extract_cleanup(&response_text).is_some() {
                let cleaned = response_text
                    .replace("<cleanup/>", "")
                    .replace("<cleanup>", "")
                    .trim()
                    .to_string();

                self.messages.push(json!({"role": "assistant", "content": cleaned}));
                if !cleaned.trim().is_empty() {
                    println!("{}Echo:\n{}\n{}", LIGHT_BLUE, cleaned.trim(), RESET_COLOR);
                }
                handle_cleanup(self, user_input).await?;
                continue;

            // 6. Final Assistant Response
            } else {
                self.messages.push(json!({"role": "assistant", "content": &response_text}));

                let total_chars: usize = self.messages.iter()
                    .map(|m| m["content"].as_str().unwrap_or("").len())
                    .sum();

                if total_chars > self.config.context.summarize_threshold {
                    summarize_context(&mut self.messages, &self.config).await?;
                }

                self.max_turns_counter = 0;
                return Ok(response_text);
            }
        }
    }

    async fn handle_max_trigger(&mut self) -> Result<()> {
        println!(
            "{}⚠️ [SAFETY TRIGGER] Model has responded {} times without user input. Pausing...{}",
            YELLOW, self.config.context.max_turns, RESET_COLOR
        );

        let trigger_message = json!({
            "role": "assistant",
            "content": format!(
                "You have gone {} turns without human interaction. Pausing now.",
                self.config.context.max_turns,
            )
        });

        self.messages.push(trigger_message.clone());
        save_chat_log_entry(&self.home_dir, "", &trigger_message["content"].as_str().unwrap_or(""), "SAFETY").await?;
        self.max_turns_counter = 0;

        Ok(())
    }
}
