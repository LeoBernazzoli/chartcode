use autoclaw::graph::KnowledgeGraph;
use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn plugin_assets_use_chartcode_commands() {
    let cases = [
        (
            "autoclaw-plugin/skills/chartcode-query/SKILL.md",
            &[
                "chartcode explore",
                "chartcode relevant",
                "chartcode file-context",
                "chartcode connect",
            ][..],
            &[
                "autoclaw explore",
                "autoclaw relevant",
                "autoclaw file-context",
                "autoclaw connect",
            ][..],
        ),
        (
            "autoclaw-plugin/skills/chartcode-impact/SKILL.md",
            &["chartcode impact"][..],
            &["autoclaw impact"][..],
        ),
        (
            "autoclaw-plugin/skills/chartcode-decide/SKILL.md",
            &["chartcode reconcile"][..],
            &["autoclaw reconcile"][..],
        ),
        (
            "autoclaw-plugin/scripts/extract-and-compact.sh",
            &["chartcode snapshot", "chartcode reindex"][..],
            &["autoclaw snapshot", "autoclaw reindex"][..],
        ),
        (
            "autoclaw-plugin/CLAUDE.md",
            &["# Chartcode"][..],
            &["# Graphocode"][..],
        ),
    ];

    for (path, expected, forbidden) in cases {
        let content = fs::read_to_string(repo_path(path)).unwrap_or_else(|err| {
            panic!("failed reading {path}: {err}");
        });
        for marker in expected {
            assert!(content.contains(marker), "{path} missing `{marker}`");
        }
        for marker in forbidden {
            assert!(!content.contains(marker), "{path} still contains `{marker}`");
        }
    }
}

#[test]
fn generated_rules_reference_chartcode() {
    let mut kg = KnowledgeGraph::new();
    kg.reindex_file_v2(
        "src/model.rs",
        "pub struct Node { pub id: u64, pub name: String, pub tier: u8, pub confidence: f32 }",
    );
    kg.reindex_file_v2("src/graph.rs", "fn r(n: &Node) { let _ = n.id; }");

    let dir = tempfile::tempdir().unwrap();
    let rules_dir = dir.path().join(".claude").join("rules");
    autoclaw::sync_rules::sync_rules(&kg, &rules_dir);

    let project_map = fs::read_to_string(rules_dir.join("project-map.md")).unwrap();
    let decisions = fs::read_to_string(rules_dir.join("decisions.md")).unwrap();
    let file_rule = fs::read_to_string(rules_dir.join("src-model-rs.md")).unwrap();

    for generated in [&project_map, &decisions, &file_rule] {
        assert!(generated.contains("chartcode"), "missing `chartcode` in:\n{generated}");
        assert!(
            !generated.contains("autoclaw"),
            "found legacy `autoclaw` in:\n{generated}"
        );
    }
}

#[test]
fn cli_user_facing_strings_use_chartcode_branding() {
    let main_rs = fs::read_to_string(repo_path("src/main.rs")).unwrap();

    for marker in [
        "Chartcode: initializing project...",
        "Chartcode ready:",
        "\"chartcode.toml\"",
        "Run /chartcode:start to complete extraction with LLM.",
    ] {
        assert!(main_rs.contains(marker), "src/main.rs missing `{marker}`");
    }

    for legacy in [
        "Graphocode: initializing project...",
        "Graphocode ready:",
        "\"graphocode.toml\"",
        "Run /graphocode:start to complete extraction with LLM.",
    ] {
        assert!(
            !main_rs.contains(legacy),
            "src/main.rs still contains legacy `{legacy}`"
        );
    }
}
