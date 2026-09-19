use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    pub bind_address: String,
    pub auth_token: String,
}

#[derive(Debug, Deserialize)]
pub struct ToolServerConfig {
    pub server: ServerSection,
}

pub fn load_config(path: &str) -> Result<ToolServerConfig, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: ToolServerConfig = toml::from_str(&content)?;
    Ok(config)
}
