use serde_json::Value;
use anyhow::Result;

pub struct ServerTool {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: &'static str,
    pub execute: fn(&Value) -> Result<String>,
}

pub fn all_tools() -> Vec<ServerTool> {
    vec![
        ServerTool {
            name: "echo_message",
            description: "Return the supplied message",
            arguments: "message: string",
            execute: echo_message,
        },
    ]
}

fn echo_message(arguments: &Value) -> Result<String> {
    let message = arguments["message"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'message' argument"))?;

    Ok(format!("Echo tool received: {}", message))
}
