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

use crate::supervisor::SessionState;
use crate::sessions::start_session_cleanup_task;
use crate::config::Config;
use crate::db::ToolDatabase;
use crate::summary::summarize_context;
use crate::sessions::{clean_up_sessions, handle_completed_session_event};
use crate::cleanup::handle_cleanup;
use crate::parser::{extract_last_tool_call, remove_tool_call_for_display, ParsedToolCallKind};
use crate::hotkeys::{self, InputAction};
use crate::log::{save_chat_log_entry, save_chat_log_message};
use crate::providers;

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
    pub active_sessions: Arc<Mutex<HashMap<String, SessionState>>>,
    pub stop_generation: Arc<std::sync::atomic::AtomicBool>,
    pub pending_background_output: Vec<String>,

    // Soft duplicate-tool detection. This never blocks execution.
    pub last_tool_signature: Option<String>,
    pub repeated_tool_call_count: u32,
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

        let db_path = if config.paths.database.starts_with('/') {
            PathBuf::from(&config.paths.database)
        } else {
            home_dir.join(&config.paths.database)
        };

        let db = ToolDatabase::new(db_path)?;

        let mut messages = vec![];

        let mut context_content = String::new();

        if !config.paths.context_file.trim().is_empty() {
            let context_path = if config.paths.context_file.starts_with('/') {
                PathBuf::from(&config.paths.context_file)
            } else {
                home_dir.join(&config.paths.context_file)
            };

            if tokio::fs::metadata(&context_path).await.is_ok() {
                match tokio::fs::read_to_string(&context_path).await {
                    Ok(content) => {
                        context_content = content;
                        println!("✅ Loaded context file: {}", context_path.display());
                    }

                    Err(e) => {
                        println!(
                            "⚠️ Could not read context file {}: {}",
                            context_path.display(),
                            e
                        );
                    }
                }
            } else {
                println!("⚠️ Context file not found at: {}", context_path.display());
            }
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
            pending_background_output: Vec::new(),
            last_tool_signature: None,
            repeated_tool_call_count: 0,
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

            // New human input is a fresh recovery boundary.
            self.reset_tool_repeat_tracker();

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

            let mut completed_events = Vec::new();

            {
                let mut sessions = self.active_sessions.lock().await;

                for state in sessions.values_mut() {
                    while let Some(event) = state.take_pending() {
                        completed_events.push(event);
                    }
                }
            }

            for event in completed_events {
                handle_completed_session_event(self, event).await?;
            }

            let payload = providers::build_payload(
                &self.config.endpoint,
                &self.messages,
            )?;

            if self.stop_generation.load(Ordering::SeqCst) {
                self.stop_generation.store(false, Ordering::SeqCst);
                return Ok("[Generation stopped by user]".to_string());
            }

            self.max_turns_counter = self.max_turns_counter.saturating_add(1);

            if self.max_turns_counter >= self.config.context.max_turns {
                let _ = self.handle_max_trigger().await;
                return Ok("[Triggered inactivity pause]".to_string());
            }

            let client = reqwest::Client::new();

            let mut request = client
                .post(&self.config.endpoint.url)
                .json(&payload);

            if !self.config.endpoint.api_key.trim().is_empty() {
                request = request.bearer_auth(&self.config.endpoint.api_key);
            }

            let response = request
                .send()
                .await?
                .error_for_status()?;

            let response_json = response.json::<Value>().await?;

            let response_text = providers::extract_response(
                &self.config.endpoint,
                &response_json,
            )?;

            save_chat_log_message(
                &self.home_dir,
                "assistant",
                &response_text,
            ).await?;

            let tags = self.config.tool_tags.clone();

            // Preserve the EXACT assistant turn in live model history.
            // Tool tags are intentionally kept here so inference context matches
            // the format used during training.
            self.messages.push(json!({
                "role": "assistant",
                "content": &response_text
            }));

            // Parse ONLY this newly generated assistant turn. The parser returns
            // the last complete valid tool call in the turn, regardless of type.
            if let Some(tool_call) = extract_last_tool_call(&response_text, &tags) {
                // Hide only the actionable tool call from terminal display.
                // The raw tagged turn remains untouched in self.messages above.
                let visible = remove_tool_call_for_display(&response_text, &tool_call);

                if !visible.trim().is_empty() {
                    println!(
                        "{}Echo:\n{}\n{}",
                        LIGHT_BLUE,
                        visible.trim(),
                        RESET_COLOR
                    );
                }

                // Soft loop detection: repeated identical calls are still allowed,
                // but Echo receives a hint to reassess instead of blindly retrying.
                let tool_signature = tool_call_signature(&tool_call.kind);
                if let Some(recovery_hint) = self.note_tool_call(&tool_signature) {
                    self.push_tool_result(&recovery_hint);
                    save_chat_log_message(
                        &self.home_dir,
                        &self.config.messages.tool_role_name,
                        &recovery_hint,
                    ).await?;
                }

                match tool_call.kind {
                    ParsedToolCallKind::Command(command) => {
                        crate::commands::handle_command(
                            self,
                            user_input,
                            &command,
                        ).await?;
                    }

                    ParsedToolCallKind::Session { name, command } => {
                        crate::sessions::handle_session_command(
                            self,
                            user_input,
                            &name,
                            Some(&command),
                        ).await?;
                    }

                    ParsedToolCallKind::EndSession(name) => {
                        crate::sessions::handle_session_command(
                            self,
                            user_input,
                            &name,
                            None,
                        ).await?;
                    }

                    ParsedToolCallKind::Json(json_content) => {
                        crate::json::handle_json_tool(
                            self,
                            user_input,
                            &response_text,
                            &json_content,
                        ).await?;
                    }

                    ParsedToolCallKind::Cleanup => {
                        handle_cleanup(self, user_input).await?;
                    }
                }

                continue;
            }

            // No tool call in this turn: this is the final assistant response.
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

    /// Record a tool call signature and return a soft recovery hint when
    /// the same exact tool call is repeated consecutively. Nothing is blocked.
    pub fn note_tool_call(&mut self, signature: &str) -> Option<String> {
        match &self.last_tool_signature {
            Some(previous) if previous == signature => {
                self.repeated_tool_call_count =
                    self.repeated_tool_call_count.saturating_add(1);
            }
            _ => {
                self.last_tool_signature = Some(signature.to_string());
                self.repeated_tool_call_count = 1;
            }
        }

        if self.repeated_tool_call_count == 2 {
            Some(
                "Recovery guidance: You have issued the exact same tool call twice in a row. \
If the previous result showed an error, no useful progress, or the same failed outcome, stop and reason about the result before repeating it again. \
Try a different command, tool, parameter, or approach when appropriate. \
If repetition is intentional (for example, polling or verifying a changed state), you may continue."
                    .to_string(),
            )
        } else {
            None
        }
    }

    /// Clear duplicate-tool state at a human/task boundary.
    pub fn reset_tool_repeat_tracker(&mut self) {
        self.last_tool_signature = None;
        self.repeated_tool_call_count = 0;
    }

    pub fn push_tool_result(&mut self, content: &str) {
        if self.pending_background_output.is_empty() {
            self.messages.push(json!({
                "role": &self.config.messages.tool_role_name,
                "content": content
            }));
        } else {
            let extra = self.pending_background_output.join("\n\n---\n\n");
            self.pending_background_output.clear();

            self.messages.push(json!({
                "role": &self.config.messages.tool_role_name,
                "content": format!("{}\n\n---\n\n{}", content, extra)
            }));
        }
    }
}

fn tool_call_signature(call: &ParsedToolCallKind) -> String {
    match call {
        ParsedToolCallKind::Command(command) => {
            format!("command:{}", command.trim())
        }
        ParsedToolCallKind::Session { name, command } => {
            format!("session:{}:{}", name.trim(), command.trim())
        }
        ParsedToolCallKind::EndSession(name) => {
            format!("end_session:{}", name.trim())
        }
        ParsedToolCallKind::Json(json_content) => {
            format!("json:{}", json_content.trim())
        }
        ParsedToolCallKind::Cleanup => "cleanup".to_string(),
    }
}
