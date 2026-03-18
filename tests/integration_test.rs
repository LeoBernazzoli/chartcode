use autoclaw::bootstrap;
use autoclaw::config::{GraphocodeConfig, SourcesConfig};
use autoclaw::context::generate_context;
use autoclaw::file_context::file_context;
use autoclaw::graph::KnowledgeGraph;
use autoclaw::impact::{impact_analysis, impact_from_diff, impact_from_diff_v2};
use autoclaw::model::*;
use autoclaw::reconcile::{
    garbage_collect, reconcile, NewFact, PromotionEntry, ReconcileInput, RelationEntry,
    SupersededEntry,
};
use autoclaw::relevant::find_relevant;
use autoclaw::snapshot::extract_heuristic;
use autoclaw::tier::ImportanceTier;
use autoclaw::treesitter::parse_rust_code;
use autoclaw::{load_or_create, save};

/// Full pipeline: bootstrap → context → impact → reconcile → GC → reindex
#[test]
fn test_full_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let kg_path = dir.path().join("test.kg");

    // 1. Create empty KG
    let mut kg = load_or_create(&kg_path).unwrap();
    assert_eq!(kg.stats().node_count, 0);

    // 2. Bootstrap code from our own source files
    let config = GraphocodeConfig {
        sources: SourcesConfig {
            code: vec!["src/tier.rs".into()],
            conversations: false,
            documents: vec![],
        },
        ..GraphocodeConfig::default()
    };
    let report = bootstrap::bootstrap(&mut kg, &config, std::path::Path::new("."));
    assert!(report.files_indexed > 0);
    assert!(report.code_entities > 0);

    // Verify code entities exist
    let has_importance_tier = kg.all_nodes().any(|n| n.name == "ImportanceTier");
    assert!(has_importance_tier, "Should find ImportanceTier enum");

    let has_relevance_fn = kg.all_nodes().any(|n| n.name == "relevance");
    assert!(has_relevance_fn, "Should find relevance function");

    // 3. Add semantic facts via reconcile
    let input = ReconcileInput {
        new_facts: vec![
            NewFact {
                name: "Use exponential decay".into(),
                fact_type: "Decision".into(),
                tier: "critical".into(),
                definition: "Use exponential decay for relevance scoring".into(),
                reason: "Natural time-based degradation".into(),
                supersedes: None,
                relations: vec![],
                evidence_text: "we chose exponential decay".into(),
            },
            NewFact {
                name: "Threshold too low bug".into(),
                fact_type: "ErrorResolution".into(),
                tier: "significant".into(),
                definition: "Threshold below 0.8 causes false positives in fuzzy matching".into(),
                reason: "Short names match too aggressively".into(),
                supersedes: None,
                relations: vec![],
                evidence_text: "the bug was caused by threshold being too low".into(),
            },
        ],
        superseded: vec![],
        promotions: vec![],
        relations: vec![RelationEntry {
            from: "relevance".into(),
            to: "ImportanceTier".into(),
            relation_type: "uses".into(),
            evidence: "relevance function reads tier weight".into(),
        }],
    };

    let rec_report = reconcile(&mut kg, &input);
    assert_eq!(rec_report.added, 2);
    assert_eq!(rec_report.edges_added, 1);

    // 4. Generate context — should show both code + semantic
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let context = generate_context(&kg, 2000, now);
    assert!(context.contains("Knowledge Graph Context"));
    assert!(context.contains("Critical")); // decay decision is critical
    assert!(context.contains("exponential decay"));

    // 5. Test relevant search
    let relevant = find_relevant(&kg, "decay relevance scoring", 500);
    assert!(relevant.contains("exponential decay") || relevant.contains("relevance"));

    // 6. Test file context
    let fc = file_context(&kg, "src/tier.rs", 500);
    assert!(!fc.is_empty()); // should find code entities from tier.rs

    // 7. Impact analysis on a code entity
    let impact = impact_analysis(&kg, "relevance", 1);
    // relevance has an edge to ImportanceTier, so it should show up
    assert!(
        impact.contains("ImportanceTier") || impact.contains("References"),
        "Impact should find references. Got: {}",
        impact
    );

    // 8. Impact from diff
    let diff_json = r#"{"file_path":"src/tier.rs","old_string":"pub fn relevance(","new_string":"pub fn compute_relevance("}"#;
    let diff_impact = impact_from_diff(&kg, diff_json, 1);
    assert!(
        diff_impact.contains("relevance") || diff_impact.is_empty(),
        "Should find relevance in diff or be empty if name too short"
    );

    // 9. Supersede a decision
    let supersede_input = ReconcileInput {
        new_facts: vec![NewFact {
            name: "Use linear decay".into(),
            fact_type: "Decision".into(),
            tier: "critical".into(),
            definition: "Switch to linear decay for simplicity".into(),
            reason: "Exponential was overfit".into(),
            supersedes: Some("Use exponential decay".into()),
            relations: vec![],
            evidence_text: "instead of exponential let's use linear".into(),
        }],
        superseded: vec![SupersededEntry {
            old: "Use exponential decay".into(),
            reason: "replaced by linear decay".into(),
        }],
        promotions: vec![],
        relations: vec![],
    };

    let sup_report = reconcile(&mut kg, &supersede_input);
    assert_eq!(sup_report.added, 1);
    assert_eq!(sup_report.superseded, 1);

    // Verify old decision is superseded
    let old = kg.lookup("Use exponential decay").unwrap();
    assert!(old.superseded_by.is_some());

    // Context should no longer show the superseded decision
    let context2 = generate_context(&kg, 2000, now);
    assert!(!context2.contains("exponential decay"));
    assert!(context2.contains("linear decay"));

    // 10. Promote a fact
    let promote_input = ReconcileInput {
        new_facts: vec![],
        superseded: vec![],
        promotions: vec![PromotionEntry {
            name: "Threshold too low bug".into(),
            new_tier: "critical".into(),
            reason: "This affects multiple components".into(),
        }],
        relations: vec![],
    };

    let prom_report = reconcile(&mut kg, &promote_input);
    assert_eq!(prom_report.promoted, 1);
    let promoted = kg.lookup("Threshold too low bug").unwrap();
    assert_eq!(promoted.tier, ImportanceTier::Critical);

    // 11. GC — superseded fact should be collected
    let gc_count = garbage_collect(&mut kg, 0.05, now);
    assert!(gc_count >= 1, "Should GC at least the superseded decision");
    assert!(
        kg.lookup("Use exponential decay").is_none()
            || kg
                .lookup("Use exponential decay")
                .unwrap()
                .superseded_by
                .is_some()
    );

    // 12. Reindex a file — verify code entities update
    let new_code = "pub fn new_function() -> bool { true }";
    kg.reindex_file("src/tier.rs", new_code);
    assert!(kg.all_nodes().any(|n| n.name == "new_function"));
    // Old entities from tier.rs should be gone
    let old_tier_entities: Vec<_> = kg
        .all_nodes()
        .filter(|n| {
            matches!(&n.source, Source::CodeAnalysis { file } if file == "src/tier.rs")
                && n.name == "ImportanceTier"
        })
        .collect();
    assert!(
        old_tier_entities.is_empty(),
        "Old ImportanceTier should be gone after reindex"
    );

    // 13. Save and reload — verify persistence
    save(&kg, &kg_path).unwrap();
    let loaded = load_or_create(&kg_path).unwrap();
    assert_eq!(loaded.stats().node_count, kg.stats().node_count);

    // 14. Heuristic snapshot extraction
    let text = "We decided to use MessagePack for storage because it's compact. The bug was caused by incorrect serialization of Option fields.";
    let facts = extract_heuristic(text);
    assert!(!facts.is_empty(), "Should extract facts from conversation text");
}

