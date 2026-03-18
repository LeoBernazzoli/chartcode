use crate::chunker;
use crate::claude_parser;
use crate::config::GraphocodeConfig;
use crate::graph::KnowledgeGraph;
use crate::model::{Node, Source};
use crate::tier::ImportanceTier;
use crate::treesitter;
use std::path::Path;

#[derive(Debug)]
pub struct BootstrapReport {
    pub files_indexed: usize,
    pub code_entities: usize,
    pub conversations_found: usize,
    pub conversation_texts: Vec<(String, String)>, // (session_id, text) for Haiku extraction
    pub document_chunks: Vec<(String, String)>,    // (doc_path, chunk_text) for Haiku extraction
}

/// CHANNEL 1: Index all code files using tree-sitter. Deterministic, 0 tokens.
/// V2: extracts both definitions AND references for complete code graph.
pub fn bootstrap_code(kg: &mut KnowledgeGraph, config: &GraphocodeConfig) -> (usize, usize) {
    let mut files = 0;
    let mut entities = 0;

    // First pass: add all entities (definitions)
    let mut file_list: Vec<(String, String)> = Vec::new(); // (path, code)
    for pattern in &config.sources.code {
        if let Ok(paths) = glob::glob(pattern) {
            for entry in paths.flatten() {
                let path_str = entry.to_string_lossy().to_string();
                if !path_str.ends_with(".rs") {
                    continue;
                }
                if let Ok(code) = std::fs::read_to_string(&entry) {
                    file_list.push((path_str, code));
                }
            }
        }
    }

    // Pass 1: definitions only (so all entities exist before we resolve references)
    for (path_str, code) in &file_list {
        let parsed = treesitter::parse_rust_code(code, path_str);
        entities += parsed.len();
        for entity in parsed {
            let mut node = Node::new(
                0,
                entity.name,
                entity.entity_type,
                entity.definition,
                1.0,
                Source::CodeAnalysis {
                    file: path_str.clone(),
                },
            );
            node.tier = ImportanceTier::Minor;
            let _ = kg.add_node(node);
        }
        files += 1;
    }

    // Pass 2: references (now all entities exist, so lookup works)
    for (path_str, code) in &file_list {
        let (_, references) = treesitter::parse_rust_code_v2(code, path_str);

        // Create file-level node
        let file_node_name = path_str.to_string();
        let file_node_id = if let Some(n) = kg.lookup(&file_node_name) {
            n.id
        } else {
            let mut fnode = Node::new(
                0,
                file_node_name,
                "File".to_string(),
                format!("Source file {}", path_str),
                1.0,
                Source::CodeAnalysis {
                    file: path_str.clone(),
                },
            );
            fnode.tier = ImportanceTier::Minor;
            kg.add_node(fnode).unwrap_or(0)
        };

        // Add reference edges
        for reference in references {
            if let Some(target) = kg.lookup(&reference.target_name) {
                let target_id = target.id;
                if file_node_id != 0 && file_node_id != target_id {
                    let ref_type_str = match reference.ref_type {
                        treesitter::RefType::Calls => "calls",
                        treesitter::RefType::ReadsField => "reads",
                        treesitter::RefType::WritesField => "writes",
                        treesitter::RefType::UsesType => "uses_type",
                        treesitter::RefType::MethodCall => "calls",
                    };
                    let edge = crate::model::Edge::new(
                        0,
                        file_node_id,
                        target_id,
                        ref_type_str.to_string(),
                        1.0,
                        Source::CodeAnalysis {
                            file: path_str.clone(),
                        },
                    );
                    let _ = kg.add_edge(edge);
                }
            }
        }
    }

    (files, entities)
}

/// CHANNEL 2: Parse all Claude Code conversations. Deterministic parsing (0 tokens).
/// Returns Vec<(session_id, text)> ready for Haiku semantic extraction.
pub fn bootstrap_conversations(project_path: &Path) -> Vec<(String, String)> {
    let conversation_files = claude_parser::find_conversations(project_path);
    let mut results = Vec::new();

    for path in conversation_files {
        if let Some(conv) = claude_parser::parse_conversation(&path) {
            // Skip automated sessions
            if conv.is_automated() || conv.messages.len() < 3 {
                continue;
            }
            let text = conv.substantive_text(50000);
            if !text.is_empty() {
                results.push((conv.session_id.clone(), text));
            }
        }
    }
    results
}

