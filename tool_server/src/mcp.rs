use anyhow::Result;
use rmcp::{
    model::{CallToolRequestParams, Tool},
    service::{RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use tokio::process::Command;
use crate::tools::ServerTool;
use serde_json::Value;
use crate::config::McpServerSection;
use std::collections::HashMap;

pub type McpClient = RunningService<RoleClient, ()>;

pub struct McpConnection {
    pub client: McpClient,
    pub tools: Vec<Tool>,
}

pub async fn connect_mcp_server(
    server: &McpServerSection,
) -> Result<McpConnection> {
    let transport = TokioChildProcess::new(
        Command::new(&server.command).configure(|cmd| {
            cmd.args(&server.args);

            for (key, value) in &server.env {
                cmd.env(key, value);
            }
        }),
    )?;

    let client = ().serve(transport).await?;

    let tools = client.list_all_tools().await?;

    Ok(McpConnection {
        client,
        tools,
    })
}

pub fn translate_mcp_tool(
    alias: &str,
    tool: &Tool,
) -> Result<ServerTool> {
    let normalized_name = format!("mcp.{}.{}", alias, tool.name);

    let description = tool
        .description
        .as_deref()
        .unwrap_or("MCP tool")
        .to_string();

    let arguments = serde_json::to_string(&tool.input_schema)?;

    Ok(ServerTool {
        name: normalized_name,
        description,
        arguments,
        execute: mcp_only,
    })
}

fn mcp_only(_arguments: &Value) -> Result<String> {
    Err(anyhow::anyhow!(
        "MCP tool requires MCP execution routing"
    ))
}

pub async fn execute_mcp_tool(
    connections: &HashMap<String, McpConnection>,
    normalized_name: &str,
    arguments: &Value,
) -> Result<String> {
    let route = normalized_name
        .strip_prefix("mcp.")
        .ok_or_else(|| anyhow::anyhow!(
            "Invalid MCP tool name: {}",
            normalized_name
        ))?;

    let (server_alias, tool_name) = route
        .split_once('.')
        .ok_or_else(|| anyhow::anyhow!(
            "Invalid MCP tool route: {}",
            normalized_name
        ))?;

    let connection = connections
        .get(server_alias)
        .ok_or_else(|| anyhow::anyhow!(
            "MCP server '{}' is not connected",
            server_alias
        ))?;

    let argument_object = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!(
            "MCP tool arguments must be a JSON object"
        ))?;

    let result = connection
        .client
        .call_tool(
            CallToolRequestParams::new(tool_name.to_string())
                .with_arguments(argument_object),
        )
        .await?;

    let mut text_parts = Vec::new();

    for content in &result.content {
        if let Some(text) = content.as_text() {
            text_parts.push(text.text.clone());
        }
    }

    let rendered = if text_parts.is_empty() {
        serde_json::to_string(&result)?
    } else {
        text_parts.join("\n")
    };

    if result.is_error.unwrap_or(false) {
        Err(anyhow::anyhow!(rendered))
    } else {
        Ok(rendered)
    }
}
