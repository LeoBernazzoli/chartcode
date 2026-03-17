use crate::graph::{Direction, KnowledgeGraph};
use crate::model::Source;

/// Analyze the impact of modifying an entity.
/// Returns markdown report of all references and breaking changes.
pub fn impact_analysis(kg: &KnowledgeGraph, entity_name: &str, depth: usize) -> String {
    let node = match kg.lookup(entity_name) {
        Some(n) => n,
        None => return format!("No entity found: {}", entity_name),
    };

    let node_id = node.id;
    let node_name = node.name.clone();
    let mut output = format!("## Impact: {}\n\n", node_name);

    // Get all neighbors (direct references)
    let neighbors = kg.neighbors(node_id);
    if neighbors.is_empty() {
        output.push_str("No references found.\n");
        return output;
    }

    output.push_str(&format!("**References ({}):**\n", neighbors.len()));
    let mut warnings: Vec<String> = Vec::new();

    for n in &neighbors {
        let file_info = match &n.node.source {
            Source::CodeAnalysis { file } => file.clone(),
            _ => "semantic".into(),
        };
        let dir_symbol = match n.direction {
            Direction::Outgoing => "→",
            Direction::Incoming => "←",
        };
        output.push_str(&format!(
            "- {} **{}** ({}) [{}] in {}\n",
            dir_symbol, n.node.name, n.relation_type, n.node.node_type, file_info
        ));

        // Detect breaking change patterns
        detect_breaking_changes(&n.node.source, &n.node.name, &n.node.node_type, &mut warnings);
    }

    // Depth > 1: follow indirect references
    if depth > 1 {
        let mut indirect: Vec<String> = Vec::new();
        for n in &neighbors {
            let second_hop = kg.neighbors(n.node.id);
            for n2 in &second_hop {
                if n2.node.id != node_id {
                    let file_info = match &n2.node.source {
                        Source::CodeAnalysis { file } => file.clone(),
                        _ => "semantic".into(),
                    };
                    indirect.push(format!(
                        "- {} → {} ({}) in {}",
                        n.node.name, n2.node.name, n2.relation_type, file_info
                    ));
                }
            }
        }
        if !indirect.is_empty() {
            indirect.dedup();
            output.push_str(&format!("\n**Indirect ({}):**\n", indirect.len()));
            for line in &indirect {
                output.push_str(line);
                output.push('\n');
            }
        }
    }

    if !warnings.is_empty() {
        output.push_str("\n**Breaking Changes:**\n");
        for w in &warnings {
            output.push_str(&format!("- {}\n", w));
        }
    }

    output
}

fn detect_breaking_changes(
    source: &Source,
    name: &str,
    node_type: &str,
    warnings: &mut Vec<String>,
) {
    if let Source::CodeAnalysis { file } = source {
        let file_lower = file.to_lowercase();
        if file_lower.contains("storage") || file_lower.contains("serial") {
            let w = format!("{}: field change may break deserialization of existing files", file);
            if !warnings.contains(&w) {
                warnings.push(w);
            }
        }
        if file_lower.contains("python") {
            let w = format!(
                "{}: change in {} '{}' breaks Python SDK",
                file, node_type, name
            );
            if !warnings.contains(&w) {
                warnings.push(w);
            }
        }
    }
}

