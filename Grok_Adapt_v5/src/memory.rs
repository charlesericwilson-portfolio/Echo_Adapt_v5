// memory.rs
use anyhow::Result;
use std::path::PathBuf;
use tokio::fs;
use chrono::Local;
use serde_json::Value;
use crate::agent::EchoAgent;

/// Simple vector math for cosine similarity
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Memory manager with semantic retrieval
pub struct Memory {
    pub main_file: PathBuf,
}

impl Memory {
    pub fn new(main_file: PathBuf) -> Self {
        Self { main_file }
    }

    /// Append important information + pre-computed embedding
    pub async fn append(&self, category: &str, content: &str, agent: &EchoAgent) -> Result<()> {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let embedding = self.get_embedding(content, agent).await?;
        let embedding_str = embedding.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(",");

        let entry = format!(
            "\n## {} [{}]\n{}\n\n[EMBEDDING: {}]\n",
            category, timestamp, content.trim(), embedding_str
        );

        let mut file_content = if self.main_file.exists() {
            fs::read_to_string(&self.main_file).await.unwrap_or_default()
        } else {
            String::new()
        };

        file_content.push_str(&entry);
        fs::write(&self.main_file, file_content).await?;

        Ok(())
    }

    /// Get embedding - supports both chat and dedicated embeddings endpoint
    pub async fn get_embedding(&self, text: &str, agent: &EchoAgent) -> Result<Vec<f32>> {
        let is_chat_endpoint = agent.config.embeddings.url.contains("/chat/completions");

        if is_chat_endpoint {
            let system_prompt = "You are an embedding generator. Your only job is to convert text into a dense vector. Output ONLY a comma-separated list of floating point numbers. No explanation, no other text.";

            let payload = serde_json::json!({
                "model": &agent.config.embeddings.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": text}
                ],
                "max_tokens": 1024,
                "temperature": 0.0
            });

            let response = reqwest::Client::new()
                .post(&agent.config.embeddings.url)
                .json(&payload)
                .send()
                .await?
                .json::<Value>()
                .await?;

            let embedding_text = response["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .trim();

            let embedding: Vec<f32> = embedding_text
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();

            Ok(embedding)
        } else {
            // Dedicated embeddings endpoint
            let payload = serde_json::json!({
                "input": text,
                "model": &agent.config.embeddings.model,
            });

            let response = reqwest::Client::new()
                .post(&agent.config.embeddings.url)
                .json(&payload)
                .send()
                .await?
                .json::<Value>()
                .await?;

            let embedding = response["data"][0]["embedding"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("No embedding in response"))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            Ok(embedding)
        }
    }

    /// Fast semantic read using pre-stored embeddings
    pub async fn read_relevant(&self, query: &str, limit: usize, agent: &EchoAgent) -> Result<String> {
        if !self.main_file.exists() {
            return Ok("No memory entries yet.".to_string());
        }

        let content = fs::read_to_string(&self.main_file).await?;

        if content.trim().is_empty() {
            return Ok("Memory is empty.".to_string());
        }

        let query_embedding = self.get_embedding(query, agent).await?;

        let mut scored: Vec<(String, f32)> = Vec::new();  // Use owned String to avoid borrow issues

        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            if line.trim().is_empty() || !line.starts_with("## ") {
                i += 1;
                continue;
            }

            // Collect text until we hit the embedding
            let mut entry_text = String::new();
            let mut embedding: Option<Vec<f32>> = None;

            entry_text.push_str(line);
            entry_text.push('\n');
            i += 1;

            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("[EMBEDDING: ") {
                    if let Some(embed_str) = l.strip_prefix("[EMBEDDING: ").and_then(|s| s.strip_suffix(']')) {
                        embedding = Some(embed_str.split(',').filter_map(|s| s.trim().parse().ok()).collect());
                    }
                    i += 1;
                    break;
                } else if l.starts_with("## ") {
                    break; // new entry started
                } else {
                    entry_text.push_str(l);
                    entry_text.push('\n');
                }
                i += 1;
            }

            if let Some(embed) = embedding {
                let score = cosine_similarity(&query_embedding, &embed);
                scored.push((entry_text.trim().to_string(), score));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let relevant: Vec<String> = scored.into_iter().take(limit).map(|(text, _)| text).collect();

        if relevant.is_empty() {
            Ok("No relevant memory found.".to_string())
        } else {
            Ok(relevant.join("\n\n"))
        }
    }
}
