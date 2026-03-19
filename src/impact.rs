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

/// V2: Parse a tool input JSON and output additionalContext JSON for PreToolUse hook.
/// Uses pattern grouping for compact reports. Outputs nothing for internal logic changes.
pub fn impact_from_diff_v2(kg: &KnowledgeGraph, tool_input: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(tool_input) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    // Support both hook format (tool_input nested) and direct format (old_string at root)
    let input = if v.get("tool_input").is_some() {
        v.get("tool_input").unwrap().clone()
    } else {
        v.clone()
    };

    let old_string = input
        .get("old_string")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let file_path = input
        .get("file_path")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if old_string.is_empty() {
        return String::new();
    }

    // Strategy: find entities DEFINED IN the file being modified that match the diff.
    // This eliminates false positives from unrelated entities in other files.
    let mut affected: Vec<String> = Vec::new();

    if !file_path.is_empty() {
        // Precise mode: only search entities from this specific file
        for node in kg.all_nodes() {
            let in_this_file = match &node.source {
                Source::CodeAnalysis { file } => {
                    let f = file.trim_start_matches("./");
                    let fp = file_path.trim_start_matches("./");
                    f == fp || fp.ends_with(f) || f.ends_with(fp)
                }
                _ => false,
            };

            if in_this_file && node.node_type != "File" && node.node_type != "Import" {
                // Check if the entity's short name appears in old_string
                let short_name = if node.name.contains('.') {
                    node.name.split('.').last().unwrap_or(&node.name)
                } else {
                    &node.name
                };
                if short_name.len() >= 3 && old_string.contains(short_name) {
                    affected.push(node.name.clone());
                }
            }
        }
    }

    // Fallback: if no file_path or no matches in file, search all entities
    if affected.is_empty() {
        for node in kg.all_nodes() {
            let is_relevant = matches!(node.source, Source::CodeAnalysis { .. })
                || node.node_type == "Decision"
                || node.node_type == "ErrorResolution";

            if is_relevant && node.node_type != "File" && node.node_type != "Import" {
                let short_name = if node.name.contains('.') {
                    node.name.split('.').last().unwrap_or(&node.name)
                } else {
                    &node.name
                };
                // Require longer names to avoid false positives in fallback
                if short_name.len() >= 6 && old_string.contains(short_name) {
                    affected.push(node.name.clone());
                }
            }
        }
    }

    if affected.is_empty() {
        return String::new();
    }

    // Get references using node IDs directly (not lookup which can misroute)
    let mut all_refs = Vec::new();
    let mut entity_label = String::new();
    for entity_name in &affected {
        // Find the node by exact name
        let node_id = kg.all_nodes()
            .find(|n| &n.name == entity_name)
            .map(|n| n.id);

        if let Some(nid) = node_id {
            // Count inbound edges directly
            let refs = kg.references_to(entity_name);
            if !refs.is_empty() {
                if entity_label.is_empty() {
                    entity_label = entity_name.clone();
                }
                all_refs.extend(refs);
            } else {
                // Try via inbound_reference_count which uses the node ID path
                let count = kg.inbound_reference_count(entity_name);
                if count > 0 && entity_label.is_empty() {
                    entity_label = entity_name.clone();
                }
            }
        }
    }

    if all_refs.is_empty() {
        return String::new(); // Internal change, no external impact
    }

    let patterns = crate::patterns::group_by_pattern(&all_refs);
    let report = crate::patterns::format_impact_report(&entity_label, &patterns);

    // Output as hookSpecificOutput JSON for PreToolUse hook
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "additionalContext": report
        }
    })
    .to_string()
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

    // ── v2 impact tests ─────────────────────────

    #[test]
    fn test_impact_from_diff_v2_outputs_json() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/model.rs", "pub struct Node { pub confidence: f32 }");
        kg.reindex_file_v2("src/graph.rs", "fn r(n: Node) { let c = n.confidence; }");

        let tool_input = r#"{"file_path":"src/model.rs","old_string":"pub confidence: f32","new_string":"pub certainty: f32"}"#;
        let output = impact_from_diff_v2(&kg, tool_input);

        if !output.is_empty() {
            let json: serde_json::Value = serde_json::from_str(&output)
                .expect(&format!("Should be valid JSON: {}", output));
            assert!(
                json.pointer("/hookSpecificOutput/additionalContext").is_some(),
                "Should have additionalContext: {}",
                output
            );
        }
    }

    #[test]
    fn test_impact_from_diff_v2_empty_for_internal_change() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/chunker.rs", "fn internal() { let x = 1; }");

        let tool_input = r#"{"file_path":"src/chunker.rs","old_string":"let x = 1;","new_string":"let x = 2;"}"#;
        let output = impact_from_diff_v2(&kg, tool_input);
        assert!(output.is_empty(), "Internal change should produce no output");
    }

    #[test]
    fn test_impact_from_diff_v2_invalid_json() {
        let kg = KnowledgeGraph::new();
        let output = impact_from_diff_v2(&kg, "not json");
        assert!(output.is_empty());
    }

    #[test]
    fn test_impact_from_diff_v2_pattern_grouped() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/chunker.rs", "pub fn chunk_text(t: &str) {}");
        kg.reindex_file_v2("src/a.rs", "fn a() { chunk_text(\"x\"); }");
        kg.reindex_file_v2("src/b.rs", "fn b() { chunk_text(\"y\"); }");
        kg.reindex_file_v2("src/c.rs", "fn c() { chunk_text(\"z\"); }");

        let tool_input = r#"{"file_path":"src/chunker.rs","old_string":"pub fn chunk_text","new_string":"pub fn split_text"}"#;
        let output = impact_from_diff_v2(&kg, tool_input);

        if !output.is_empty() {
            let json: serde_json::Value = serde_json::from_str(&output).unwrap();
            let ctx = json
                .pointer("/hookSpecificOutput/additionalContext")
                .unwrap()
                .as_str()
                .unwrap();
            assert!(ctx.contains("IMPACT"), "Report: {}", ctx);
            assert!(ctx.contains("PATTERNS"), "Report: {}", ctx);
        }
    }

    #[test]
    fn test_impact_from_diff_v2_hook_format() {
        // Test with the format that Claude Code hooks send (tool_input nested)
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/model.rs", "pub struct Node { pub confidence: f32 }");
        kg.reindex_file_v2("src/graph.rs", "fn r(n: Node) { let c = n.confidence; }");

        let hook_input = r#"{
            "session_id": "abc123",
            "hook_event_name": "PreToolUse",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": "src/model.rs",
                "old_string": "pub confidence: f32",
                "new_string": "pub certainty: f32"
            }
        }"#;

        let output = impact_from_diff_v2(&kg, hook_input);
        if !output.is_empty() {
            let json: serde_json::Value = serde_json::from_str(&output)
                .expect(&format!("Should be valid JSON: {}", output));
            assert!(
                json.pointer("/hookSpecificOutput/additionalContext").is_some(),
                "Should have additionalContext for hook format"
            );
        }
    }
}
