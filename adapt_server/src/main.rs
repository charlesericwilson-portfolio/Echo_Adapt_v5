use std::{
    net::SocketAddr,
    num::NonZeroU32,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use encoding_rs::UTF_8;
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{
        params::LlamaModelParams,
        AddBos,
        LlamaChatMessage,
        LlamaChatTemplate,
        LlamaModel,
    },
    sampling::LlamaSampler,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

// Configuration

#[cfg(all(feature = "rocm", feature = "cuda"))]
compile_error!("Enable only one GPU backend: `rocm` or `cuda`.");

#[derive(Parser, Debug)]
#[command(name = "adapt_server")]
#[command(about = "Minimal GGUF inference server for ADAPT")]
struct Args {
    /// Path to the TOML configuration file
    #[arg(long, default_value = "config.toml")]
    config: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    server: ServerConfig,
    model: ModelConfig,
    generation: GenerationConfig,

    #[serde(default)]
    output: OutputConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerConfig {
    bind: SocketAddr,

    #[serde(default)]
    debug_requests: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelConfig {
    path: String,
    context_size: u32,
    batch_size: usize,
    gpu_layers: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct GenerationConfig {
    max_tokens: usize,
    temperature: f32,
    top_p: f32,
    top_k: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct OutputConfig {
    #[serde(default = "default_true")]
    strip_reasoning: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            strip_reasoning: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn load_config(path: &str) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {path}"))?;

    let config: Config = toml::from_str(&text)
        .with_context(|| format!("failed to parse config file: {path}"))?;

    if config.model.context_size == 0 {
        bail!("model.context_size must be greater than zero");
    }

    if config.model.batch_size == 0 {
        bail!("model.batch_size must be greater than zero");
    }

    if config.generation.max_tokens == 0 {
        bail!("generation.max_tokens must be greater than zero");
    }

    Ok(config)
}

// OpenAI-compatible request / response

#[derive(Debug, Clone, Deserialize)]
struct ChatRequest {
    #[serde(default)]
    _model: Option<String>,

    messages: Vec<Message>,

    #[serde(default)]
    max_tokens: Option<usize>,

    #[serde(default)]
    temperature: Option<f32>,

    #[serde(default)]
    top_p: Option<f32>,

    #[serde(default)]
    top_k: Option<i32>,

    #[serde(default)]
    stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
struct Choice {
    index: usize,
    message: AssistantMessage,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
struct AssistantMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct Usage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

// Runtime

struct Runtime {
    // backend MUST outlive model/context
    backend: LlamaBackend,
    model: LlamaModel,
    template: LlamaChatTemplate,

    model_name: String,
    ctx_size: u32,
    batch_size: usize,
    strip_reasoning_output: bool,

    default_max_tokens: usize,
    default_temperature: f32,
    default_top_p: f32,
    default_top_k: i32,
}

struct GenerationResult {
    raw: String,
    clean: String,
    prompt_tokens: usize,
    completion_tokens: usize,
}

impl Runtime {
    fn load(config: &Config) -> Result<Self> {
        println!("Initializing llama.cpp backend...");

        let backend =
            LlamaBackend::init().context("failed to initialize llama.cpp backend")?;

        println!("Loading GGUF: {}", config.model.path);

        let model_params =
            LlamaModelParams::default().with_n_gpu_layers(config.model.gpu_layers);

        let model = LlamaModel::load_from_file(
            &backend,
            &config.model.path,
            &model_params,
        )
        .context("failed to load GGUF model")?;

        // Use the GGUF's native chat template. The server does not reinterpret
        // Harmony, ChatML, native tool calls, or other model-specific formats.
        let template = model
            .chat_template(None)
            .context("GGUF has no usable native chat template")?;

        let model_name = std::path::Path::new(&config.model.path)
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("local-gguf")
            .to_string();

        println!("Model loaded: {model_name}");
        println!("Context: {}", config.model.context_size);
        println!("Batch size: {}", config.model.batch_size);
        println!("GPU layers: {}", config.model.gpu_layers);

        Ok(Self {
            backend,
            model,
            template,

            model_name,
            ctx_size: config.model.context_size,
            batch_size: config.model.batch_size,
            strip_reasoning_output: config.output.strip_reasoning,

            default_max_tokens: config.generation.max_tokens,
            default_temperature: config.generation.temperature,
            default_top_p: config.generation.top_p,
            default_top_k: config.generation.top_k,
        })
    }

    fn generate(&mut self, request: &ChatRequest) -> Result<GenerationResult> {
        if request.stream.unwrap_or(false) {
            bail!("stream=true is not implemented yet");
        }

        // Build chat using the model's native template.

        let mut chat = Vec::with_capacity(request.messages.len());

        for msg in &request.messages {
            match msg.role.as_str() {
                "system" | "user" | "assistant" | "tool" => {}
                other => bail!("unsupported role: {other}"),
            }

            chat.push(
                LlamaChatMessage::new(
                    msg.role.clone(),
                    msg.content.clone(),
                )
                .context("invalid chat message")?
            );
        }

        let prompt = self
            .model
            .apply_chat_template(
                &self.template,
                &chat,
                true,
            )
            .context("failed to apply model chat template")?;

        // The chat template already contains its control/BOS structure,
        // so do not inject another BOS token.

        let prompt_tokens = self
            .model
            .str_to_token(
                &prompt,
                AddBos::Never,
            )
            .context("tokenization failed")?;

        let max_tokens = request
            .max_tokens
            .unwrap_or(self.default_max_tokens);

        let total_needed = prompt_tokens.len() + max_tokens;

        if total_needed > self.ctx_size as usize {
            bail!(
                "request exceeds context: prompt={} generation={} ctx={}",
                prompt_tokens.len(),
                max_tokens,
                self.ctx_size
            );
        }

        // Fresh context per request; ADAPT owns conversation history.

        let ctx_size = NonZeroU32::new(self.ctx_size)
            .ok_or_else(|| anyhow!("ctx_size cannot be zero"))?;

        let ctx_params =
            LlamaContextParams::default()
                .with_n_ctx(Some(ctx_size));

        let mut ctx = self
            .model
            .new_context(
                &self.backend,
                ctx_params,
            )
            .context("failed to create inference context")?;

        // Decode the prompt in chunks no larger than the configured batch size.

       let last =
            prompt_tokens
                .len()
                .checked_sub(1)
                .ok_or_else(|| anyhow!("empty prompt"))?;

        let mut batch =
            LlamaBatch::new(self.batch_size, 1);

        for (chunk_index, chunk) in
            prompt_tokens.chunks(self.batch_size).enumerate()
        {
            batch.clear();

            let chunk_start = chunk_index * self.batch_size;

            for (offset, token) in
                chunk.iter().copied().enumerate()
            {
                let position = chunk_start + offset;

                batch.add(
                    token,
                    position as i32,
                    &[0],
                    position == last,
                )?;
            }

            ctx.decode(&mut batch)
                .context("failed to decode prompt chunk")?;
        }

        // Sampling

        let temperature = request
            .temperature
            .unwrap_or(self.default_temperature);

        let top_p = request
            .top_p
            .unwrap_or(self.default_top_p);

        let top_k = request
            .top_k
            .unwrap_or(self.default_top_k);

        let mut sampler =
            LlamaSampler::chain_simple([
                LlamaSampler::top_k(top_k),
                LlamaSampler::top_p(top_p, 1),
                LlamaSampler::temp(temperature),
                LlamaSampler::dist(0xC0FFEE),
            ]);

        // Generate raw model output only. Tool parsing, Harmony handling,
        // JSON conversion, and function execution stay outside inference.

        let mut decoder = UTF_8.new_decoder();

        let mut raw = String::new();
        let mut generated = 0usize;

        let mut position = prompt_tokens.len() as i32;

        while generated < max_tokens {
            let token =
                sampler.sample(
                    &ctx,
                    batch.n_tokens() - 1,
                );

            sampler.accept(token);

            // Honor normal EOG behavior; never reinterpret EOG as a tool call.
            if self.model.is_eog_token(token) {
                break;
            }

            // Keep special tokens visible internally so the sanitizer decides
            // what leaves the server.
            let piece = self
                .model
                .token_to_piece(
                    token,
                    &mut decoder,
                    true,
                    None,
                )
                .context("failed to decode generated token")?;

            raw.push_str(&piece);

            batch.clear();

            batch.add(
                token,
                position,
                &[0],
                true,
            )?;

            ctx.decode(&mut batch)
                .context("generation decode failed")?;

            position += 1;
            generated += 1;
        }

        // Normalize model output before returning it to ADAPT.

        let clean = if self.strip_reasoning_output {
            strip_reasoning(&raw)
        } else {
            raw.trim().to_string()
        };

        Ok(GenerationResult {
            raw,
            clean,
            prompt_tokens: prompt_tokens.len(),
            completion_tokens: generated,
        })
    }
}

// Reasoning stripper

fn strip_reasoning(raw: &str) -> String {
    // For GPT-OSS/Harmony, prefer the explicitly marked final channel.

    const FINAL_MARKER: &str =
        "<|channel|>final<|message|>";

    if let Some(pos) = raw.rfind(FINAL_MARKER) {
        let final_text =
            &raw[pos + FINAL_MARKER.len()..];

        return clean_control_tokens(final_text);
    }

    // Remove complete standard reasoning blocks.

    let mut text = raw.to_string();

    let patterns = [
        r"(?s)<think>.*?</think>",
        r"(?s)<reasoning>.*?</reasoning>",
        r"(?s)<analysis>.*?</analysis>",

        // Remove only complete Harmony analysis channels. Never extract
        // ADAPT commands from reasoning.
        r"(?s)<\|channel\|>analysis<\|message\|>.*?<\|end\|>",
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            text = re.replace_all(&text, "").to_string();
        }
    }

    clean_control_tokens(&text)
}

fn clean_control_tokens(text: &str) -> String {
    let mut out = text.to_string();

    // Remove only model-envelope markers; preserve ADAPT tool tags.

    let markers = [
        "<|start|>assistant",
        "<|start|>",
        "<|channel|>final",
        "<|message|>",
        "<|end|>",
        "<|return|>",
        "<|flush|>",
    ];

    for marker in markers {
        out = out.replace(marker, "");
    }

    // Safety net for complete think blocks left after control-token cleanup.

    let think_re =
        Regex::new(r"(?s)<think>.*?</think>")
            .expect("valid regex");

    out = think_re
        .replace_all(&out, "")
        .to_string();

    out.trim().to_string()
}

// HTTP state

#[derive(Clone)]
struct AppState {
    runtime: Arc<Mutex<Runtime>>,
    debug_requests: bool,
}

async fn index() -> Html<&'static str> {
    Html(r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ADAPT GGUF Chat</title>
<style>
body {
    margin: 0;
    font-family: system-ui, sans-serif;
    background: #111;
    color: #eee;
    display: flex;
    flex-direction: column;
    height: 100vh;
}
header {
    padding: 12px 16px;
    border-bottom: 1px solid #333;
    font-weight: 600;
}
#chat {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
}
.msg {
    max-width: 900px;
    margin: 0 auto 12px;
    white-space: pre-wrap;
    line-height: 1.45;
}
.user { color: #9ecbff; }
.assistant { color: #eee; }
#composer {
    border-top: 1px solid #333;
    padding: 12px;
}
#row {
    max-width: 900px;
    margin: 0 auto;
    display: flex;
    gap: 8px;
}
textarea {
    flex: 1;
    min-height: 72px;
    resize: vertical;
    background: #1b1b1b;
    color: #eee;
    border: 1px solid #444;
    border-radius: 6px;
    padding: 10px;
}
button {
    background: #2b6cb0;
    color: white;
    border: 0;
    border-radius: 6px;
    padding: 0 16px;
    cursor: pointer;
}
button:disabled { opacity: .5; cursor: default; }
#controls {
    max-width: 900px;
    margin: 8px auto 0;
    display: flex;
    gap: 12px;
    align-items: center;
    font-size: 13px;
    color: #aaa;
}
input {
    width: 90px;
    background: #1b1b1b;
    color: #eee;
    border: 1px solid #444;
    border-radius: 4px;
    padding: 4px 6px;
}
</style>
</head>
<body>
<header>ADAPT GGUF Chat</header>

<div id="chat"></div>

<div id="composer">
    <div id="row">
        <textarea id="input" placeholder="Message the model..."></textarea>
        <button id="send">Send</button>
    </div>
    <div id="controls">
        <label>Temperature <input id="temperature" type="number" min="0" max="2" step="0.05" value="0.7"></label>
        <label>Max tokens <input id="max_tokens" type="number" min="1" step="1" value="2048"></label>
        <button id="clear" type="button">Clear chat</button>
    </div>
</div>

<script>
const chat = document.getElementById("chat");
const input = document.getElementById("input");
const send = document.getElementById("send");
const clear = document.getElementById("clear");
const temperature = document.getElementById("temperature");
const maxTokens = document.getElementById("max_tokens");

let messages = [];

function addMessage(role, content) {
    const div = document.createElement("div");
    div.className = `msg ${role}`;
    div.textContent = `${role === "user" ? "You" : "Assistant"}:\n${content}`;
    chat.appendChild(div);
    chat.scrollTop = chat.scrollHeight;
}

async function submit() {
    const content = input.value.trim();
    if (!content || send.disabled) return;

    messages.push({ role: "user", content });
    addMessage("user", content);
    input.value = "";
    send.disabled = true;

    try {
        const response = await fetch("/v1/chat/completions", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
                model: "local",
                messages,
                temperature: Number(temperature.value),
                max_tokens: Number(maxTokens.value)
            })
        });

        const data = await response.json();

        if (!response.ok) {
            throw new Error(data?.error?.message || `HTTP ${response.status}`);
        }

        const content = data?.choices?.[0]?.message?.content ?? "";
        messages.push({ role: "assistant", content });
        addMessage("assistant", content || "[empty response]");
    } catch (err) {
        addMessage("assistant", `Error: ${err.message}`);
    } finally {
        send.disabled = false;
        input.focus();
    }
}

send.addEventListener("click", submit);

input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        submit();
    }
});

