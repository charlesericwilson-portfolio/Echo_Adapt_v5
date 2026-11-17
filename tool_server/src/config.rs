use serde::Deserialize;
use std::{collections::HashMap, fs};

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    pub bind_address: String,
    pub auth_token: String,
}

#[derive(Debug, Deserialize)]
pub struct GlobalSection {
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct InstanceSection {
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TavilySection {
    pub url: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct McpSection {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub servers: HashMap<String, McpServerSection>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct McpServerSection {
    pub command: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EndpointConfig {
    pub provider: String,
    pub url: String,
    pub model: String,

    #[serde(default)]
    pub api_key: String,

    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct ToolServerConfig {
    pub server: ServerSection,
    pub global: GlobalSection,
    pub instances: HashMap<String, InstanceSection>,
    pub tavily: TavilySection,

    #[serde(default)]
    pub mcp: McpSection,

    #[serde(default)]
    pub oracle: Option<EndpointConfig>,
}

pub fn load_config(path: &str) -> Result<ToolServerConfig, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: ToolServerConfig = toml::from_str(&content)?;
    Ok(config)
}
