/// Parser for Claude Code conversation JSONL files.
/// Extracts user messages and assistant text responses into
/// clean conversation text suitable for KG extraction.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A parsed conversation from a Claude Code JSONL file.
#[derive(Debug, Clone)]
pub struct Conversation {
    pub session_id: String,
    pub messages: Vec<ConversationMessage>,
    pub file_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub role: String,      // "user" or "assistant"
    pub content: String,
    pub timestamp: String,
}

impl Conversation {
    /// Combine all messages into a single text for extraction,
    /// preserving the conversation flow.
    pub fn to_text(&self, max_chars: usize) -> String {
        let mut text = String::new();
        for msg in &self.messages {
            if text.len() >= max_chars {
                break;
            }
            let prefix = if msg.role == "user" { "User" } else { "Assistant" };
            text.push_str(&format!("[{}]: {}\n\n", prefix, msg.content));
        }
        if text.len() > max_chars {
            text.truncate(max_chars);
        }
        text
    }

    /// Get substantive text optimized for KG extraction.
    /// Prioritizes user messages (decisions, requests) over assistant output.
    /// Truncates long assistant messages. Samples across the full conversation.
    pub fn substantive_text(&self, max_chars: usize) -> String {
        // Build condensed messages: keep all user messages, truncate assistant messages
        let mut condensed: Vec<(String, String)> = Vec::new(); // (role, text)

        for msg in &self.messages {
            if msg.content.len() < 15 {
                continue;
            }
            if msg.role == "user" {
                // Keep user messages fully (they contain decisions)
                condensed.push(("User".to_string(), msg.content.clone()));
            } else {
                // Truncate assistant messages to first 300 chars (key info is at the start)
                let truncated = if msg.content.len() > 300 {
                    let mut end = 300;
                    while end > 0 && !msg.content.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &msg.content[..end])
                } else {
                    msg.content.clone()
                };
                condensed.push(("Assistant".to_string(), truncated));
            }
        }

        if condensed.is_empty() {
            return String::new();
        }

        // Check if everything fits
        let total: usize = condensed.iter().map(|(r, t)| r.len() + t.len() + 10).sum();
        if total <= max_chars {
            return condensed.iter()
                .map(|(r, t)| format!("[{}]: {}\n\n", r, t))
                .collect();
        }

        // Sample: 20% start, 40% middle, 40% end (recent work matters more)
        let n = condensed.len();
        let mut text = String::new();
        let sections = [
            (0, n / 5, max_chars / 5),                    // start: 20%
            (n * 2 / 5, n * 3 / 5, max_chars * 2 / 5),   // middle: 40%
            (n * 4 / 5, n, max_chars * 2 / 5),            // end: 40%
        ];

        for (start, end, budget) in &sections {
            let section_start = text.len();
            if *start > 0 {
                text.push_str("\n[...]\n\n");
            }
            for (role, content) in &condensed[*start..*end] {
                if text.len() - section_start >= *budget {
                    break;
                }
                text.push_str(&format!("[{}]: {}\n\n", role, content));
            }
        }

        // Safe truncate
        if text.len() > max_chars {
            let mut end = max_chars;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        text
    }

    /// Get only user messages (where decisions and key concepts live).
    /// Best for project ingestion - captures all decisions without assistant verbosity.
    pub fn user_messages_text(&self, max_chars: usize) -> String {
        let mut text = String::new();
        for msg in &self.messages {
            if msg.role != "user" {
                continue;
            }
            if msg.content.len() < 15 {
                continue;
            }
            // Skip system/command messages
            let content_lower = msg.content.to_lowercase();
            if content_lower.starts_with("<command")
                || content_lower.starts_with("<task-notification")
                || content_lower.starts_with("<local-command")
                || content_lower.contains("tool-use-id")
            {
                continue;
            }
            // Skip extraction prompts (from automated tests)
            if content_lower.starts_with("extract entities")
                || content_lower.starts_with("analyze this text")
                || content_lower.contains("return only a valid json")
            {
                continue;
            }

            text.push_str(&msg.content);
            text.push_str("\n\n");

            if text.len() >= max_chars {
                break;
            }
        }
        // Safe truncate
        if text.len() > max_chars {
            let mut end = max_chars;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        text
    }

    /// Check if this conversation is automated (e.g., from `claude -p` calls).
    pub fn is_automated(&self) -> bool {
        // Automated conversations typically have exactly 2 messages
        // and the user message starts with extraction/analysis prompts
        if self.messages.len() <= 2 {
            if let Some(first) = self.messages.first() {
                let content = first.content.to_lowercase();
                return content.starts_with("extract entities")
                    || content.starts_with("analyze this text")
                    || content.starts_with("read and follow")
                    || content.contains("return only a valid json");
            }
        }
        false
    }
}

/// Internal deserialization structures for JSONL lines.
#[derive(Deserialize)]
struct JsonlLine {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    message: Option<MessageContent>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct MessageContent {
    role: Option<String>,
    content: Option<serde_json::Value>,
}

/// Parse a single JSONL conversation file.
pub fn parse_conversation(path: &Path) -> Option<Conversation> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut messages = Vec::new();
    let mut session_id = String::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: JsonlLine = match serde_json::from_str(line) {
            Ok(p) => p,
            Err(_) => continue,
        };

