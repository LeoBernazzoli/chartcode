use regex::Regex;

#[derive(Debug, PartialEq)]
pub enum FactType {
    Decision,
    Error,
    Relation,
    Supersession,
}

#[derive(Debug)]
pub struct ExtractedFact {
    pub fact_type: FactType,
    pub text: String,
}

/// Extract facts from conversation text using regex patterns.
/// No LLM needed — pure Rust, milliseconds.
pub fn extract_heuristic(text: &str) -> Vec<ExtractedFact> {
    let mut facts = Vec::new();

    let decision_patterns = [
        r"(?i)(?:we |let's |I'll |I )?(?:decided?|chose?|use|switch(?:ed|ing)? to|go(?:ing)? with)\s+(.{5,80})",
        r"(?i)the approach is\s+(.{5,80})",
    ];

    let error_patterns = [
        r"(?i)(?:the )?bug (?:is|was) (?:caused by|in|due to)\s+(.{5,80})",
        r"(?i)doesn't work because\s+(.{5,80})",
        r"(?i)(?:the )?(?:fix|solution) (?:is|was)\s+(.{5,80})",
    ];

    let supersession_patterns = [
        r"(?i)instead of\s+(\S+(?:\s+\S+){0,3})\s+(?:we'll |let's )?use\s+(.{3,40})",
        r"(?i)replac(?:e|ing)\s+(\S+(?:\s+\S+){0,3})\s+with\s+(.{3,40})",
    ];

    let relation_patterns = [
        r"(?i)(\S+(?:\.\S+)?)\s+(?:depends on|calls|imports|requires)\s+(\S+(?:\.\S+)?)",
        r"(?i)after changing\s+(\S+(?:\.\S+)?),?\s+(\S+(?:\.\S+)?)\s+broke",
    ];

    for pattern in &decision_patterns {
        let re = Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
            facts.push(ExtractedFact {
                fact_type: FactType::Decision,
                text: cap[0].trim().to_string(),
            });
        }
    }

    for pattern in &error_patterns {
        let re = Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
            facts.push(ExtractedFact {
                fact_type: FactType::Error,
                text: cap[0].trim().to_string(),
            });
        }
    }

    for pattern in &supersession_patterns {
        let re = Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
            facts.push(ExtractedFact {
                fact_type: FactType::Supersession,
                text: cap[0].trim().to_string(),
            });
        }
    }

    for pattern in &relation_patterns {
        let re = Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
            facts.push(ExtractedFact {
                fact_type: FactType::Relation,
                text: cap[0].trim().to_string(),
            });
        }
    }

    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_decision() {
        let text = "[User] let's use Levenshtein instead of LCS\n[Assistant] Good choice, switching to Levenshtein for better results.";
        let facts = extract_heuristic(text);
        assert!(!facts.is_empty());
        assert!(facts.iter().any(|f| f.fact_type == FactType::Decision));
    }

    #[test]
    fn test_extract_error() {
        let text = "[Assistant] The bug was caused by threshold being too low, generating false positives.";
        let facts = extract_heuristic(text);
        assert!(facts.iter().any(|f| f.fact_type == FactType::Error));
    }

    #[test]
    fn test_extract_supersession() {
        let text = "[User] instead of LCS we'll use Levenshtein distance";
        let facts = extract_heuristic(text);
        assert!(facts.iter().any(|f| f.fact_type == FactType::Supersession));
    }

    #[test]
    fn test_extract_relation() {
        let text = "[Assistant] resolver.rs depends on graph.rs for the merge operation";
        let facts = extract_heuristic(text);
        assert!(facts.iter().any(|f| f.fact_type == FactType::Relation));
    }

    #[test]
    fn test_no_extraction_from_noise() {
        let text = "[Assistant] Done. Let me know if you need anything else.";
        let facts = extract_heuristic(text);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_extract_fix() {
        let text = "[Assistant] The fix was to increase the threshold to 0.85 for better precision.";
        let facts = extract_heuristic(text);
        assert!(facts.iter().any(|f| f.fact_type == FactType::Error)); // fix pattern is under error
    }

    #[test]
    fn test_multiple_facts_from_same_text() {
        let text = "[User] let's use Levenshtein. The bug was caused by LCS failing on short names. instead of LCS let's use Levenshtein distance";
        let facts = extract_heuristic(text);
        assert!(facts.len() >= 2);
    }
}