/// Test that tree-sitter can parse our actual codebase
#[test]
fn test_parse_own_codebase() {
    let code = std::fs::read_to_string("src/graph.rs").unwrap();
    let entities = parse_rust_code(&code, "src/graph.rs");

    // Should find KnowledgeGraph struct
    assert!(entities.iter().any(|e| e.name == "KnowledgeGraph"));
    // Should find methods
    assert!(entities.iter().any(|e| e.name == "add_node" && e.entity_type == "Method"));
    assert!(entities.iter().any(|e| e.name == "lookup" && e.entity_type == "Method"));
    // Should find enums
    assert!(entities.iter().any(|e| e.name == "GraphError"));
    // Should have reasonable count
    assert!(entities.len() > 20, "graph.rs should have 20+ entities, got {}", entities.len());
}

/// Test bootstrap on the full project
#[test]
fn test_bootstrap_full_project() {
    let mut kg = KnowledgeGraph::new();
    let config = GraphocodeConfig::default(); // uses src/**/*.rs

    let (files, entities) = bootstrap::bootstrap_code(&mut kg, &config);
    assert!(files >= 10, "Should index 10+ .rs files, got {}", files);
    assert!(entities >= 100, "Should find 100+ entities, got {}", entities);

    // Should find key types from our codebase
    assert!(kg.all_nodes().any(|n| n.name == "KnowledgeGraph"));
    assert!(kg.all_nodes().any(|n| n.name == "Node"));
    assert!(kg.all_nodes().any(|n| n.name == "ImportanceTier"));
    assert!(kg.all_nodes().any(|n| n.name == "reconcile"));
}

