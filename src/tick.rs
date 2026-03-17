use crate::monitor::check_context_usage;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum TickAction {
    None,     // Under threshold, not at snapshot interval
    Snapshot, // Time for a lightweight heuristic snapshot
    Extract,  // Context threshold reached — deep extraction needed
}

pub struct TickResult {
    pub action: TickAction,
    pub counter: u64,
    pub used_pct: u64,
}

/// Combined monitor + periodic snapshot. Called on every PostToolUse.
/// Returns what action should be taken.
pub fn tick(
    transcript_path: &Path,
    counter_file: &Path,
    snapshot_every: u64,
    threshold: u64,
    window_size: u64,
) -> TickResult {
    // 1. Increment counter
    let prev = std::fs::read_to_string(counter_file)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let current = prev + 1;

    // 2. Check context threshold first (takes priority)
    let used_pct = if let Ok(usage) = check_context_usage(transcript_path, threshold, window_size) {
        if usage.should_extract {
            let _ = std::fs::write(counter_file, "0");
            return TickResult {
                action: TickAction::Extract,
                counter: 0,
                used_pct: usage.used_pct,
            };
        }
        usage.used_pct
    } else {
        0
    };

    // 3. Check if snapshot interval reached
    if current >= snapshot_every {
        let _ = std::fs::write(counter_file, "0");
        return TickResult {
            action: TickAction::Snapshot,
            counter: 0,
            used_pct,
        };
    }

    // 4. No action needed
    let _ = std::fs::write(counter_file, current.to_string());
    TickResult {
        action: TickAction::None,
        counter: current,
        used_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_transcript(dir: &tempfile::TempDir, input_tokens: u64) -> std::path::PathBuf {
        let path = dir.path().join("session.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":"x","usage":{{"input_tokens":{},"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":100}}}}}}"#,
            input_tokens
        )
        .unwrap();
        path
    }

    #[test]
    fn test_tick_no_action_below_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = write_transcript(&dir, 5000);
        let counter_file = dir.path().join(".tick_counter");

        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.action, TickAction::None);
        assert_eq!(result.counter, 1);
    }

    #[test]
    fn test_tick_increments_counter() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = write_transcript(&dir, 5000);
        let counter_file = dir.path().join(".tick_counter");

        tick(&transcript, &counter_file, 20, 85, 200000);
        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.counter, 2);

        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.counter, 3);
    }

    #[test]
    fn test_tick_triggers_snapshot_at_interval() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = write_transcript(&dir, 5000);
        let counter_file = dir.path().join(".tick_counter");

        // Simulate 19 previous ticks
        std::fs::write(&counter_file, "19").unwrap();

        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.action, TickAction::Snapshot);
        assert_eq!(result.counter, 0); // reset

        // Next tick should be 1 again
        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.action, TickAction::None);
        assert_eq!(result.counter, 1);
    }

    #[test]
    fn test_tick_triggers_extract_at_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = write_transcript(&dir, 178000); // 89% of 200000
        let counter_file = dir.path().join(".tick_counter");

        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.action, TickAction::Extract);
        assert_eq!(result.used_pct, 89);
    }

    #[test]
    fn test_tick_extract_takes_priority_over_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = write_transcript(&dir, 178000); // 89%
        let counter_file = dir.path().join(".tick_counter");

        // At snapshot interval AND above threshold — extract wins
        std::fs::write(&counter_file, "19").unwrap();

        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.action, TickAction::Extract);
    }

    #[test]
    fn test_tick_handles_missing_counter_file() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = write_transcript(&dir, 5000);
        let counter_file = dir.path().join("nonexistent_counter");

        let result = tick(&transcript, &counter_file, 20, 85, 200000);
        assert_eq!(result.action, TickAction::None);
        assert_eq!(result.counter, 1);
    }
}
