use std::path::Path;

use crate::graph::KnowledgeGraph;
use crate::model::Source;
use crate::pagerank::pagerank;
use crate::tier::ImportanceTier;

/// Generate `.claude/rules/` files from the KG.
/// Called at SessionStart to give Claude structured project knowledge.
pub fn sync_rules(kg: &KnowledgeGraph, rules_dir: &Path) {
    std::fs::create_dir_all(rules_dir).ok();

    // Generate project-map.md (always loaded)
    let map = generate_project_map(kg);
    std::fs::write(rules_dir.join("project-map.md"), map).ok();

    // Generate decisions.md (always loaded)
    let decisions = generate_decisions_rule(kg);
    std::fs::write(rules_dir.join("decisions.md"), decisions).ok();

    // Generate per-file rules (path-specific)
    let files = collect_source_files(kg);
    for file_path in files {
        let entity_count = kg
            .all_nodes()
            .filter(|n| matches!(&n.source, Source::CodeAnalysis { file } if file == &file_path))
            .filter(|n| n.node_type != "File" && n.node_type != "Import")
            .count();

        // Only generate rules for files with >3 meaningful entities
        if entity_count > 3 {
            let rule = generate_file_rule(kg, &file_path);
            let safe_name = file_path
                .replace('/', "-")
                .replace('\\', "-")
                .replace('.', "-");
            std::fs::write(rules_dir.join(format!("{}.md", safe_name)), rule).ok();
        }
    }
}

