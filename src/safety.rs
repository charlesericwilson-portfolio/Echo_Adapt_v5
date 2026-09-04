use crate::config::Config;

/// Validates whether a command or a chain of commands is safe to execute.
/// Allows legitimate chaining (`&&`, `||`, `;`, `|`) while inspecting every individual stage.
pub fn is_command_safe(command: &str, config: &Config) -> Result<(), String> {
    let lower_cmd = command.to_lowercase();

    // === Layer 1: Global Exact Substring Denylist ===
    for dangerous in &config.security.denylist {
        if lower_cmd.contains(&dangerous.to_lowercase()) {
            return Err(format!("Command contains blocked keyword: {}", dangerous));
        }
    }

    // === Layer 2: Obfuscation Filter (r'm', "r"m, etc.) ===
    let normalized: String = lower_cmd
        .chars()
        .filter(|c| *c != '\'' && *c != '"' && *c != '\\' && *c != '$' && *c != '(' && *c != ')')
        .collect();

    let destructive_primitives = ["mkfs", "wipefs", "shred", "fdisk", "parted", "dd", "rm"];

    // === Layer 3: Split and Validate Each Subcommand in the Chain ===
    // Splits on &&, ||, ;, |, and newlines so every step in a pipeline is checked individually
    let subcommands = command.split(|c| c == ';' || c == '|' || c == '&' || c == '\n');

    for raw_sub in subcommands {
        let sub = raw_sub.trim();
        if sub.is_empty() {
            continue;
        }

        // Tokenize the individual subcommand
        if let Ok(tokens) = shell_words::split(sub) {
            if let Some(first_token) = tokens.first() {
                let base = first_token.to_lowercase();

                // Check denylist on the base executable of the subcommand
                for dangerous in &config.security.denylist {
                    if base == dangerous.to_lowercase() {
                        return Err(format!("Chained subcommand '{}' is blocked by security policy", first_token));
                    }
                }

                // Check destructive primitives on the base executable
                if destructive_primitives.contains(&base.as_str()) {
                    return Err(format!("Destructive binary '{}' blocked in command chain", first_token));
                }
            }
        }
    }

    // === Layer 4: Catch Obfuscated Destructive Calls Inside Normalized String ===
    for primitive in destructive_primitives {
        if normalized.contains(&format!("{} ", primitive))
            || normalized.contains(&format!("{}-", primitive))
            || normalized.contains(&format!("{}/", primitive))
            || normalized.ends_with(primitive)
        {
            return Err(format!("Obfuscated execution of '{}' detected", primitive));
        }
    }

    Ok(())
}

// Unit Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, SecurityConfig, EndpointConfig, SummarizerConfig,
                        PromptsConfig, ContextConfig, PathsConfig, EmbeddingsConfig,
                        MessagesConfig, ToolTagsConfig, JsonToolsConfig};

    fn test_config() -> Config {
        Config {
            endpoint: EndpointConfig {
                url: "http://localhost:8080".to_string(),
                model: "test".to_string(),
                temperature: 0.7,
                max_tokens: 2048,
            },
            summarizer: SummarizerConfig {
                url: "http://localhost:8082".to_string(),
                model: "summarizer".to_string(),
                enabled: true,
                max_raw_output_chars: 6000,
            },
            embeddings: EmbeddingsConfig {
                url: "http://localhost:8080".to_string(),
                model: "test".to_string(),
            },
            prompts: PromptsConfig {
                main_system: "test.txt".to_string(),
                summarizer: "test.txt".to_string(),
            },
            security: SecurityConfig {
                denylist: vec![
                    "rm -rf".to_string(),
                    "rm -r /".to_string(),
                    "> /dev/sda".to_string(),
                ],
            },
            context: ContextConfig {
                summarize_threshold: 100000,
                max_turns: 15,
            },
            paths: PathsConfig {
                home_dir: None,
                context_file: "test.txt".to_string(),
                database: "test.db".to_string(),
                memory_file: "memory.md".to_string(),
            },
            web_search: None,
            json_tools: JsonToolsConfig::default(),
            messages: MessagesConfig {
                tool_role_name: "tool".to_string(),
            },
            tool_tags: ToolTagsConfig::default(),
        }
    }

    #[test]
    fn test_safe_chained_commands() {
        let config = test_config();
        // Valid multi-tool workflows must pass cleanly
        assert!(is_command_safe("cargo check && cargo build --release", &config).is_ok());
        assert!(is_command_safe("git status || git log", &config).is_ok());
        assert!(is_command_safe("cat src/main.rs | grep -i struct", &config).is_ok());
        assert!(is_command_safe("mkdir -p /tmp/workspace; cd /tmp/workspace && ls -la", &config).is_ok());
    }

    #[test]
    fn test_blocked_chained_commands() {
        let config = test_config();
        // Destructive binaries hidden anywhere inside chains must fail
        assert!(is_command_safe("git status && rm -rf /", &config).is_err());
        assert!(is_command_safe("ls -la; dd if=/dev/zero of=/dev/sda", &config).is_err());
        assert!(is_command_safe("cat file.txt | rm temp.txt", &config).is_err());
    }
}