        if session_id.is_empty() {
            if let Some(sid) = &parsed.session_id {
                session_id = sid.clone();
            }
        }

        let msg_type = parsed.msg_type.as_deref().unwrap_or("");
        if msg_type != "user" && msg_type != "assistant" {
            continue;
        }

        if let Some(message) = &parsed.message {
            let role = message.role.as_deref().unwrap_or("").to_string();
            if role != "user" && role != "assistant" {
                continue;
            }

            let content = extract_text_content(&message.content);
            if content.is_empty() {
                continue;
            }

            let timestamp = parsed.timestamp.clone().unwrap_or_default();

            // Deduplicate: assistant messages can appear multiple times
            // (streaming chunks). Keep only the last/longest for each timestamp group.
            if role == "assistant" {
                if let Some(last) = messages.last_mut() {
                    let last_msg: &mut ConversationMessage = last;
                    if last_msg.role == "assistant" && last_msg.timestamp == timestamp {
                        // Same assistant turn - keep the longer content
                        if content.len() > last_msg.content.len() {
                            last_msg.content = content;
                        }
                        continue;
                    }
                }
            }

            messages.push(ConversationMessage {
                role,
                content,
                timestamp,
            });
        }
    }

    if messages.is_empty() {
        return None;
    }

    Some(Conversation {
        session_id,
        messages,
        file_path: path.to_path_buf(),
    })
}

/// Extract text content from a message's content field.
/// Handles both string content and array of content blocks.
fn extract_text_content(content: &Option<serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => {
            let mut text_parts = Vec::new();
            for block in blocks {
                if let Some(obj) = block.as_object() {
                    let block_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match block_type {
                        "text" => {
                            if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                        "tool_use" => {
                            // Include tool name for context
                            if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                                let input = obj.get("input")
                                    .and_then(|v| v.as_object())
                                    .map(|o| {
                                        // Extract key parameters
                                        o.iter()
                                            .filter(|(k, _)| *k != "command" && *k != "prompt")
                                            .take(3)
                                            .map(|(k, v)| format!("{}={}", k, truncate_value(v)))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .unwrap_or_default();
                                if !input.is_empty() {
                                    text_parts.push(format!("[Tool: {} ({})]", name, input));
                                }
                            }
                        }
                        // Skip thinking blocks, tool_result, etc.
                        _ => {}
                    }
                }
            }
            text_parts.join("\n")
        }
        _ => String::new(),
    }
}

fn truncate_value(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 50 {
        let mut end = 47;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    } else {
        s
    }
}

/// Find all conversation files for a Claude Code project.
pub fn find_conversations(project_path: &Path) -> Vec<PathBuf> {
    // Claude Code stores projects in ~/.claude/projects/<path-hash>/
    let claude_dir = dirs_or_home().join(".claude").join("projects");

    // Try to find the matching project directory
    let project_str = project_path.to_string_lossy().replace('/', "-");
    // Remove leading dash if present
    let project_key = if project_str.starts_with('-') {
        project_str.clone()
    } else {
        format!("-{}", project_str)
    };

    let project_dir = claude_dir.join(&project_key);

    if !project_dir.exists() {
        // Try to find by scanning directory names
        if let Ok(entries) = std::fs::read_dir(&claude_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains(&project_path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy().to_string())
                {
                    return find_jsonl_files(&entry.path());
                }
            }
        }
        return Vec::new();
    }

    find_jsonl_files(&project_dir)
}

fn find_jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    // Sort by modification time (newest first)
    files.sort_by(|a, b| {
        let a_time = std::fs::metadata(a).and_then(|m| m.modified()).ok();
        let b_time = std::fs::metadata(b).and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });
    files
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_string() {
        let content = Some(serde_json::Value::String("Hello world".to_string()));
        assert_eq!(extract_text_content(&content), "Hello world");
    }

    #[test]
    fn test_extract_text_blocks() {
        let blocks = serde_json::json!([
            {"type": "thinking", "thinking": "internal thought"},
            {"type": "text", "text": "Hello user"},
            {"type": "tool_use", "name": "Read", "input": {"file_path": "/test.rs"}}
        ]);
        let content = Some(blocks);
        let result = extract_text_content(&content);
        assert!(result.contains("Hello user"));
        assert!(result.contains("[Tool: Read"));
        assert!(!result.contains("internal thought"));
    }

    #[test]
    fn test_extract_none() {
        assert_eq!(extract_text_content(&None), "");
    }
}
