use crate::graph::KnowledgeGraph;

/// Find KG facts relevant to a text query.
/// Extracts keywords, matches against entity names and definitions.
pub fn find_relevant(kg: &KnowledgeGraph, query: &str, budget: usize) -> String {
    let char_budget = budget * 4;
    let keywords: Vec<String> = query
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .map(|w| w.to_lowercase())
        .collect();

    if keywords.is_empty() {
        return String::new();
    }

    // Score each node by keyword matches in name + definition
    let mut matches: Vec<(f64, &str, &str, &str)> = Vec::new();
    for node in kg.all_nodes() {
        let name_lower = node.name.to_lowercase();
        let def_lower = node.definition.to_lowercase();
        let mut score = 0.0;
        for kw in &keywords {
            if name_lower.contains(kw) {
                score += 2.0;
            }
            if def_lower.contains(kw) {
                score += 1.0;
            }
        }
        if score > 0.0 {
            matches.push((score, &node.name, &node.node_type, &node.definition));
        }
    }

    if matches.is_empty() {
        return String::new();
    }

    matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut output = String::new();
    let mut total = 0;
    for (_, name, ntype, def) in matches {
        let line = format!("- **{}** ({}): {}\n", name, ntype, def);
        if total + line.len() > char_budget {
            break;
        }
        output.push_str(&line);
        total += line.len();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::KnowledgeGraph;
    use crate::model::*;

    fn make_kg() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();

        kg.add_node(Node::new(
            1,
            "chunker.rs".into(),
            "File".into(),
            "Text chunking module with sentence-aware splitting".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/chunker.rs".into(),
            },
        ))
        .unwrap();

        kg.add_node(Node::new(
            2,
            "sentence-aware splitting".into(),
            "Decision".into(),
            "Use sentence boundaries for chunking to avoid breaking entities".into(),
            0.9,
            Source::Conversation,
        ))
        .unwrap();

        kg.add_node(Node::new(
            3,
            "resolver.rs".into(),
            "File".into(),
            "Entity resolution module with fuzzy matching".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/resolver.rs".into(),
            },
        ))
        .unwrap();

        kg
    }

    #[test]
    fn test_relevant_finds_by_keyword() {
        let kg = make_kg();
        let result = find_relevant(&kg, "chunker splitting", 500);
        assert!(result.contains("chunker.rs"));
        assert!(result.contains("sentence-aware"));
    }

    #[test]
    fn test_relevant_no_match() {
        let kg = make_kg();
        let result = find_relevant(&kg, "database migration deploy", 500);
        assert!(result.is_empty());
    }

    #[test]
    fn test_relevant_respects_budget() {
        let kg = make_kg();
        let result = find_relevant(&kg, "chunker resolver", 10);
        // Very small budget — should truncate
        assert!(result.len() < 100);
    }

    #[test]
    fn test_relevant_empty_query() {
        let kg = make_kg();
        let result = find_relevant(&kg, "", 500);
        assert!(result.is_empty());
    }

    #[test]
    fn test_relevant_short_words_filtered() {
        let kg = make_kg();
        let result = find_relevant(&kg, "a to be", 500);
        assert!(result.is_empty()); // all words <= 2 chars
    }
}