/// Parse a tool input JSON (Edit/Write) and run impact analysis on affected entities.
/// Looks for entity names from the KG that appear in old_string.
pub fn impact_from_diff(kg: &KnowledgeGraph, tool_input: &str, depth: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(tool_input) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    let old_string = v
        .get("old_string")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if old_string.is_empty() {
        return String::new();
    }

    // Find entities whose names appear in the old_string being modified
    let mut affected: Vec<String> = Vec::new();
    for node in kg.all_nodes() {
        // Only check code entities and significant semantic entities
        let is_relevant = matches!(node.source, Source::CodeAnalysis { .. })
            || node.node_type == "Decision"
            || node.node_type == "ErrorResolution";

        if is_relevant && old_string.contains(&node.name) && node.name.len() >= 3 {
            affected.push(node.name.clone());
        }
    }

    if affected.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for entity in &affected {
        let report = impact_analysis(kg, entity, depth);
        if !report.contains("No references found") && !report.contains("No entity found") {
            output.push_str(&report);
            output.push('\n');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::KnowledgeGraph;
    use crate::model::*;

    fn make_code_kg() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();

        kg.add_node(Node::new(
            1,
            "confidence".into(),
            "Field".into(),
            "pub confidence: f32".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/model.rs".into(),
            },
        ))
        .unwrap();

        kg.add_node(Node::new(
            2,
            "relevance".into(),
            "Function".into(),
            "fn relevance() -> f64".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/graph.rs".into(),
            },
        ))
        .unwrap();

        kg.add_node(Node::new(
            3,
            "merge_nodes".into(),
            "Function".into(),
            "fn merge_nodes()".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/resolver.rs".into(),
            },
        ))
        .unwrap();

        kg.add_node(Node::new(
            4,
            "py_confidence".into(),
            "Method".into(),
            "Python SDK confidence property".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/python.rs".into(),
            },
        ))
        .unwrap();

        kg.add_node(Node::new(
            5,
            "save_node".into(),
            "Function".into(),
            "fn save_node() serializes to .kg".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/storage.rs".into(),
            },
        ))
        .unwrap();

        // Add edges: relevance reads confidence, merge_nodes reads confidence
        let e1 = Edge::new(1, 2, 1, "reads".into(), 1.0, Source::Inferred);
        kg.add_edge(e1).unwrap();
        let e2 = Edge::new(2, 3, 1, "reads".into(), 1.0, Source::Inferred);
        kg.add_edge(e2).unwrap();
        // py_confidence exposes confidence
        let e3 = Edge::new(3, 4, 1, "exposes".into(), 1.0, Source::Inferred);
        kg.add_edge(e3).unwrap();
        // save_node serializes confidence
        let e4 = Edge::new(4, 5, 1, "serializes".into(), 1.0, Source::Inferred);
        kg.add_edge(e4).unwrap();

        kg
    }

    #[test]
    fn test_impact_finds_references() {
        let kg = make_code_kg();
        let report = impact_analysis(&kg, "confidence", 1);
        assert!(report.contains("relevance"));
        assert!(report.contains("merge_nodes"));
        assert!(report.contains("References"));
    }

    #[test]
    fn test_impact_detects_breaking_changes() {
        let kg = make_code_kg();
        let report = impact_analysis(&kg, "confidence", 1);
        assert!(report.contains("python") || report.contains("Python"));
        assert!(report.contains("storage") || report.contains("serializ"));
    }

    #[test]
    fn test_impact_depth_2() {
        let mut kg = KnowledgeGraph::new();

        // A -> B -> C chain
        kg.add_node(Node::new(1, "field_a".into(), "Field".into(), "field a".into(), 1.0,
            Source::CodeAnalysis { file: "src/a.rs".into() })).unwrap();
        kg.add_node(Node::new(2, "func_b".into(), "Function".into(), "reads field_a".into(), 1.0,
            Source::CodeAnalysis { file: "src/b.rs".into() })).unwrap();
        kg.add_node(Node::new(3, "func_c".into(), "Function".into(), "calls func_b".into(), 1.0,
            Source::CodeAnalysis { file: "src/c.rs".into() })).unwrap();

        // B reads A, C calls B
        let e1 = Edge::new(1, 2, 1, "reads".into(), 1.0, Source::Inferred);
        kg.add_edge(e1).unwrap();
        let e2 = Edge::new(2, 3, 2, "calls".into(), 1.0, Source::Inferred);
        kg.add_edge(e2).unwrap();

        let report = impact_analysis(&kg, "field_a", 2);
        // Direct: func_b reads field_a
        assert!(report.contains("func_b"));
        // Indirect: func_c calls func_b
        assert!(report.contains("Indirect"));
        assert!(report.contains("func_c"));
    }

    #[test]
    fn test_impact_unknown_entity() {
        let kg = make_code_kg();
        let report = impact_analysis(&kg, "nonexistent_thing", 1);
        assert!(report.contains("No entity found"));
    }

    #[test]
    fn test_impact_no_references() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(Node::new(
            1,
            "isolated".into(),
            "Function".into(),
            "fn isolated()".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/test.rs".into(),
            },
        ))
        .unwrap();

        let report = impact_analysis(&kg, "isolated", 1);
        assert!(report.contains("No references found"));
    }

    #[test]
    fn test_impact_from_diff_finds_entities() {
        let kg = make_code_kg();
        let tool_input = r#"{
            "file_path": "src/model.rs",
            "old_string": "pub confidence: f32",
            "new_string": "pub certainty: f32"
        }"#;

        let report = impact_from_diff(&kg, tool_input, 1);
        assert!(report.contains("confidence"));
        assert!(report.contains("References"));
    }

    #[test]
    fn test_impact_from_diff_empty_old_string() {
        let kg = make_code_kg();
        let tool_input = r#"{
            "file_path": "src/model.rs",
            "old_string": "",
            "new_string": "new code"
        }"#;

        let report = impact_from_diff(&kg, tool_input, 1);
        assert!(report.is_empty());
    }

    #[test]
    fn test_impact_from_diff_invalid_json() {
        let kg = make_code_kg();
        let report = impact_from_diff(&kg, "not json", 1);
        assert!(report.is_empty());
    }

    #[test]
    fn test_impact_from_diff_no_matching_entities() {
        let kg = make_code_kg();
        let tool_input = r#"{
            "file_path": "src/new.rs",
            "old_string": "let x = 42;",
            "new_string": "let x = 43;"
        }"#;

        let report = impact_from_diff(&kg, tool_input, 1);
        assert!(report.is_empty());
    }
}
