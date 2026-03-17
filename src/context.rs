use crate::graph::KnowledgeGraph;
use crate::model::Source;
use crate::tier::{relevance, ImportanceTier};

/// Generate markdown context for re-injection, ranked by relevance.
/// `budget` is approximate token count (~4 chars per token).
pub fn generate_context(kg: &KnowledgeGraph, budget: usize, now: u64) -> String {
    let char_budget = budget * 4;

    // Collect all nodes with relevance > 0
    let mut scored: Vec<(f64, ImportanceTier, &str, &str, &str)> = Vec::new();
    for node in kg.all_nodes() {
        let superseded = node.superseded_by.is_some();
        let is_code = matches!(node.source, Source::CodeAnalysis { .. });
        let r = relevance(node.tier, node.created_at, now, superseded, is_code);
        if r > 0.01 {
            scored.push((r, node.tier, &node.name, &node.definition, &node.node_type));
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut output = String::from("## Knowledge Graph Context\n");
    let mut current_tier: Option<ImportanceTier> = None;
    let mut total_chars = output.len();

    for (_rel, tier, name, definition, node_type) in &scored {
        // Add tier header if changed
        if current_tier != Some(*tier) {
            let header = match tier {
                ImportanceTier::Critical => "\n### Critical\n",
                ImportanceTier::Significant => "\n### Significant\n",
                ImportanceTier::Minor => "\n### Minor\n",
            };
            if total_chars + header.len() > char_budget {
                break;
            }
            output.push_str(header);
            total_chars += header.len();
            current_tier = Some(*tier);
        }

        let line = format!("- **{}** ({}): {}\n", name, node_type, definition);
        if total_chars + line.len() > char_budget {
            break;
        }
        output.push_str(&line);
        total_chars += line.len();
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::KnowledgeGraph;
    use crate::model::*;
    use crate::tier::ImportanceTier;

    fn make_test_kg() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();

        let mut n1 = Node::new(
            1,
            "Storage decision".into(),
            "Decision".into(),
            "Use .kg file, no database".into(),
            0.95,
            Source::Conversation,
        );
        n1.tier = ImportanceTier::Critical;
        kg.add_node(n1).unwrap();

        let mut n2 = Node::new(
            2,
            "LCS choice".into(),
            "Decision".into(),
            "Use LCS for fuzzy matching".into(),
            0.9,
            Source::Conversation,
        );
        n2.tier = ImportanceTier::Significant;
        kg.add_node(n2).unwrap();

        let mut n3 = Node::new(
            3,
            "variable rename".into(),
            "TechnicalFact".into(),
            "Renamed x to count".into(),
            0.5,
            Source::Conversation,
        );
        n3.tier = ImportanceTier::Minor;
        n3.created_at = 0; // old
        kg.add_node(n3).unwrap();

        kg
    }

    #[test]
    fn test_context_includes_critical_first() {
        let kg = make_test_kg();
        let now = 100 * 86400;
        let output = generate_context(&kg, 2000, now);
        assert!(output.contains("Storage decision"));
        assert!(output.contains("### Critical"));
        let crit_pos = output.find("Storage decision").unwrap();
        let sig_pos = output.find("LCS choice").unwrap();
        assert!(crit_pos < sig_pos);
    }

    #[test]
    fn test_context_respects_budget() {
        let kg = make_test_kg();
        let output = generate_context(&kg, 30, 100 * 86400);
        // Very small budget — should truncate
        assert!(output.len() < 200);
    }

    #[test]
    fn test_context_excludes_superseded() {
        let mut kg = make_test_kg();
        // Supersede node 2 (LCS choice) — need to find its actual ID
        let lcs_id = kg.lookup("LCS choice").unwrap().id;
        kg.get_node_mut(lcs_id).unwrap().superseded_by = Some(99);
        let output = generate_context(&kg, 2000, 100 * 86400);
        assert!(!output.contains("LCS choice"));
    }

    #[test]
    fn test_context_empty_kg() {
        let kg = KnowledgeGraph::new();
        let output = generate_context(&kg, 2000, 100 * 86400);
        assert!(output.contains("Knowledge Graph Context"));
        // Should just be the header, nothing else significant
        assert!(!output.contains("### Critical"));
    }
}