/// Collect unique source file paths from the KG.
fn collect_source_files(kg: &KnowledgeGraph) -> Vec<String> {
    let mut files: Vec<String> = kg
        .all_nodes()
        .filter_map(|n| match &n.source {
            Source::CodeAnalysis { file } => Some(file.clone()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    files.sort();
    files
}

/// Generate project-map.md — overview of all modules ranked by connectivity.
pub fn generate_project_map(kg: &KnowledgeGraph) -> String {
    let mut output = String::new();

    // Count totals
    let total_nodes = kg.all_nodes().count();
    let total_edges = kg.stats().edge_count;
    let files = collect_source_files(kg);
    let decision_count = kg
        .all_nodes()
        .filter(|n| {
            (n.node_type == "Decision" || n.node_type == "TechnicalFact")
                && n.tier == ImportanceTier::Critical
        })
        .count();

    output.push_str(&format!(
        "Progetto: {} file, {} entità, {} riferimenti cross-file\n",
        files.len(),
        total_nodes,
        total_edges
    ));
    if decision_count > 0 {
        output.push_str(&format!(
            "Decisioni architetturali attive: {} (autoclaw explore <nome>)\n",
            decision_count
        ));
    }
    output.push_str("Usa: autoclaw impact <entità> prima di rename/refactor\n\n");

    // Compute PageRank for file-level ranking
    let edges: Vec<(String, String)> = kg
        .all_nodes()
        .filter(|n| n.node_type == "File")
        .flat_map(|file_node| {
            let file_id = file_node.id;
            kg.neighbors(file_id)
                .into_iter()
                .filter_map(move |neighbor| {
                    // Get the file that defines the target entity
                    if let Source::CodeAnalysis { file } = &neighbor.node.source {
                        Some((file_node.name.clone(), file.clone()))
                    } else {
                        None
                    }
                })
        })
        .collect();

    let ranks = pagerank(&edges, 20, 0.85);

    // Build file stats: entity count + inbound reference count
    let mut file_stats: Vec<(String, usize, usize, f64)> = Vec::new(); // (file, entities, refs_in, rank)
    for file_path in &files {
        let entities = kg
            .all_nodes()
            .filter(|n| {
                matches!(&n.source, Source::CodeAnalysis { file } if file == file_path)
                    && n.node_type != "File"
                    && n.node_type != "Import"
            })
            .count();

        // Count edges pointing to entities in this file
        let refs_in: usize = kg
            .all_nodes()
            .filter(|n| {
                matches!(&n.source, Source::CodeAnalysis { file } if file == file_path)
                    && n.node_type != "File"
            })
            .map(|n| kg.inbound_reference_count(&n.name))
            .sum();

        let rank = ranks.get(file_path).copied().unwrap_or(0.0);
        if entities > 0 {
            file_stats.push((file_path.clone(), entities, refs_in, rank));
        }
    }

    // Sort by inbound references (most connected first)
    file_stats.sort_by(|a, b| b.2.cmp(&a.2));

    output.push_str("Moduli per connettività:\n");
    for (file, entities, refs_in, _) in &file_stats {
        let short = Path::new(file)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(file);
        let warning = if *refs_in > 100 {
            " ← toccare con cautela"
        } else if *refs_in == 0 {
            " ← leaf module"
        } else {
            ""
        };
        output.push_str(&format!(
            "  {} ({} entità, {} refs IN){}\n",
            short, entities, refs_in, warning
        ));
    }

    output
}

/// Generate decisions.md — imperative architectural decisions.
pub fn generate_decisions_rule(kg: &KnowledgeGraph) -> String {
    let mut output = String::new();

    let decisions: Vec<_> = kg
        .all_nodes()
        .filter(|n| {
            (n.node_type == "Decision" || n.node_type == "TechnicalFact")
                && (n.tier == ImportanceTier::Critical || n.tier == ImportanceTier::Significant)
                && n.superseded_by.is_none()
        })
        .collect();

    if decisions.is_empty() {
        output.push_str("Nessuna decisione architetturale registrata.\n");
        output.push_str("Usa /graphocode:decide per registrare decisioni.\n");
        return output;
    }

    for node in &decisions {
        let prefix = if node.tier == ImportanceTier::Critical {
            "NON VIOLARE"
        } else {
            "NOTA"
        };
        output.push_str(&format!("{}: {}\n", prefix, node.definition));
    }

    output.push_str("\nUSARE autoclaw impact prima di ogni rename o cambio di signature\n");
    output
}

/// Generate a path-specific rule file for a source file.
pub fn generate_file_rule(kg: &KnowledgeGraph, file_path: &str) -> String {
    let mut output = String::new();

    // YAML frontmatter
    output.push_str("---\npaths:\n");
    output.push_str(&format!("  - \"{}\"\n", file_path));
    output.push_str("---\n\n");

    // Collect entities in this file (excluding File and Import nodes)
    let mut entities: Vec<_> = kg
        .all_nodes()
        .filter(|n| {
            matches!(&n.source, Source::CodeAnalysis { file } if file == file_path)
                && n.node_type != "File"
                && n.node_type != "Import"
        })
        .collect();

    // Sort by PageRank (inbound reference count as proxy)
    entities.sort_by(|a, b| {
        let a_refs = kg.inbound_reference_count(&a.name);
        let b_refs = kg.inbound_reference_count(&b.name);
        b_refs.cmp(&a_refs)
    });

    // Group by type
    let mut structs: Vec<&crate::model::Node> = Vec::new();
    let mut functions: Vec<&crate::model::Node> = Vec::new();
    let mut other: Vec<&crate::model::Node> = Vec::new();

    for e in &entities {
        match e.node_type.as_str() {
            "Struct" | "Enum" | "Trait" => structs.push(e),
            "Function" | "Method" => functions.push(e),
            _ => other.push(e),
        }
    }

    // Structs with field reference info
    for s in &structs {
        let refs_in = kg.inbound_reference_count(&s.name);
        output.push_str(&format!("{}: {} refs IN\n", s.name, refs_in));

        // Find fields of this struct
        let prefix = format!("{}.", s.name);
        for field in entities
            .iter()
            .filter(|e| e.node_type == "Field" && e.name.starts_with(&prefix))
        {
            let field_short = field.name.strip_prefix(&prefix).unwrap_or(&field.name);
            let reads = kg.references_to(&field.name)
                .iter()
                .filter(|r| r.ref_type == crate::treesitter::RefType::ReadsField)
                .count();
            let writes = kg.references_to(&field.name)
                .iter()
                .filter(|r| r.ref_type == crate::treesitter::RefType::WritesField)
                .count();
            if reads > 0 || writes > 0 {
                output.push_str(&format!(
                    "  .{}: letto in {} file, scritto in {}\n",
                    field_short, reads, writes
                ));
            }
        }
    }

    // Top functions/methods (limit to 10)
    if !functions.is_empty() {
        output.push('\n');
        for f in functions.iter().take(10) {
            let refs_in = kg.inbound_reference_count(&f.name);
            if refs_in > 0 {
                output.push_str(&format!("{} ({}): {} refs IN\n", f.name, f.node_type, refs_in));
            }
        }
    }

    // Imperative instructions
    if !structs.is_empty() {
        output.push_str("\nSE MODIFICHI STRUCT: autoclaw impact <nome> per vedere tutti i riferimenti\n");
    }
    if !functions.is_empty() {
        output.push_str("SE RINOMINI FUNZIONE: autoclaw impact <nome> prima di procedere\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::KnowledgeGraph;
    use crate::model::{Node, Source};
    use crate::tier::ImportanceTier;

    #[test]
    fn test_generate_project_map() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/model.rs", "pub struct Node { pub id: u64 }");
        kg.reindex_file_v2("src/graph.rs", "fn test(n: Node) { let x = n.id; }");

        let map = generate_project_map(&kg);
        assert!(map.contains("model"), "Should mention model.rs: {}", map);
        assert!(map.contains("graph"), "Should mention graph.rs: {}", map);
        assert!(map.contains("refs IN"), "Should show ref counts: {}", map);
    }

    #[test]
    fn test_generate_decisions_rule() {
        let mut kg = KnowledgeGraph::new();
        let mut node = Node::new(
            1,
            "No decay for Critical".into(),
            "Decision".into(),
            "Critical tier never decays".into(),
            0.95,
            Source::Conversation,
        );
        node.tier = ImportanceTier::Critical;
        kg.add_node(node).unwrap();

        let rule = generate_decisions_rule(&kg);
        assert!(rule.contains("Critical tier never decays"));
        assert!(rule.contains("NON VIOLARE"));
    }

    #[test]
    fn test_generate_decisions_empty() {
        let kg = KnowledgeGraph::new();
        let rule = generate_decisions_rule(&kg);
        assert!(rule.contains("Nessuna decisione"));
    }

    #[test]
    fn test_generate_file_rule() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2(
            "src/model.rs",
            r#"
pub struct Node {
    pub id: u64,
    pub confidence: f32,
    pub name: String,
    pub tier: u8,
}
"#,
        );
        kg.reindex_file_v2("src/graph.rs", "fn r(n: &Node) { let c = n.confidence; }");

        let rule = generate_file_rule(&kg, "src/model.rs");
        assert!(rule.contains("paths:"), "Should have frontmatter: {}", rule);
        assert!(
            rule.contains("src/model.rs"),
            "Should reference file: {}",
            rule
        );
        assert!(rule.contains("Node"), "Should list Node struct: {}", rule);
    }

    #[test]
    fn test_sync_rules_writes_files() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join(".claude").join("rules");

        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2(
            "src/model.rs",
            "pub struct Node { pub id: u64, pub name: String, pub tier: u8, pub confidence: f32 }",
        );
        kg.reindex_file_v2("src/graph.rs", "fn r(n: Node) { let c = n.confidence; }");

        sync_rules(&kg, &rules_dir);

        assert!(rules_dir.join("project-map.md").exists());
        assert!(rules_dir.join("decisions.md").exists());
    }

    #[test]
    fn test_sync_rules_on_real_project() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join(".claude").join("rules");

        let mut kg = KnowledgeGraph::new();
        let config = crate::config::GraphocodeConfig {
            sources: crate::config::SourcesConfig {
                code: vec!["src/tier.rs".into(), "src/model.rs".into()],
                conversations: false,
                documents: vec![],
            },
            ..Default::default()
        };
        crate::bootstrap::bootstrap_code(&mut kg, &config);

        sync_rules(&kg, &rules_dir);

        let map = std::fs::read_to_string(rules_dir.join("project-map.md")).unwrap();
        assert!(map.contains("entità"), "Project map: {}", map);
        assert!(map.contains("refs IN"), "Project map: {}", map);
    }
}
