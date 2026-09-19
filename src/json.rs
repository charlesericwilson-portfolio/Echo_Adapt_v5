use serde_json::Value;
use anyhow::Result;
use chrono::Local;
use crate::log::save_chat_log_message;
use std::time::Duration;
use crate::memory::Memory;
use std::path::PathBuf;
use crate::config::WebSearchConfig;
use scraper::Html;
use scraper::Selector;
use crate::remote_tools::{call_remote_tool, RemoteToolRegistry};

//  MAIN JSON TOOL HANDLER
pub async fn handle_json_tool(
    agent: &mut crate::agent::EchoAgent,
    _user_input: &str,
    _current_response: &str,
    json_content: &str,
) -> Result<()> {
    println!("{}Echo: Detected JSON tool call{}",
             crate::agent::YELLOW, crate::agent::RESET_COLOR);

    let enabled_tools = &agent.config.json_tools.enabled;

        // Memory tools
        if let Some(tool_name) = extract_tool_name(json_content) {
            if tool_name == "append_memory" || tool_name == "read_memory" {
                if !enabled_tools.contains(&tool_name) {
                    let error_msg = format!(
                        "JSON Tool error: Tool '{}' is not enabled in config",
                        tool_name
                    );

                    agent.push_tool_result(&error_msg);

                    save_chat_log_message(
                        &agent.home_dir,
                        &agent.config.messages.tool_role_name,
                        &error_msg,
                    ).await?;

                    return Ok(());
                }

                let arguments = parse_arguments(json_content);

                match handle_memory_tool(agent, &tool_name, &arguments).await {
                    Ok(result) => {
                        let tool_content = format!("Tool output:\n{}", result);

                        agent.push_tool_result(&tool_content);

                        save_chat_log_message(
                            &agent.home_dir,
                            &agent.config.messages.tool_role_name,
                            &tool_content,
                        ).await?;
                    }

                    Err(e) => {
                        let error_msg = format!("Memory Tool error: {}", e);

                        agent.push_tool_result(&error_msg);

                        save_chat_log_message(
                            &agent.home_dir,
                            &agent.config.messages.tool_role_name,
                            &error_msg,
                        ).await?;
                    }
                }

                return Ok(());
            }
        }

    // Regular tools (passes config)
    match handle_json_tool_call_str(
        json_content,
        agent.config.web_search.as_ref(),
        enabled_tools,
        &agent.remote_tool_registry,
        &agent.config.tool_server.url,
    ).await {
        Ok(result) => {
            if let Some(tool_name) = extract_tool_name(json_content) {
                println!("{}Echo: [TOOL] {} executed{}",
                         crate::agent::YELLOW, tool_name, crate::agent::RESET_COLOR);
            }

            let tool_content = format!("Tool output:\n{}", result);

            agent.push_tool_result(&tool_content);

            save_chat_log_message(
                &agent.home_dir,
                &agent.config.messages.tool_role_name,
                &tool_content,
            ).await?;
        }
        Err(e) => {
            let error_msg = format!("JSON Tool error: {}", e);

            agent.push_tool_result(&error_msg);

            save_chat_log_message(
                &agent.home_dir,
                &agent.config.messages.tool_role_name,
                &error_msg,
            ).await?;
        }
    }

    Ok(())
}

