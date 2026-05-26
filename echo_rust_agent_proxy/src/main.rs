// main.rs
use std::io::{self, Write};
use std::process::Command;
use std::path::PathBuf;
use tokio::signal::unix::{signal, SignalKind};
use dirs_next as dirs;
use serde_json::{self, Value, json};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use std::collections::HashMap;
use anyhow::Result as AnyhowResult;
mod json;
mod db;
use db::ToolDatabase;

// ANSI color codes
pub const LIGHT_BLUE: &str = "\x1b[94m";
pub const YELLOW: &str = "\x1b[33m";
pub const RESET_COLOR: &str = "\x1b[0m";

// Constants
//const MODEL_NAME: &str = "Echo";
//const API_URL: &str = "http://localhost:8080/v1/chat/completions";

pub static ACTIVE_SESSIONS: Lazy<Mutex<HashMap<String, (String, String)>>> = Lazy::new(|| Mutex::new(HashMap::new()));
pub static SHUTDOWN_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static STOP_GENERATION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static CONFIG: Lazy<config::Config> = Lazy::new(|| {
    config::load_config("config.toml").expect("Failed to load config.toml")
});

#[tokio::main]
async fn main() -> AnyhowResult<()> {
    println!("Echo Rust Wrapper v2 – Async Tool Calls with Named Pipes");
    println!("Type 'quit' or 'exit' to stop.\n");
       // Handle graceful shutdowns + generation interrupt
   let mut quit = signal(SignalKind::quit()).expect("Failed to set up SIGQUIT handler");

    tokio::spawn(async move {
    while quit.recv().await.is_some() {
        STOP_GENERATION.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });

    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/eric/Documents"));
    let context_path = PathBuf::from("/home/eric/echo/Echo_rag/Echo-context.txt");
    let mut context_content = String::new();

    let db = ToolDatabase::new(PathBuf::from("echo_tools.db"))?;

    if tokio::fs::metadata(&context_path).await.is_ok() {
        context_content = tokio::fs::read_to_string(&context_path)
            .await
            .expect("Failed to read context file");
        println!("✅ Loaded context file: {}", context_path.display());
    } else {
        println!("⚠️ Context file not found at: {}", context_path.display());
    }

    tokio::fs::create_dir_all(home_dir.join("Documents"))
        .await
        .expect("Failed to create Documents dir");

    let main_prompt = tokio::fs::read_to_string(&CONFIG.prompts.main_system)
    .await
    .expect("Failed to read main system prompt");

    let full_system_prompt = format!("{}\n\n{}", main_prompt.trim(), context_content.trim());

    let mut messages = vec![
        json!({"role": "system", "content": full_system_prompt}),
    ];

    println!("Echo: Ready. Type 'quit' or 'exit' to end session.\n");

    loop {
        print!("You: ");
        io::stdout().flush()?;
        let mut user_input = String::new();
        io::stdin()
            .read_line(&mut user_input)
            .expect("Failed to read line");
        let trimmed_input = user_input.trim();

        // Exit check
        if trimmed_input.eq_ignore_ascii_case("quit") || trimmed_input.eq_ignore_ascii_case("exit") {
            println!("Session ended.");
            save_chat_log_entry(&home_dir, "", "", "SESSION_END").await.unwrap();
            break;
        }

        if SHUTDOWN_REQUESTED.load(std::sync::atomic::Ordering::SeqCst) {
            println!("\nGraceful shutdown initiated...");
            clean_up_sessions().await?;
            println!("All sessions terminated. Goodbye!");
            return Ok(());
        }

        messages.push(json!({
            "role": "user",
            "content": trimmed_input,
        }));

       let payload = json!({
            "model": CONFIG.endpoint.model,
            "messages": &messages,
            "temperature": CONFIG.endpoint.temperature,
            "max_tokens": CONFIG.endpoint.max_tokens
        });

        let response_text = tokio::select! {
            biased;

            _ = async {
                loop {
                    if STOP_GENERATION.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
            } => {
                STOP_GENERATION.store(false, std::sync::atomic::Ordering::SeqCst);
                "[Generation stopped by user]".to_string()
            }

            result = async {
                reqwest::Client::new()
                    .post(&CONFIG.endpoint.url)
                    .header("Content-Type", "application/json")
                    .json(&payload)
                    .send()
                    .await
            } => {
                match result {
                    Ok(res) => {
                        if res.status().is_success() {
                            let body_str = res.text().await.unwrap_or_default();
                            match serde_json::from_str::<Value>(&body_str) {
                                Ok(parsed) => parsed["choices"][0]["message"]["content"]
                                    .as_str()
                                    .unwrap_or("")
                                    .trim()
                                    .to_string(),
                                Err(_) => "Invalid JSON from API response.".to_string(),
                            }
                        } else {
                            format!("API request failed with status: {}", res.status())
                        }
                    }
                    Err(e) => format!(
                        "Request to {} failed: {}. Is your local model server running?",
                        CONFIG.endpoint.url, e
                    ),
                }
            }
        };

        STOP_GENERATION.store(false, std::sync::atomic::Ordering::SeqCst);

        // === TOOL CALL DETECTION + AUTONOMOUS CHAINING ===
        let mut current_response = response_text;

        loop {
             if let Some(json_content) = extract_json_tool(&current_response) {
                println!("{}Echo: Detected JSON tool call{}", LIGHT_BLUE, RESET_COLOR);
                println!("{}Echo: {}", LIGHT_BLUE, current_response.trim());
                save_chat_log_entry(&home_dir, trimmed_input, &current_response, "assistant").await.unwrap();
                messages.push(json!({"role": "assistant", "content": current_response.clone()}));

                match handle_json_tool_call_str(&json_content).await {
                    Ok(result) => {
                        let tool_content = format!("Tool output:\n{}", result);
                        save_chat_log_entry(&home_dir, trimmed_input, &tool_content, "assistant").await.unwrap();
                        messages.push(json!({"role": "tool", "content": tool_content}));
                    }
                    Err(e) => {
                        let error_msg = format!("JSON Tool error: {}", e);
                        messages.push(json!({"role": "tool", "content": error_msg}));
                    }
                }

          } else if let Some((session_name, command)) = extract_session_command(&current_response) {
                println!("{}Echo: {}", LIGHT_BLUE, current_response.trim());
                save_chat_log_entry(&home_dir, trimmed_input, &current_response, "assistant").await.unwrap();
                messages.push(json!({"role": "assistant", "content": current_response.clone()}));
                println!("{}Echo: Creating/reusing session '{}' and running '{}'.{}", LIGHT_BLUE, &session_name, &command, RESET_COLOR);

                if let Err(e) = is_command_safe(&command) {
                    println!("{}Echo: {}", LIGHT_BLUE, current_response.trim());
                    println!("{}Safety block: {}{}", YELLOW, e, RESET_COLOR);
                    save_chat_log_entry(&home_dir, trimmed_input, &format!("Blocked: {}", e), "assistant").await.unwrap();
                    messages.push(json!({"role": "assistant", "content": format!("Safety block: {}", e)}));
                } else {
                    start_or_reuse_session(home_dir.clone(), &session_name, &command).await?;
                    let raw_output = execute_in_session(home_dir.clone(), &session_name, command.to_string()).await?;

                    let summary = match summarize_output(&raw_output).await {
                        Ok(s) => s,
                        Err(e) => {
                            println!("{}Summarizer failed: {}{}", YELLOW, e, RESET_COLOR);
                            format!("(Summarizer failed: {})", e)
                        }
                    };

                    db.log_tool_call(&session_name, &command, &summary)?;

                    let tool_content = format!(
                        "Tool output from SESSION '{}':\n{}",
                        session_name, summary
                    );

                    println!("{}Echo: Session summary:\n{}{}", LIGHT_BLUE, summary, RESET_COLOR);

                    save_chat_log_entry(&home_dir, trimmed_input, &tool_content, "assistant").await.unwrap();

                    messages.push(json!({
                        "role": "assistant",
                        "content": format!("Executed command in session '{}'", session_name)
                    }));

                    messages.push(json!({
                        "role": "tool",
                        "content": tool_content
                    }));
                }

            } else if let Some((session_name, sub_command)) = extract_run_command(&current_response) {
                let full_cmd = format!("run {}", sub_command.trim());
                println!("{}Echo: {}", LIGHT_BLUE, current_response.trim());
                save_chat_log_entry(&home_dir, trimmed_input, &current_response, "assistant").await.unwrap();
                messages.push(json!({"role": "assistant", "content": current_response.clone()}));

                if let Err(e) = is_command_safe(&full_cmd) {
                    println!("{}Safety block: {}{}", YELLOW, e, RESET_COLOR);
                    save_chat_log_entry(&home_dir, trimmed_input, &format!("Blocked: {}", e), "assistant").await.unwrap();
                    messages.push(json!({"role": "assistant", "content": format!("Safety block: {}", e)}));
                } else {
                    let output = execute_in_session(home_dir.clone(), &session_name, full_cmd).await?;
                    let tool_content = format!("Tool output from SESSION '{}':\n{}", session_name, output);
                    save_chat_log_entry(&home_dir, trimmed_input, &tool_content, "assistant").await.unwrap();
                    messages.push(json!({"role": "tool", "content": tool_content}));
                }

            } else if let Some(session_name) = extract_end_command(&current_response) {
                println!("{}Echo: {}", LIGHT_BLUE, current_response.trim());
                save_chat_log_entry(&home_dir, trimmed_input, &current_response, "assistant").await.unwrap();
                messages.push(json!({"role": "assistant", "content": current_response.clone()}));

                let _ = end_session(home_dir.clone(), &session_name).await;
                let tool_content = format!("Session '{}' has been terminated.", session_name);
                save_chat_log_entry(&home_dir, trimmed_input, &tool_content, "assistant").await.unwrap();
                messages.push(json!({"role": "tool", "content": tool_content}));

            } else if let Some(command) = extract_command(&current_response) {
                println!("{}Echo: {}", LIGHT_BLUE, current_response.trim());
                println!("{}Echo: Executing command:{}\n{}\n{}", LIGHT_BLUE, RESET_COLOR, command.trim(), RESET_COLOR);
                save_chat_log_entry(&home_dir, trimmed_input, &current_response, "assistant").await.unwrap();
                messages.push(json!({"role": "assistant", "content": current_response.clone()}));

                if let Err(e) = is_command_safe(&command) {
                    println!("{}Safety block: {}{}", YELLOW, e, RESET_COLOR);
                    save_chat_log_entry(&home_dir, trimmed_input, &format!("Blocked: {}", e), "assistant").await.unwrap();
                    messages.push(json!({"role": "assistant", "content": format!("Safety block: {}", e)}));
                } else {
                    let output_cmd = Command::new("sh")
                        .arg("-c")
                        .arg(command.trim())
                        .output()
                        .expect("Failed to execute command");

                    let stdout = String::from_utf8_lossy(&output_cmd.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();

                    db.log_tool_call("COMMAND", &command.trim(), &format!("STDOUT:\n{}\nSTDERR:\n{}", stdout.trim(), stderr.trim()))?;

                    if !stdout.is_empty() {
                        println!("{}Echo:\n{}\n{}", LIGHT_BLUE, &stdout.trim(), RESET_COLOR);
                    }
                    if !stderr.is_empty() {
                        println!("{}Errors/Warnings:\n{}\n---", YELLOW, &stderr.trim());
                    }

                    let tool_content = format!(
                        "Tool output from COMMAND '{}':\nReturn code: {}\nSTDOUT:\n{}\nSTDERR:\n{}\nUse this to decide next suggestion.",
                        command.trim(),
                        output_cmd.status.code().unwrap_or(-1),
                        stdout,
                        stderr
                    );

                    save_chat_log_entry(&home_dir, trimmed_input, &tool_content, "assistant").await.unwrap();

                    messages.push(json!({
                        "role": "assistant",
                        "content": current_response
                    }));

                    messages.push(json!({
                        "role": "tool",
                        "content": tool_content
                    }));
                }

            } else {
                // No tool call — final answer
                println!("{}Echo:\n{}\n{}", LIGHT_BLUE, current_response.trim(), RESET_COLOR);

                save_chat_log_entry(&home_dir, trimmed_input, &current_response, "assistant").await.unwrap();

                messages.push(json!({
                    "role": "assistant",
                    "content": &current_response,
                }));

                let total_chars: usize = messages.iter()
                    .map(|m| m["content"].as_str().unwrap_or("").len())
                    .sum();

                if total_chars > CONFIG.context.summarize_threshold {
                    summarize_context(&mut messages).await?;
                }
                break;
            }

            // Call model again after tool result
            let payload = json!({
                "model": CONFIG.endpoint.model,
                "messages": &messages,
                "temperature": 0.7,
                "max_tokens": 2048
            });

            let next = reqwest::Client::new()
                .post(&CONFIG.endpoint.url)
                .json(&payload)
                .send()
                .await?
                .json::<Value>()
                .await?;

            current_response = next["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_string();
        }
    }

    clean_up_sessions().await?;
    println!("\nSession ended normally. Goodbye!");

    Ok(())
}

async fn summarize_context(messages: &mut Vec<Value>) -> anyhow::Result<()> {
    let _summary_prompt = "Summarize the entire conversation so far in a concise way. Keep key facts, decisions, and important details. Output ONLY the summary, nothing else.";

    let payload = json!({
        "model": &CONFIG.endpoint.model,
        "messages": messages.clone(),
        "temperature": CONFIG.endpoint.temperature,
        "max_tokens": CONFIG.endpoint.max_tokens
    });

    let response = reqwest::Client::new()
        .post(&CONFIG.endpoint.url)
        .json(&payload)
        .send()
        .await?
        .json::<Value>()
        .await?;

    let summary = response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("Summary failed.")
        .to_string();

    let last_turns: Vec<Value> = messages.iter().rev().take(4).cloned().collect();

    let mut new_messages = vec![
        messages[0].clone(),
        json!({"role": "assistant", "content": summary}),
    ];
    new_messages.extend(last_turns.into_iter().rev());

    *messages = new_messages;
    println!("{}[Context auto-summarized]{}", YELLOW, RESET_COLOR);

    Ok(())
}

async fn summarize_output(raw_output: &str) -> AnyhowResult<String> {
    let summarizer_prompt = tokio::fs::read_to_string(&CONFIG.prompts.summarizer)
        .await
        .expect("Failed to read summarizer prompt");

    let payload = json!({
        "model": "summarizer",
        "messages": [
            {
                "role": "system",
                "content": summarizer_prompt
            },
            {
                "role": "user",
                "content": raw_output
            }
        ],
        "temperature": 0.2,
        "max_tokens": 1500
    });

    let response = match reqwest::Client::new()
        .post(&CONFIG.summarizer.url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(res) => {
            if res.status().is_success() {
                let body = res.text().await.unwrap_or_default();
                match serde_json::from_str::<Value>(&body) {
                    Ok(parsed) => parsed["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("Summary failed.")
                        .trim()
                        .to_string(),
                    Err(_) => "Failed to parse summarizer response.".to_string(),
                }
            } else {
                format!("Summarizer returned status: {}", res.status())
            }
        }
        Err(e) => format!("Failed to connect to summarizer: {}", e),
    };

    Ok(response)
}

mod sessions;
mod log;
mod commands;
mod safety;
mod config;

use sessions::{start_or_reuse_session, execute_in_session, end_session, clean_up_sessions};
use log::save_chat_log_entry;
use commands::{extract_session_command, extract_run_command, extract_end_command, extract_command, extract_json_tool};
use safety::is_command_safe;
use json::handle_json_tool_call_str;
