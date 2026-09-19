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

#[derive(Clone)]
struct ToolServerState {
    registry: Arc<ServerToolRegistry>,
    auth_token: String,
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
    let state = ToolServerState { registry, auth_token: config.server.auth_token.clone(), };

    let app = Router::new()
        .route("/tools", get(list_tools))
        .route("/execute", post(execute_tool))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_address).await?;

    println!("Tool server listening on {}", bind_address);

    axum::serve(listener, app).await?;

    Ok(())
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

    let tools = state
        .registry
        .all()
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
        return (
            StatusCode::UNAUTHORIZED,
            Json(ExecuteResponse {
                result: None,
                error: Some("Unauthorized".to_string()),
            }),
        );
    }

    let Some(tool) = state.registry.get(&request.name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ExecuteResponse {
                result: None,
                error: Some(format!("Unknown server tool: {}", request.name)),
            }),
        );
    };

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