clear.addEventListener("click", () => {
    messages = [];
    chat.innerHTML = "";
    input.focus();
});
</script>
</body>
</html>"#)
}

// Endpoints

async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "server": "adapt-gguf-server"
    }))
}

async fn models(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let runtime = state.runtime.lock().unwrap();

    Json(json!({
        "object": "list",
        "data": [
            {
                "id": runtime.model_name,
                "object": "model",
                "owned_by": "local"
            }
        ]
    }))
}

async fn chat_completions(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if state.debug_requests {
        let raw = String::from_utf8_lossy(&body);

        eprintln!("\n========== RAW REQUEST ==========");
        eprintln!("{raw}");
        eprintln!("=================================\n");
    }

    let request: ChatRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": err.to_string(),
                        "type": "bad_request"
                    }
                })),
            )
                .into_response();
        }
    };

    let runtime = state.runtime.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut runtime = runtime
            .lock()
            .map_err(|_| anyhow!("runtime mutex poisoned"))?;

        runtime.generate(&request)
    })
    .await;

    // llama.cpp inference is blocking, so keep it off Tokio's async executor.
    // The mutex intentionally serializes generation for now.

    let generation = match result {
        Ok(Ok(value)) => value,

        Ok(Err(err)) => {
            tracing::error!("generation error: {err:#}");

            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": err.to_string(),
                        "type": "inference_error"
                    }
                })),
            )
                .into_response();
        }

        Err(err) => {
            tracing::error!("worker join error: {err}");

            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": err.to_string(),
                        "type": "server_error"
                    }
                })),
            )
                .into_response();
        }
    };

    // Raw model output stays server-side; ADAPT receives only cleaned output.
    tracing::debug!(
        raw_model_output = %generation.raw,
        clean_model_output = %generation.clean,
        "generation complete"
    );

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let runtime = state.runtime.lock().unwrap();

    let response = ChatResponse {
        id: format!(
            "chatcmpl-{}",
            Uuid::new_v4().simple()
        ),

        object: "chat.completion".to_string(),

        created,

        model: runtime.model_name.clone(),

        choices: vec![
            Choice {
                index: 0,

                message: AssistantMessage {
                    role: "assistant".to_string(),

                    // ADAPT receives only cleaned assistant content.
                    content: generation.clean,
                },

                finish_reason: "stop".to_string(),
            }
        ],

        usage: Usage {
            prompt_tokens:
                generation.prompt_tokens,

            completion_tokens:
                generation.completion_tokens,

            total_tokens:
                generation.prompt_tokens
                + generation.completion_tokens,
        },
    };

    Json(response).into_response()
}

