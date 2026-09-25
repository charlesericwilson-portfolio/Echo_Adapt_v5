mod registry;
mod tools;
mod config;

use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::registry::ServerToolRegistry;
use crate::config::{InstanceSection, TavilySection};
use crate::tools::web_search;

#[derive(Clone)]
struct ToolServerState {
    registry: Arc<ServerToolRegistry>,
    auth_token: String,
    tavily: TavilySection,
    global_tools: Vec<String>,
    instances: std::collections::HashMap<String, InstanceSection>,
}

#[derive(Debug, Serialize)]
struct ToolDescription {
    name: String,
    description: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct ToolListResponse {
    tools: Vec<ToolDescription>,
}

#[derive(Debug, Deserialize)]
struct ExecuteRequest {
    name: String,
    arguments: Value,
}

#[derive(Debug, Serialize)]
struct ExecuteResponse {
    result: Option<String>,
    error: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::load_config("tool_server.toml")
        .expect("Failed to load tool_server.toml");

    let bind_address = &config.server.bind_address;

    let registry = Arc::new(ServerToolRegistry::new());
    let state = ToolServerState {
        registry,
        auth_token: config.server.auth_token.clone(),
        tavily: config.tavily.clone(),
        global_tools: config.global.allowed_tools,
        instances: config.instances,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/tools", get(list_tools))
        .route("/execute", post(execute_tool))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_address).await?;

    println!("Tool server listening on {}", bind_address);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn list_tools(
    State(state): State<ToolServerState>,
    headers: HeaderMap,
) -> Result<Json<ToolListResponse>, StatusCode> {
    let expected = format!("Bearer {}", state.auth_token);

    let authorized = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(|value| value == expected)
        .unwrap_or(false);

    if !authorized {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let instance_id = headers
    .get("x-instance-id")
    .and_then(|value| value.to_str().ok())
    .ok_or(StatusCode::BAD_REQUEST)?;

    let instance_tools = state
    .instances
    .get(instance_id)
    .map(|instance| &instance.allowed_tools);

    let tools = state
        .registry
        .all()
        .filter(|tool| {
            state.global_tools.iter().any(|name| name == tool.name)
            || instance_tools
            .map(|allowed| allowed.iter().any(|name| name == tool.name))
            .unwrap_or(false)
        })
        .map(|tool| ToolDescription {
            name: tool.name.to_string(),
            description: tool.description.to_string(),
            arguments: tool.arguments.to_string(),
        })
        .collect();

    Ok(Json(ToolListResponse { tools }))
}

async fn execute_tool(
    State(state): State<ToolServerState>,
    headers: HeaderMap,
    Json(request): Json<ExecuteRequest>,
) -> (StatusCode, Json<ExecuteResponse>) {
    let expected = format!("Bearer {}", state.auth_token);

    let authorized = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(|value| value == expected)
        .unwrap_or(false);

    if !authorized {
        println!(
            "[AUTH] DENIED execute request for tool: {}",
            request.name
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(ExecuteResponse {
                result: None,
                error: Some("Unauthorized".to_string()),
            }),
        );
    }

    let instance_id = match headers
    .get("x-instance-id")
    .and_then(|value| value.to_str().ok())
    {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ExecuteResponse {
                    result: None,
                    error: Some("Missing x-instance-id header".to_string()),
                }),
            );
        }
    };

    let instance_allowed = state
    .instances
    .get(instance_id)
    .map(|instance| {
        instance
        .allowed_tools
        .iter()
        .any(|name| name == &request.name)
    })
    .unwrap_or(false);

    let global_allowed = state
    .global_tools
    .iter()
    .any(|name| name == &request.name);

    if !global_allowed && !instance_allowed {
        println!(
            "[AUTH] DENIED instance '{}' access to tool: {}",
            instance_id,
            request.name
        );

        return (
            StatusCode::FORBIDDEN,
            Json(ExecuteResponse {
                result: None,
                error: Some(format!(
                    "Tool '{}' is not allowed for instance '{}'",
                    request.name, instance_id
                )),
            }),
        );
    }

    println!(
        "[AUTH] SUCCESS execute request for tool: {}",
        request.name
    );

    let Some(tool) = state.registry.get(&request.name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ExecuteResponse {
                result: None,
                error: Some(format!("Unknown server tool: {}", request.name)),
            }),
        );
    };

   if request.name == "web_search" {
    let query = match request.arguments["query"].as_str() {
        Some(query) => query,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ExecuteResponse {
                    result: None,
                    error: Some("Missing 'query' argument".to_string()),
                }),
            );
        }
    };

    return match web_search(query, &state.tavily).await {
        Ok(result) => (
            StatusCode::OK,
            Json(ExecuteResponse {
                result: Some(result),
                error: None,
            }),
        ),

        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(ExecuteResponse {
                result: None,
                error: Some(error.to_string()),
            }),
        ),
    };
}


    match (tool.execute)(&request.arguments) {
        Ok(result) => (
            StatusCode::OK,
            Json(ExecuteResponse {
                result: Some(result),
                error: None,
            }),
        ),

        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(ExecuteResponse {
                result: None,
                error: Some(error.to_string()),
            }),
        ),
    }
}
