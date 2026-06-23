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

    /// Append important information to memory
    pub async fn append(&self, category: &str, content: &str) -> Result<()> {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let entry = format!(
            "\n## {} [{}]\n{}\n",
            category, timestamp, content.trim()
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

    /// Get embedding for a text using the main model
    pub async fn get_embedding(&self, text: &str, agent: &EchoAgent) -> Result<Vec<f32>> {
        let prompt = format!("Generate a dense embedding vector for the following text. Output only the vector as comma-separated numbers: {}", text);

        let payload = serde_json::json!({
            "model": &agent.config.endpoint.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 512,
            "temperature": 0.0
        });

        let response = reqwest::Client::new()
            .post(&agent.config.endpoint.url)
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
    }

    /// Semantic read - finds relevant context
    pub async fn read_relevant(&self, query: &str, limit: usize, agent: &EchoAgent) -> Result<String> {
        if !self.main_file.exists() {
            return Ok("No memory entries yet.".to_string());
        }

        let content = fs::read_to_string(&self.main_file).await?;

        if content.trim().is_empty() {
            return Ok("Memory is empty.".to_string());
        }

        let query_embedding = self.get_embedding(query, agent).await?;

        // Score lines by similarity to query
        let mut scored: Vec<(&str, f32)> = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let line_embedding = self.get_embedding(line, agent).await?;
            let score = cosine_similarity(&query_embedding, &line_embedding);
            scored.push((line, score));
        }

        // Sort by score and take top results
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let relevant: Vec<&str> = scored.into_iter().take(limit).map(|(line, _)| line).collect();

        if relevant.is_empty() {
            Ok("No relevant memory found.".to_string())
        } else {
            Ok(relevant.join("\n\n"))
        }
    }
}
