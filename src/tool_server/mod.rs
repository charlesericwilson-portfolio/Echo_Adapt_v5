pub mod registry;
pub mod tools;

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool_server::registry::ServerToolRegistry;

#[derive(Clone)]
struct ToolServerState {
    registry: Arc<ServerToolRegistry>,
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

pub async fn start_tool_server(bind_address: &str) -> anyhow::Result<()> {
    let registry = Arc::new(ServerToolRegistry::new());

    let state = ToolServerState { registry };

    let app = Router::new()
        .route("/tools", get(list_tools))
        .route("/execute", post(execute_tool))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_address).await?;

    println!("Tool server listening on {}", bind_address);

    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("Tool server error: {}", error);
        }
    });

    Ok(())
}

async fn list_tools(
    State(state): State<ToolServerState>,
) -> Json<ToolListResponse> {
    let tools = state
        .registry
        .all()
        .map(|tool| ToolDescription {
            name: tool.name.to_string(),
            description: tool.description.to_string(),
            arguments: tool.arguments.to_string(),
        })
        .collect();

    Json(ToolListResponse { tools })
}

async fn execute_tool(
    State(state): State<ToolServerState>,
    Json(request): Json<ExecuteRequest>,
) -> (StatusCode, Json<ExecuteResponse>) {
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
