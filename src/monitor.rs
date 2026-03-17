use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct UsageResult {
    pub used_tokens: u64,
    pub used_pct: u64,
    pub window_size: u64,
    pub should_extract: bool,
}

/// Read the last assistant message from a JSONL transcript and check context usage.
pub fn check_context_usage(
    transcript_path: &Path,
    threshold: u64,
    window_size: u64,
) -> Result<UsageResult, String> {
    let file =
        std::fs::File::open(transcript_path).map_err(|e| format!("Cannot open transcript: {}", e))?;
    let reader = BufReader::new(file);

    let mut last_usage: Option<(u64, u64, u64)> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Read error: {}", e))?;
        if line.trim().is_empty() {
            continue;
        }

        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // Skip malformed lines
        };

        if v.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            if let Some(usage) = v.pointer("/message/usage") {
                let input = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let cache_create = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let cache_read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                last_usage = Some((input, cache_create, cache_read));
            }
        }
    }

    let (input, cache_create, cache_read) =
        last_usage.ok_or_else(|| "No assistant message with usage found".to_string())?;

    let used = input + cache_create + cache_read;
    let pct = if window_size > 0 {
        (used * 100) / window_size
    } else {
        0
    };

    Ok(UsageResult {
        used_tokens: used,
        used_pct: pct,
        window_size,
        should_extract: pct >= threshold,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_usage_from_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":"hi","usage":{{"input_tokens":5000,"cache_creation_input_tokens":2000,"cache_read_input_tokens":1000,"output_tokens":500}}}}}}"#
        )
        .unwrap();

        let result = check_context_usage(&path, 85, 200000).unwrap();
        assert_eq!(result.used_tokens, 8000);
        assert_eq!(result.used_pct, 4);
        assert!(!result.should_extract);
    }

    #[test]
    fn test_threshold_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":"x","usage":{{"input_tokens":170000,"cache_creation_input_tokens":5000,"cache_read_input_tokens":3000,"output_tokens":1000}}}}}}"#
        )
        .unwrap();

        let result = check_context_usage(&path, 85, 200000).unwrap();
        assert_eq!(result.used_tokens, 178000);
        assert_eq!(result.used_pct, 89);
        assert!(result.should_extract);
    }

    #[test]
    fn test_uses_last_assistant_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        // First assistant message — low usage
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":"a","usage":{{"input_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":100}}}}}}"#
        )
        .unwrap();
        // Second assistant message — high usage (cumulative)
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":"b","usage":{{"input_tokens":180000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":200}}}}}}"#
        )
        .unwrap();

        let result = check_context_usage(&path, 85, 200000).unwrap();
        assert_eq!(result.used_tokens, 180000); // uses last, not first
        assert_eq!(result.used_pct, 90);
    }

    #[test]
    fn test_no_assistant_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();

        let result = check_context_usage(&path, 85, 200000);
        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_lines_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "not json at all").unwrap();
        writeln!(f, r#"{{"type":"garbage"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":"x","usage":{{"input_tokens":5000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":100}}}}}}"#
        )
        .unwrap();

        let result = check_context_usage(&path, 85, 200000).unwrap();
        assert_eq!(result.used_tokens, 5000);
    }
}
