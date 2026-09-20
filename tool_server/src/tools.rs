use serde_json::Value;
use anyhow::Result;

use crate::config::TavilySection;

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

        ServerTool {
            name: "web_search",
            description: "Search the web for current information",
            arguments: "query: string",
            execute: remote_only,
        },
    ]
}

fn remote_only(_arguments: &Value) -> Result<String> {
    Err(anyhow::anyhow!("Tool requires async server execution"))
}

fn echo_message(arguments: &Value) -> Result<String> {
    let message = arguments["message"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'message' argument"))?;

    Ok(format!("Echo tool received: {}", message))
}

//  TAVILY WEB SEARCH
pub async fn web_search(query: &str, config: &TavilySection) -> Result<String, anyhow::Error> {
    let client = reqwest::Client::new();

    let response = client
        .post(&config.url)
        .json(&serde_json::json!({
            "query": query,
            "api_key": config.api_key,
            "search_depth": "basic",
            "max_results": 6
        }))
        .send()
        .await?;

    let data: Value = response.json().await?;

    let mut results = Vec::new();
    if let Some(results_array) = data["results"].as_array() {
        for (i, item) in results_array.iter().take(6).enumerate() {
            let title = item["title"].as_str().unwrap_or("No title");
            let link = item["url"].as_str().unwrap_or("No link");
            let snippet = item["content"].as_str().unwrap_or("No snippet");

            results.push(format!(
                "{}. {}\n   {}\n   {}",
                i + 1, title, link, snippet
            ));
        }
    }

    if results.is_empty() {
        Ok("No search results found.".to_string())
    } else {
        Ok(results.join("\n\n"))
    }
}