/// CHANNEL 3: Process business documents. Returns chunked text for Haiku extraction.
pub fn bootstrap_documents(config: &GraphocodeConfig) -> Vec<(String, String)> {
    let mut results = Vec::new();
    for doc_path in &config.sources.documents {
        if let Ok(content) = std::fs::read_to_string(doc_path) {
            let chunks = chunker::chunk_text(&content, 4000, 500);
            for chunk in chunks {
                results.push((doc_path.clone(), chunk.text));
            }
        }
    }
    results
}

/// Full bootstrap: runs all three channels.
/// Code indexing is done entirely in Rust (deterministic, 0 tokens).
/// Conversations and documents produce text that needs to be passed to Haiku
/// for semantic extraction (done by the /graphocode:start skill which orchestrates LLM calls).
pub fn bootstrap(
    kg: &mut KnowledgeGraph,
    config: &GraphocodeConfig,
    project_path: &Path,
) -> BootstrapReport {
    let (files, code_entities) = bootstrap_code(kg, config);

    let conversation_texts = if config.sources.conversations {
        bootstrap_conversations(project_path)
    } else {
        vec![]
    };

    let document_chunks = bootstrap_documents(config);

    BootstrapReport {
        files_indexed: files,
        code_entities,
        conversations_found: conversation_texts.len(),
        conversation_texts,
        document_chunks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_code_on_self() {
        // Bootstrap the autoclaw project itself
        let mut kg = KnowledgeGraph::new();
        let config = GraphocodeConfig {
            sources: crate::config::SourcesConfig {
                code: vec!["src/**/*.rs".into()],
                conversations: false,
                documents: vec![],
            },
            ..GraphocodeConfig::default()
        };

        let (files, entities) = bootstrap_code(&mut kg, &config);

        // We should find our own source files
        assert!(files > 0, "Should index at least one .rs file");
        assert!(entities > 0, "Should extract at least one entity");

        // Verify we can find known entities from our codebase
        let has_knowledge_graph = kg.all_nodes().any(|n| n.name == "KnowledgeGraph");
        assert!(
            has_knowledge_graph,
            "Should find KnowledgeGraph struct in our code"
        );
    }

    #[test]
    fn test_bootstrap_code_entities_are_code_analysis_source() {
        let mut kg = KnowledgeGraph::new();
        let config = GraphocodeConfig {
            sources: crate::config::SourcesConfig {
                code: vec!["src/tier.rs".into()],
                conversations: false,
                documents: vec![],
            },
            ..GraphocodeConfig::default()
        };

        bootstrap_code(&mut kg, &config);

        for node in kg.all_nodes() {
            assert!(
                matches!(node.source, Source::CodeAnalysis { .. }),
                "All bootstrapped entities should have CodeAnalysis source"
            );
            assert_eq!(
                node.tier,
                ImportanceTier::Minor,
                "Code entities should be Minor tier"
            );
        }
    }

    #[test]
    fn test_bootstrap_documents_with_nonexistent() {
        let config = GraphocodeConfig {
            sources: crate::config::SourcesConfig {
                code: vec![],
                conversations: false,
                documents: vec!["nonexistent.md".into()],
            },
            ..GraphocodeConfig::default()
        };

        let chunks = bootstrap_documents(&config);
        assert!(chunks.is_empty()); // gracefully handles missing files
    }

    #[test]
    fn test_bootstrap_documents_with_real_file() {
        let config = GraphocodeConfig {
            sources: crate::config::SourcesConfig {
                code: vec![],
                conversations: false,
                documents: vec!["README.md".into()],
            },
            ..GraphocodeConfig::default()
        };

        let chunks = bootstrap_documents(&config);
        // README.md exists in our project
        if std::path::Path::new("README.md").exists() {
            assert!(!chunks.is_empty());
            assert_eq!(chunks[0].0, "README.md");
        }
    }

    #[test]
    fn test_full_bootstrap_report() {
        let mut kg = KnowledgeGraph::new();
        let config = GraphocodeConfig {
            sources: crate::config::SourcesConfig {
                code: vec!["src/tier.rs".into()],
                conversations: false,
                documents: vec![],
            },
            ..GraphocodeConfig::default()
        };

        let report = bootstrap(&mut kg, &config, Path::new("."));
        assert!(report.files_indexed > 0);
        assert!(report.code_entities > 0);
        assert!(report.conversation_texts.is_empty()); // disabled
        assert!(report.document_chunks.is_empty()); // none configured
    }
}
