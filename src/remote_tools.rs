use std::collections::HashMap;
use serde::Deserialize;
use anyhow::Result;

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteToolDefinition {
    pub name: String,
    pub description: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteToolListResponse {
    pub tools: Vec<RemoteToolDefinition>,
}

pub async fn fetch_remote_tools(
    base_url: &str,
    auth_token: &str,
    instance_id: &str,
) -> Result<RemoteToolListResponse> {
    let url = format!("{}/tools", base_url.trim_end_matches('/'));

    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(auth_token)
        .header("x-instance-id", instance_id)
        .send()
        .await?
        .error_for_status()?;

    let tools = response
        .json::<RemoteToolListResponse>()
        .await?;

    Ok(tools)
}

pub async fn call_remote_tool(
    base_url: &str,
    auth_token: &str,
    instance_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Result<String> {
    let url = format!("{}/execute", base_url.trim_end_matches('/'));

    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(auth_token)
        .header("x-instance-id", instance_id)
        .json(&serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        }))
        .send()
        .await?
        .error_for_status()?;

    let body: serde_json::Value = response.json().await?;

    if let Some(result) = body["result"].as_str() {
        Ok(result.to_string())
    } else if let Some(error) = body["error"].as_str() {
        Err(anyhow::anyhow!(error.to_string()))
    } else {
        Err(anyhow::anyhow!("Invalid response from remote tool server"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct RemoteToolRegistry {
    tools: HashMap<String, RemoteToolDefinition>,
}

impl RemoteToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }


    pub fn prompt_text(&self) -> String {
        if self.tools.is_empty() {
            return String::new();
        }

        let mut lines = Vec::new();

        for tool in self.tools.values() {
            lines.push(format!(
                "{}({}) - {}",
                tool.name,
                tool.arguments,
                tool.description
            ));
        }

        format!(
            "### Remote JSON Tools\n\n{}\n\n\
    Call these tools using the normal JSON tool format:\n\
    <json>{{\"name\":\"tool_name\",\"arguments\":{{...}}}}</json>",
            lines.join("\n")
        )
    }

    pub fn load_tools(&mut self, tools: Vec<RemoteToolDefinition>) {
        self.tools.clear();

        for tool in tools {
            self.insert(tool);
        }
    }

    pub fn insert(&mut self, tool: RemoteToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn contains(&self, tool_name: &str) -> bool {
        self.tools.contains_key(tool_name)
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }
}