// Main

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .init();

    let args = Args::parse();

    let config = load_config(&args.config)?;
    let bind = config.server.bind;

    let runtime =
        Runtime::load(&config)?;

    let state = AppState {
        runtime: Arc::new(
            Mutex::new(runtime)
        ),
        debug_requests: config.server.debug_requests,
    };

    let app = Router::new()
        .route(
            "/",
            get(index),
        )
        .route(
            "/health",
            get(health),
        )
        .route(
            "/v1/models",
            get(models),
        )
        .route(
            "/v1/chat/completions",
            post(chat_completions),
        )
        .with_state(state);

    let listener =
        tokio::net::TcpListener::bind(bind)
            .await
            .with_context(|| {
                format!("failed to bind {bind}")
            })?;

    println!();
    println!("ADAPT GGUF server ready");
    println!("Listening: http://{bind}");
    println!("POST /v1/chat/completions");
    println!();

    axum::serve(listener, app)
        .await
        .context("HTTP server failed")?;

    Ok(())
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_think_tags() {
        let raw =
            "<think>I am thinking</think><command>whoami</command>";

        assert_eq!(
            strip_reasoning(raw),
            "<command>whoami</command>"
        );
    }

    #[test]
    fn strips_reasoning_tags() {
        let raw =
            "<reasoning>secret thought</reasoning>Hello";

        assert_eq!(
            strip_reasoning(raw),
            "Hello"
        );
    }

    #[test]
    fn strips_gpt_oss_analysis_channel() {
        let raw =
            "<|channel|>analysis<|message|>\
Need to greet the user.\
<|end|>\
<|start|>assistant<|channel|>final<|message|>\
Hello Charles!";

        assert_eq!(
            strip_reasoning(raw),
            "Hello Charles!"
        );
    }

    #[test]
    fn preserves_adapt_command() {
        let raw =
            "<|channel|>analysis<|message|>\
Need to execute whoami.\
<|end|>\
<|start|>assistant<|channel|>final<|message|>\
<command>whoami</command>";

        assert_eq!(
            strip_reasoning(raw),
            "<command>whoami</command>"
        );
    }

    #[test]
    fn never_executes_command_mentioned_in_reasoning() {
        let raw =
            "<|channel|>analysis<|message|>\
I should emit <command>rm something</command>, but I am still thinking.\
<|end|>\
<|start|>assistant<|channel|>final<|message|>\
I need clarification.";

        assert_eq!(
            strip_reasoning(raw),
            "I need clarification."
        );
    }
}