//  TOOL CALL PARSER
pub async fn handle_json_tool_call_str(
    tool_call: &str,
    web_search_config: Option<&WebSearchConfig>,
    enabled_tools: &[String],
    remote_registry: &RemoteToolRegistry,
    remote_server_url: &str,
) -> Result<String> {
    let parsed: Value = serde_json::from_str(tool_call)
        .map_err(|e| anyhow::anyhow!("Failed to parse JSON tool call: {}", e))?;

    let function = if parsed["tool_calls"].is_array() && parsed["tool_calls"][0]["function"].is_object() {
        &parsed["tool_calls"][0]["function"]
    } else if parsed["function"].is_object() {
        &parsed["function"]
    } else {
        &parsed
    };

    let tool_name = function["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No tool name found in JSON"))?;

    if !enabled_tools.contains(&tool_name.to_string())
        && !remote_registry.contains(tool_name)
    {
        return Err(anyhow::anyhow!(
            "Tool '{}' is not enabled in config",
            tool_name
        ));
    }

    let arguments: Value = if function["arguments"].is_string() {
        let args_str = function["arguments"].as_str().unwrap();
        serde_json::from_str(args_str).unwrap_or(Value::Object(serde_json::Map::new()))
    } else if function["arguments"].is_object() {
        function["arguments"].clone()
    } else {
        Value::Object(serde_json::Map::new())
    };

    match tool_name {
        "get_current_datetime" => {
            let now = Local::now();
            Ok(format!("Current datetime: {}", now.format("%Y-%m-%d %H:%M:%S %Z")))
        }

        "web_search" => {
            let query = arguments["query"].as_str().unwrap_or("No query provided");
            let config = web_search_config.ok_or_else(|| anyhow::anyhow!("Web search not configured"))?;

            match web_search(query, config).await {
                Ok(results) => Ok(format!("Web search results for '{}':\n\n{}", query, results)),
                Err(e) => Ok(format!("Web search failed: {}", e)),
            }
        }

        "browse_page" => {
            let url = arguments["url"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'url' argument for browse_page"))?;
            let max_chars = arguments["max_chars"].as_u64().map(|v| v as usize);
            match browse_page(url, max_chars).await {
                Ok(content) => Ok(format!("Content from {}:\n\n{}", url, content)),
                Err(e) => Ok(format!("Failed to browse page: {}", e)),
            }
        }

         _ if remote_registry.contains(tool_name) => {
            call_remote_tool(
                remote_server_url,
                tool_name,
                &arguments,
            ).await
        }

        _ => Err(anyhow::anyhow!("Unknown JSON tool: {}", tool_name)),
    }
}

//  TAVILY WEB SEARCH
pub async fn web_search(query: &str, config: &WebSearchConfig) -> Result<String, anyhow::Error> {
    let client = reqwest::Client::new();

    let response = client
        .post(&config.url)
        .json(&serde_json::json!({
            "query": query,
            "api_key": config.api_key.as_deref().unwrap_or(""),
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

//  BROWSE PAGE
pub async fn browse_page(url: &str, max_chars: Option<usize>) -> Result<String, anyhow::Error> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; EchoAgent/1.0)")
        .timeout(Duration::from_secs(30))
        .build()?;

    let response = client
        .get(url)
        .send()
        .await?
        .error_for_status()?;

    let final_url = response.url().clone();
    let html = response.text().await?;
    let document = Html::parse_document(&html);

    // Render readable page content instead of raw HTML/CSS/JS structure.
    let content_selector = Selector::parse(
        "h1, h2, h3, h4, h5, h6, p, li, pre, blockquote, td, th, caption, figcaption"
    ).unwrap();

    let mut blocks = Vec::new();

    for element in document.select(&content_selector) {
        let text = clean_text(&element.text().collect::<Vec<_>>().join(" "));

        if text.is_empty() {
            continue;
        }

        let rendered = match element.value().name() {
            "h1" => format!("# {}", text),
            "h2" => format!("## {}", text),
            "h3" => format!("### {}", text),
            "h4" => format!("#### {}", text),
            "h5" => format!("##### {}", text),
            "h6" => format!("###### {}", text),
            "li" => format!("- {}", text),
            "blockquote" => format!("> {}", text),
            "pre" => format!("```\n{}\n```", text),
            _ => text,
        };

        if blocks.last().map(|last| last != &rendered).unwrap_or(true) {
            blocks.push(rendered);
        }
    }

    // Fallback for pages that do not use normal semantic elements.
    if blocks.is_empty() {
        let body_selector = Selector::parse("body").unwrap();

        if let Some(body) = document.select(&body_selector).next() {
            let fallback = clean_text(&body.text().collect::<Vec<_>>().join(" "));

            if !fallback.is_empty() {
                blocks.push(fallback);
            }
        }
    }

    // Expose links so the model can continue browsing without seeing raw HTML.
    let link_selector = Selector::parse("a[href]").unwrap();
    let mut links = Vec::new();

    for link in document.select(&link_selector) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };

        let href = href.trim();

        if href.is_empty()
            || href.starts_with("javascript:")
            || href.starts_with("data:")
        {
            continue;
        }

        let label = clean_text(&link.text().collect::<Vec<_>>().join(" "));

        let resolved = final_url
            .join(href)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| href.to_string());

        let rendered = if label.is_empty() {
            resolved
        } else {
            format!("{} [{}]", label, resolved)
        };

        if !links.contains(&rendered) {
            links.push(rendered);
        }
    }

    let mut text_content = blocks.join("\n\n");

    if !links.is_empty() {
        text_content.push_str("\n\nLinks:\n");
        text_content.push_str(&links.join("\n"));
    }

    let max = max_chars.unwrap_or(100_000);

    if text_content.len() <= max {
        return Ok(text_content);
    }

    let mut end = max.min(text_content.len());

    while end > 0 && !text_content.is_char_boundary(end) {
        end -= 1;
    }

    Ok(format!(
        "{}...\n\n[Content truncated at {} characters. Original extracted length: {} characters.]",
        &text_content[..end],
        end,
        text_content.len()
    ))
}

