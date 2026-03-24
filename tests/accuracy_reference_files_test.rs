use autoclaw::impact::reference_files_for_entity;
use autoclaw::{Edge, KnowledgeGraph, Node, Source};
use std::collections::BTreeSet;

#[test]
fn accuracy_reference_files_collects_normalized_deduplicated_sources() {
    let mut kg = KnowledgeGraph::new();

    let file_a = kg
        .add_node(Node::new(
            0,
            "src/routes/auth.py".into(),
            "File".into(),
            "".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/routes/auth.py".into(),
            },
        ))
        .unwrap();
    let file_b = kg
        .add_node(Node::new(
            0,
            "src/tests/test_auth.py".into(),
            "File".into(),
            "".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/tests/test_auth.py".into(),
            },
        ))
        .unwrap();
    let target = kg
        .add_node(Node::new(
            0,
            "User.password_hash".into(),
            "Field".into(),
            "".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/models.py".into(),
            },
        ))
        .unwrap();

    kg.add_edge(Edge::new(
        0,
        file_a,
        target,
        "reads".into(),
        1.0,
        Source::CodeAnalysis {
            file: "./src/routes/auth.py".into(),
        },
    ))
    .unwrap();
    kg.add_edge(Edge::new(
        0,
        file_b,
        target,
        "writes".into(),
        1.0,
        Source::CodeAnalysis {
            file: "src/tests/test_auth.py".into(),
        },
    ))
    .unwrap();
    kg.add_edge(Edge::new(
        0,
        file_b,
        target,
        "reads".into(),
        1.0,
        Source::CodeAnalysis {
            file: "./src/tests/test_auth.py".into(),
        },
    ))
    .unwrap();

    let files = reference_files_for_entity(&kg, "password_hash");

    assert_eq!(
        files,
        BTreeSet::from([
            "src/routes/auth.py".to_string(),
            "src/tests/test_auth.py".to_string(),
        ])
    );
}
