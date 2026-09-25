use crate::config::ToolTagsConfig;

/// A tool call extracted from a single, newly generated assistant turn.
///
/// `start` and `end` are byte offsets into that assistant turn. They allow
/// the terminal/UI to hide only the actionable tool call while preserving
/// the exact tagged response in model history.
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub kind: ParsedToolCallKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub enum ParsedToolCallKind {
    Command(String),
    Session {
        name: String,
        command: String,
    },
    EndSession(String),
    Json(String),
    Cleanup,
}

/// Examine ONLY the current assistant turn and return the last complete,
/// valid tool call found in that turn.
///
/// This deliberately does not inspect conversation history. Old tool calls
/// therefore cannot be re-executed, and their original tags can remain in
/// model context.
pub fn extract_last_tool_call(
    response_text: &str,
    tags: &ToolTagsConfig,
) -> Option<ParsedToolCall> {
    let mut candidates: Vec<ParsedToolCall> = Vec::new();

    collect_wrapped_calls(
        response_text,
        &tags.command_open,
        &tags.command_close,
        |inner| ParsedToolCallKind::Command(inner.to_string()),
        &mut candidates,
    );

    collect_session_calls(response_text, tags, &mut candidates);
    collect_end_session_calls(response_text, tags, &mut candidates);

    collect_wrapped_calls(
        response_text,
        &tags.json_open,
        &tags.json_close,
        |inner| ParsedToolCallKind::Json(inner.to_string()),
        &mut candidates,
    );

    collect_cleanup_calls(response_text, &mut candidates);

    // "Last call" means the call whose opening tag appears latest in the
    // current model turn. end is used only as a deterministic tie-breaker.
    candidates
        .into_iter()
        .max_by_key(|call| (call.start, call.end))
}

/// Return a terminal-display version of the assistant turn with only the
/// selected actionable tool call removed.
///
/// The original response must still be stored unchanged in model history.
pub fn remove_tool_call_for_display(
    response_text: &str,
    call: &ParsedToolCall,
) -> String {
    if call.start > call.end
        || call.end > response_text.len()
        || !response_text.is_char_boundary(call.start)
        || !response_text.is_char_boundary(call.end)
    {
        return response_text.trim().to_string();
    }

    let mut visible = String::with_capacity(
        response_text.len().saturating_sub(call.end - call.start)
    );

    visible.push_str(&response_text[..call.start]);
    visible.push_str(&response_text[call.end..]);

    visible.trim().to_string()
}

fn collect_wrapped_calls<F>(
    response_text: &str,
    open: &str,
    close: &str,
    make_kind: F,
    candidates: &mut Vec<ParsedToolCall>,
)
where
    F: Fn(&str) -> ParsedToolCallKind,
{
    if open.is_empty() || close.is_empty() {
        return;
    }

    let mut search_from = 0;

    while search_from < response_text.len() {
        let Some(relative_start) = response_text[search_from..].find(open) else {
            break;
        };

        let start = search_from + relative_start;
        let content_start = start + open.len();

        let Some(relative_end) = response_text[content_start..].find(close) else {
            // This opening tag is incomplete. Move forward so a later,
            // complete call of the same type can still be discovered.
            search_from = content_start;
            continue;
        };

        let content_end = content_start + relative_end;
        let end = content_end + close.len();
        let inner = response_text[content_start..content_end].trim();

        candidates.push(ParsedToolCall {
            kind: make_kind(inner),
            start,
            end,
        });

        search_from = end;
    }
}

fn collect_session_calls(
    response_text: &str,
    tags: &ToolTagsConfig,
    candidates: &mut Vec<ParsedToolCall>,
) {
    if tags.session_open.is_empty() || tags.session_close.is_empty() {
        return;
    }

    let mut search_from = 0;

    while search_from < response_text.len() {
        let Some(relative_start) =
            response_text[search_from..].find(&tags.session_open)
        else {
            break;
        };

        let start = search_from + relative_start;
        let name_start = start + tags.session_open.len();
        let after_open = &response_text[name_start..];

        let Some(name_end_relative) = after_open.find('"') else {
            search_from = name_start;
            continue;
        };

        let name_end = name_start + name_end_relative;
        let session_name = response_text[name_start..name_end].trim().to_string();

        let after_name = name_end + 1;
        let Some(opening_end_relative) = response_text[after_name..].find('>') else {
            search_from = after_name;
            continue;
        };

        let content_start = after_name + opening_end_relative + 1;

        let Some(close_relative) =
            response_text[content_start..].find(&tags.session_close)
        else {
            search_from = content_start;
            continue;
        };

        let content_end = content_start + close_relative;
        let end = content_end + tags.session_close.len();
        let command = response_text[content_start..content_end].trim().to_string();

        candidates.push(ParsedToolCall {
            kind: ParsedToolCallKind::Session {
                name: session_name,
                command,
            },
            start,
            end,
        });

        search_from = end;
    }
}

fn collect_end_session_calls(
    response_text: &str,
    tags: &ToolTagsConfig,
    candidates: &mut Vec<ParsedToolCall>,
) {
    if tags.end_session_open.is_empty() {
        return;
    }

    let mut search_from = 0;

    while search_from < response_text.len() {
        let Some(relative_start) =
            response_text[search_from..].find(&tags.end_session_open)
        else {
            break;
        };

        let start = search_from + relative_start;
        let name_start = start + tags.end_session_open.len();
        let after_open = &response_text[name_start..];

        let Some(name_end_relative) = after_open.find('"') else {
            search_from = name_start;
            continue;
        };

        let name_end = name_start + name_end_relative;
        let session_name = response_text[name_start..name_end].trim().to_string();

        let after_name = name_end + 1;
        let Some(tag_end_relative) = response_text[after_name..].find('>') else {
            search_from = after_name;
            continue;
        };

        let end = after_name + tag_end_relative + 1;

        candidates.push(ParsedToolCall {
            kind: ParsedToolCallKind::EndSession(session_name),
            start,
            end,
        });

        search_from = end;
    }
}

fn collect_cleanup_calls(
    response_text: &str,
    candidates: &mut Vec<ParsedToolCall>,
) {
    for tag in ["<cleanup/>", "<cleanup>"] {
        let mut search_from = 0;

        while search_from < response_text.len() {
            let Some(relative_start) = response_text[search_from..].find(tag) else {
                break;
            };

            let start = search_from + relative_start;
            let end = start + tag.len();

            candidates.push(ParsedToolCall {
                kind: ParsedToolCallKind::Cleanup,
                start,
                end,
            });

            search_from = end;
        }
    }
}
