use crate::graph::KnowledgeGraph;
use crate::model::Source;

/// Return what the KG knows about entities in a specific file.
/// Includes code entities from that file + semantic facts mentioning it.
pub fn file_context(kg: &KnowledgeGraph, file_path: &str, budget: usize) -> String {
    let char_budget = budget * 4;
    let mut output = String::new();
    let mut total = 0;

    let file_stem = std::path::Path::new(file_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file_path);

    // 1. Find code entities from this file
    for node in kg.all_nodes() {
        if let Source::CodeAnalysis { ref file } = node.source {
            if file == file_path || file_path.ends_with(file) || file.ends_with(file_path) {
                let line = format!("- **{}** ({}): {}\n", node.name, node.node_type, node.definition);
                if total + line.len() > char_budget {
                    break;
                }
                output.push_str(&line);
                total += line.len();
            }
        }
    }

    // 2. Find semantic facts related to this file
    for node in kg.all_nodes() {
        match &node.source {
            Source::Conversation | Source::Document { .. } | Source::Memory => {
                if node.name.contains(file_stem) || node.definition.contains(file_stem) {
                    let line =
                        format!("- **{}** ({}): {}\n", node.name, node.node_type, node.definition);
                    if total + line.len() > char_budget {
                        break;
                    }
                    output.push_str(&line);
                    total += line.len();
                }
            }
            _ => {}
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::KnowledgeGraph;
    use crate::model::*;

    #[test]
    fn test_file_context_finds_code_entities() {
        let mut kg = KnowledgeGraph::new();

        kg.add_node(Node::new(
            1,
            "chunk_text".into(),
            "Function".into(),
            "Main chunking function".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/chunker.rs".into(),
            },
        ))
        .unwrap();

        kg.add_node(Node::new(
            2,
            "split_sentences".into(),
            "Function".into(),
            "Helper for sentence splitting".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/chunker.rs".into(),
            },
        ))
        .unwrap();

        kg.add_node(Node::new(
            3,
            "resolve".into(),
            "Function".into(),
            "Entity resolver".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/resolver.rs".into(),
            },
        ))
        .unwrap();

        let output = file_context(&kg, "src/chunker.rs", 300);
        assert!(output.contains("chunk_text"));
        assert!(output.contains("split_sentences"));
        assert!(!output.contains("resolve")); // different file
    }

    #[test]
    fn test_file_context_includes_semantic_facts() {
        let mut kg = KnowledgeGraph::new();

        kg.add_node(Node::new(
            1,
            "chunker.rs sentence-aware decision".into(),
            "Decision".into(),
            "Use sentence boundaries in chunker.rs to avoid breaking entities".into(),
            0.9,
            Source::Conversation,
        ))
        .unwrap();

        let output = file_context(&kg, "src/chunker.rs", 300);
        assert!(output.contains("sentence-aware"));
    }

    #[test]
    fn test_file_context_empty_for_unknown_file() {
        let kg = KnowledgeGraph::new();
        let output = file_context(&kg, "src/nonexistent.rs", 300);
        assert!(output.is_empty());
    }

    #[test]
    fn test_file_context_respects_budget() {
        let mut kg = KnowledgeGraph::new();
        for i in 0..50 {
            kg.add_node(Node::new(
                i,
                format!("func_{}", i),
                "Function".into(),
                format!("Function number {}", i),
                1.0,
                Source::CodeAnalysis {
                    file: "src/big.rs".into(),
                },
            ))
            .unwrap();
        }
        let output = file_context(&kg, "src/big.rs", 20);
        // Very small budget — should not include all 50
        assert!(output.lines().count() < 10);
    }
}
