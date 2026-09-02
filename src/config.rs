use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct ToolTagsConfig {
    #[serde(default = "default_json_open")]
    pub json_open: String,
    #[serde(default = "default_json_close")]
    pub json_close: String,
    #[serde(default = "default_command_open")]
    pub command_open: String,
    #[serde(default = "default_command_close")]
    pub command_close: String,
    #[serde(default = "default_session_open")]
    pub session_open: String,
    #[serde(default = "default_session_close")]
    pub session_close: String,
    #[serde(default = "default_end_session_open")]
    pub end_session_open: String,
}

fn default_json_open() -> String { "<json>".to_string() }
fn default_json_close() -> String { "</json>".to_string() }
fn default_command_open() -> String { "<command>".to_string() }
fn default_command_close() -> String { "</command>".to_string() }
fn default_session_open() -> String { "<session name=\"".to_string() }
fn default_session_close() -> String { "</session>".to_string() }
fn default_end_session_open() -> String { "<end_session name=\"".to_string() }

impl Default for ToolTagsConfig {
    fn default() -> Self {
        Self {
            json_open: default_json_open(),
            json_close: default_json_close(),
            command_open: default_command_open(),
            command_close: default_command_close(),
            session_open: default_session_open(),
            session_close: default_session_close(),
            end_session_open: default_end_session_open(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EndpointConfig {
    pub url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingsConfig {
    pub url: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct PathsConfig {
    pub home_dir: Option<String>,
    pub context_file: String,
    pub database: String,
    pub memory_file:String,
}
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct WebSearchConfig {
    pub url: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct JsonToolsConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SummarizerConfig {
    pub enabled: bool,
    pub url: String,
    pub model: String,
    #[serde(default = "default_max_raw_output_chars")]
    pub max_raw_output_chars: usize,
}

fn default_max_raw_output_chars() -> usize {
    6000
}

#[derive(Debug, Deserialize)]
pub struct PromptsConfig {
    pub main_system: String,
    pub summarizer: String,
}

#[derive(Debug, Deserialize)]
pub struct MessagesConfig {
    pub tool_role_name: String,
}

#[derive(Debug, Deserialize)]
pub struct SecurityConfig {
    pub denylist: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContextConfig {
    pub summarize_threshold: usize,
    pub max_turns: u32,
}


#[derive(Debug, Deserialize)]
pub struct Config {
    pub endpoint: EndpointConfig,
    pub summarizer: SummarizerConfig,
    pub prompts: PromptsConfig,
    pub security: SecurityConfig,
    pub context: ContextConfig,
    pub paths: PathsConfig,
    pub web_search: Option<WebSearchConfig>,
    pub embeddings: EmbeddingsConfig,
    #[serde(default)]
    pub json_tools: JsonToolsConfig,
    pub messages: MessagesConfig,
    #[serde(default)]
    pub tool_tags: ToolTagsConfig,

}

pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}