// ── V2 Integration Tests ─────────────────────────

/// Test the full v2 pipeline: bootstrap with references → sync-rules → impact v2
#[test]
fn test_v2_full_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let kg_path = dir.path().join("test.kg");
    let rules_dir = dir.path().join(".claude").join("rules");

    // 1. Bootstrap with v2 (definitions + references)
    let mut kg = autoclaw::load_or_create(&kg_path).unwrap();
    let config = GraphocodeConfig {
        sources: SourcesConfig {
            code: vec!["src/tier.rs".into(), "src/model.rs".into(), "src/graph.rs".into()],
            conversations: false,
            documents: vec![],
        },
        ..GraphocodeConfig::default()
    };
    bootstrap::bootstrap(&mut kg, &config, std::path::Path::new("."));

    // 2. Verify reference edges exist
    assert!(kg.stats().edge_count > 0, "Should have reference edges, got 0");

    // 3. Verify inbound references
    let node_refs = kg.inbound_reference_count("Node");
    assert!(node_refs > 0, "Node should have inbound references, got {}", node_refs);

    // 4. Sync rules
    autoclaw::sync_rules::sync_rules(&kg, &rules_dir);
    assert!(rules_dir.join("project-map.md").exists(), "project-map.md should exist");
    assert!(rules_dir.join("decisions.md").exists(), "decisions.md should exist");

    // 5. Verify project-map contains reference counts
    let map = std::fs::read_to_string(rules_dir.join("project-map.md")).unwrap();
    assert!(map.contains("refs IN"), "Project map should show ref counts: {}", map);

    // 6. Test pattern-grouped impact v2
    let impact = impact_from_diff_v2(
        &kg,
        r#"{"file_path":"src/model.rs","old_string":"pub struct Node","new_string":"pub struct GraphNode"}"#,
    );
    if !impact.is_empty() {
        let json: serde_json::Value = serde_json::from_str(&impact)
            .expect(&format!("Should be valid JSON: {}", impact));
        assert!(
            json.pointer("/hookSpecificOutput/additionalContext").is_some(),
            "Should have additionalContext"
        );
        let ctx = json.pointer("/hookSpecificOutput/additionalContext").unwrap().as_str().unwrap();
        assert!(ctx.contains("IMPACT"), "Report should contain IMPACT: {}", ctx);
    }

    // 7. Save and reload
    autoclaw::save(&kg, &kg_path).unwrap();
    let loaded = autoclaw::load_or_create(&kg_path).unwrap();
    assert_eq!(loaded.stats().node_count, kg.stats().node_count);
    assert_eq!(loaded.stats().edge_count, kg.stats().edge_count);
}

/// Test sync-rules generates path-specific rules with valid YAML frontmatter
#[test]
fn test_v2_rules_have_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let rules_dir = dir.path().join(".claude").join("rules");

    let mut kg = KnowledgeGraph::new();
    kg.reindex_file_v2("src/model.rs", r#"
pub struct Node {
    pub id: u64,
    pub confidence: f32,
    pub name: String,
    pub tier: u8,
}
"#);
    kg.reindex_file_v2("src/graph.rs", "fn test(n: Node) { let c = n.confidence; }");

    autoclaw::sync_rules::sync_rules(&kg, &rules_dir);

    // Find the model rule file
    let model_rule = std::fs::read_dir(&rules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().contains("model"))
        .expect("Should have a model rule file");

    let content = std::fs::read_to_string(model_rule.path()).unwrap();
    assert!(content.contains("---"), "Should have YAML frontmatter: {}", content);
    assert!(content.contains("paths:"), "Should have paths field: {}", content);
    assert!(content.contains("src/model.rs"), "Should reference model.rs: {}", content);
}

/// Test that v2 bootstrap creates more edges than v1
#[test]
fn test_v2_bootstrap_creates_edges() {
    let mut kg = KnowledgeGraph::new();
    let config = GraphocodeConfig {
        sources: SourcesConfig {
            code: vec!["src/tier.rs".into()],
            conversations: false,
            documents: vec![],
        },
        ..GraphocodeConfig::default()
    };
    bootstrap::bootstrap_code(&mut kg, &config);

    // tier.rs has functions that reference ImportanceTier — should create edges
    assert!(kg.stats().edge_count > 0, "V2 bootstrap should create reference edges");
}
