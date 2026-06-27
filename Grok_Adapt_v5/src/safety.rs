use crate::config::Config;

pub fn is_command_safe(command: &str, config: &Config) -> Result<(), String> {
    let lower_cmd = command.to_lowercase();

    // === Layer 1: Raw denylist check ===
    for dangerous in &config.security.denylist {
        if lower_cmd.contains(&dangerous.to_lowercase()) {
            return Err(format!("Command contains dangerous keyword: {}", dangerous));
        }
    }

    // === Layer 2: Hardcoded dangerous patterns ===
    if lower_cmd.contains("sudo rm") || lower_cmd.contains("rm -rf /") {
        return Err("Dangerous rm command blocked for safety.".to_string());
    }

    // === Layer 3: Detect obfuscated dangerous commands ===
    // Catches things like: rm"-rf", r'm', $(rm), `rm`, rm$var, etc.
    let dangerous_bases = ["rm", "mkfs", "dd", "shred", "wipefs", "fdisk", "parted"];

    for base in dangerous_bases {
        // Check for the base command even when it's concatenated or quoted
        if lower_cmd.contains(&format!("{} ", base)) ||
           lower_cmd.contains(&format!("{}-", base)) ||
           lower_cmd.contains(&format!("{}'", base)) ||
           lower_cmd.contains(&format!("{}\"", base)) ||
           lower_cmd.contains(&format!("$({})", base)) ||
           lower_cmd.contains(&format!("`{}`", base))
        {
            // Now check if it also contains dangerous flags
            if lower_cmd.contains("-r") || lower_cmd.contains("-f") ||
               lower_cmd.contains("/dev/") || lower_cmd.contains("/home") ||
               lower_cmd.contains("/tmp") || lower_cmd.contains("/root")
            {
                return Err(format!("Obfuscated dangerous command detected: {}", base));
            }
        }
    }

    // === Layer 4: Token-based check using shell-words ===
    if let Ok(tokens) = shell_words::split(command) {
        if let Some(first) = tokens.first() {
            let base = first.to_lowercase();

            for dangerous in &config.security.denylist {
                if base == dangerous.to_lowercase() {
                    return Err(format!("Base command '{}' is blocked", first));
                }
            }

            if matches!(base.as_str(), "rm" | "mkfs" | "dd" | "shred" | "wipefs" | "fdisk" | "parted") {
                return Err(format!("Dangerous base command blocked: {}", first));
            }
        }
    }

    Ok(())
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, SecurityConfig, EndpointConfig, SummarizerConfig,
                        PromptsConfig, ContextConfig, PathsConfig};

    fn test_config() -> Config {
        Config {
            endpoint: EndpointConfig {
                url: "http://localhost".to_string(),
                model: "test".to_string(),
                temperature: 0.7,
                max_tokens: 2048,
            },
            summarizer: SummarizerConfig {
                url: "http://localhost".to_string(),
                model: "summarizer".to_string(),
            },
            prompts: PromptsConfig {
                main_system: "test".to_string(),
                summarizer: "test".to_string(),
            },
            security: SecurityConfig {
                denylist: vec![
                    "rm -rf".to_string(),
                    "rm -r /".to_string(),
                    "$(echo rm) -rf /tmp".to_string(),
                    "mkfs".to_string(),
                    "dd if=/dev/zero".to_string(),
                ],
            },
            context: ContextConfig {
                summarize_threshold: 100000,
            },
            paths: PathsConfig {
                home_dir: None,
                context_file: "test.txt".to_string(),
                database: "test.db".to_string(),
            },
            web_search: None,
            json_tools: crate::config::JsonToolsConfig::default(),
        }
    }

    #[test]
    fn test_safe_command() {
        let config = test_config();
        assert!(is_command_safe("ls -la", &config).is_ok());
        assert!(is_command_safe("echo hello", &config).is_ok());
        assert!(is_command_safe("cat file.txt", &config).is_ok());
    }

    #[test]
    fn test_direct_dangerous_command() {
        let config = test_config();
        assert!(is_command_safe("rm -rf /home", &config).is_err());
        assert!(is_command_safe("mkfs.ext4 /dev/sda", &config).is_err());
    }

    #[test]
    fn test_obfuscated_command() {
        let config = test_config();
        assert!(is_command_safe(r#"rm"-rf" /home"#, &config).is_err());
        assert!(is_command_safe("r'm' -rf /tmp", &config).is_err());
        assert!(is_command_safe("$(echo rm) -rf /tmp", &config).is_err());
    }

    #[test]
    fn test_command_chaining() {
        let config = test_config();
        assert!(is_command_safe("ls && rm -rf /tmp/test", &config).is_err());
        assert!(is_command_safe("echo hi; rm -r /", &config).is_err());
    }

    #[test]
    fn test_base_command_blocking() {
        let config = test_config();
        assert!(is_command_safe("rm important.txt", &config).is_err());
        assert!(is_command_safe("dd if=/dev/zero of=/dev/sda", &config).is_err());
    }
}
