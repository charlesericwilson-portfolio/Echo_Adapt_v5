use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct EndpointConfig {
    pub url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
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

#[derive(Debug, Deserialize)]
pub struct SummarizerConfig {
    pub url: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct PromptsConfig {
    pub main_system: String,
    pub summarizer: String,
}

#[derive(Debug, Deserialize)]
pub struct SecurityConfig {
    pub denylist: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContextConfig {
    pub summarize_threshold: usize,
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
    #[serde(default)]
    pub json_tools: JsonToolsConfig,
}

pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}
