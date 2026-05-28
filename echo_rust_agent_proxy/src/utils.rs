/// Remove lines starting with COMMAND: or SESSION: from the model's response.
/// Preserves original casing of everything else.
pub fn strip_special_lines(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.to_uppercase().starts_with("COMMAND:")
                && !trimmed.to_uppercase().starts_with("SESSION:")
        })
        .collect::<Vec<&str>>()
        .join("\n")
        .trim()
        .to_string()
}
