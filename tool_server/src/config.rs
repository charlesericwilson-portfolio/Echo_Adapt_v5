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

#[derive(Debug, Deserialize)]
pub struct ToolServerConfig {
    pub server: ServerSection,
    pub global: GlobalSection,
    pub instances: HashMap<String, InstanceSection>,
    pub tavily: TavilySection,
}

pub fn load_config(path: &str) -> Result<ToolServerConfig, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: ToolServerConfig = toml::from_str(&content)?;
    Ok(config)
}