fn clean_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

//  MEMORY TOOL HANDLER
pub async fn handle_memory_tool(
    agent: &mut crate::agent::EchoAgent,
    tool_name: &str,
    arguments: &Value,
) -> Result<String> {
    let memory = Memory::new(PathBuf::from(&agent.config.paths.memory_file));

    match tool_name {
        "append_memory" => {
            let category = arguments["category"].as_str().unwrap_or("General");
            let content = arguments["content"].as_str().unwrap_or("");

            println!("{}Echo: [MEMORY] append_memory → category: {}{}",
                     crate::agent::YELLOW, category, crate::agent::RESET_COLOR);

            memory.append(category, content, agent).await?;
            Ok("Memory updated successfully.".to_string())
        }

        "read_memory" => {
            let query = arguments["query"].as_str().unwrap_or("");
            let limit = arguments["limit"].as_u64().unwrap_or(5) as usize;

            println!("{}Echo: [MEMORY] read_memory → query: '{}' (limit: {}){}",
                     crate::agent::YELLOW, query, limit, crate::agent::RESET_COLOR);

            memory.read_relevant(query, limit, agent).await
        }

        _ => Err(anyhow::anyhow!("Unknown memory tool: {}", tool_name)),
    }
}

//  HELPERS
fn extract_tool_name(json_str: &str) -> Option<String> {
    if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
        if let Some(name) = parsed["name"].as_str() {
            return Some(name.to_string());
        }

        if let Some(function) = parsed["function"].as_object() {
            if let Some(name) = function["name"].as_str() {
                return Some(name.to_string());
            }
        }

        if let Some(function) = parsed["tool_calls"]
            .get(0)
            .and_then(|tool_call| tool_call["function"].as_object())
        {
            if let Some(name) = function["name"].as_str() {
                return Some(name.to_string());
            }
        }
    }

    None
}

fn parse_arguments(json_str: &str) -> Value {
    let parsed = match serde_json::from_str::<Value>(json_str) {
        Ok(parsed) => parsed,
        Err(_) => return Value::Object(serde_json::Map::new()),
    };

    let function = if parsed["tool_calls"].is_array()
        && parsed["tool_calls"][0]["function"].is_object()
    {
        &parsed["tool_calls"][0]["function"]
    } else if parsed["function"].is_object() {
        &parsed["function"]
    } else {
        &parsed
    };

    if let Some(args) = function["arguments"].as_object() {
        return Value::Object(args.clone());
    }

    if let Some(args) = function["arguments"].as_str() {
        if let Ok(value) = serde_json::from_str::<Value>(args) {
            return value;
        }
    }

    Value::Object(serde_json::Map::new())
}
