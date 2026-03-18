use tree_sitter::Parser;

#[derive(Debug, Clone)]
pub struct CodeEntity {
    pub name: String,
    pub entity_type: String, // Function, Struct, Field, Import, Method, Enum, Trait, Const
    pub definition: String,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RefType {
    Calls,
    ReadsField,
    WritesField,
    UsesType,
    MethodCall,
}

#[derive(Debug, Clone)]
pub struct CodeReference {
    pub source_file: String,
    pub source_line: usize,
    pub target_name: String,
    pub ref_type: RefType,
}

/// Parse Rust source code and extract code entities.
/// Deterministic, no LLM, milliseconds.
pub fn parse_rust_code(source: &str, file_path: &str) -> Vec<CodeEntity> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Error loading Rust grammar");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let root = tree.root_node();
    let bytes = source.as_bytes();
    let mut entities = Vec::new();
    extract_from_node(&root, bytes, file_path, &mut entities, None);
    entities
}

/// Parse Rust source code and extract both entities AND references.
/// V2: returns (definitions, references) for complete code graph.
pub fn parse_rust_code_v2(source: &str, file_path: &str) -> (Vec<CodeEntity>, Vec<CodeReference>) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Error loading Rust grammar");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };

    let root = tree.root_node();
    let bytes = source.as_bytes();
    let mut entities = Vec::new();
    extract_from_node(&root, bytes, file_path, &mut entities, None);
    let mut references = Vec::new();
    extract_references(&root, bytes, file_path, &mut references);
    (entities, references)
}

const RUST_PRIMITIVES: &[&str] = &[
    "Self", "str", "bool", "u8", "u16", "u32", "u64", "u128",
    "i8", "i16", "i32", "i64", "i128", "f32", "f64", "usize", "isize",
    "String", "Vec", "Option", "Result", "HashMap", "HashSet",
    "Box", "Arc", "Rc", "Path", "PathBuf", "Cow", "BTreeMap", "BTreeSet",
];

fn extract_references(
    node: &tree_sitter::Node,
    source: &[u8],
    file: &str,
    refs: &mut Vec<CodeReference>,
) {
    match node.kind() {
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                match func_node.kind() {
                    // Method call: obj.method(args)
                    "field_expression" => {
                        if let Some(method_node) = func_node.child_by_field_name("field") {
                            refs.push(CodeReference {
                                source_file: file.into(),
                                source_line: node.start_position().row + 1,
                                target_name: node_text(&method_node, source).to_string(),
                                ref_type: RefType::MethodCall,
                            });
                        }
                    }
                    // Simple function call: func(args)
                    "identifier" => {
                        let name = node_text(&func_node, source).to_string();
                        // Skip common macros and builtins
                        if !["println", "eprintln", "format", "vec", "panic",
                             "assert", "assert_eq", "assert_ne", "unreachable",
                             "todo", "unimplemented", "dbg", "write", "writeln",
                             "Some", "None", "Ok", "Err"].contains(&name.as_str()) {
                            refs.push(CodeReference {
                                source_file: file.into(),
                                source_line: node.start_position().row + 1,
                                target_name: name,
                                ref_type: RefType::Calls,
                            });
                        }
                    }
                    // Qualified call: Module::func(args)
                    "scoped_identifier" => {
                        // Extract the last segment as the function name
                        let text = node_text(&func_node, source);
                        if let Some(name) = text.rsplit("::").next() {
                            refs.push(CodeReference {
                                source_file: file.into(),
                                source_line: node.start_position().row + 1,
                                target_name: name.to_string(),
                                ref_type: RefType::Calls,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        "field_expression" => {
            // Skip if parent is a call_expression (handled above as MethodCall)
            let is_method_call = node
                .parent()
                .map(|p| p.kind() == "call_expression" &&
                    p.child_by_field_name("function")
                        .map(|f| f.id() == node.id())
                        .unwrap_or(false))
                .unwrap_or(false);

            if !is_method_call {
                if let Some(field_node) = node.child_by_field_name("field") {
                    let field_name = node_text(&field_node, source).to_string();
                    // Determine if read or write
                    let is_write = node
                        .parent()
                        .map(|p| {
                            p.kind() == "assignment_expression"
                                && p.child_by_field_name("left")
                                    .map(|l| l.id() == node.id())
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    refs.push(CodeReference {
                        source_file: file.into(),
                        source_line: node.start_position().row + 1,
                        target_name: field_name,
                        ref_type: if is_write {
                            RefType::WritesField
                        } else {
                            RefType::ReadsField
                        },
                    });
                }
            }
        }
        "type_identifier" => {
            let name = node_text(node, source).to_string();
            if !RUST_PRIMITIVES.contains(&name.as_str()) {
                refs.push(CodeReference {
                    source_file: file.into(),
                    source_line: node.start_position().row + 1,
                    target_name: name,
                    ref_type: RefType::UsesType,
                });
            }
        }
        _ => {}
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_references(&child, source, file, refs);
    }
}

fn node_text<'a>(node: &tree_sitter::Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or("")
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

fn extract_from_node(
    node: &tree_sitter::Node,
    source: &[u8],
    file: &str,
    entities: &mut Vec<CodeEntity>,
    impl_type: Option<&str>,
) {
    match node.kind() {
        "function_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let full_text = node_text(node, source);
                let sig = first_line(full_text).to_string();

                let etype = if impl_type.is_some() {
                    "Method"
                } else {
                    "Function"
                };

                let def = if let Some(t) = impl_type {
                    format!("{}::{} — {}", t, name, sig.trim())
                } else {
                    sig.trim().to_string()
                };

                entities.push(CodeEntity {
                    name,
                    entity_type: etype.into(),
                    definition: def,
                    file: file.into(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });
            }
        }
        "struct_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                entities.push(CodeEntity {
                    name: name.clone(),
                    entity_type: "Struct".into(),
                    definition: format!("struct {}", name),
                    file: file.into(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });

                // Extract fields from body
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    for child in body.children(&mut cursor) {
                        if child.kind() == "field_declaration" {
                            if let Some(field_name) = child.child_by_field_name("name") {
                                let fname = node_text(&field_name, source).to_string();
                                let fdef = node_text(&child, source).trim().to_string();
                                entities.push(CodeEntity {
                                    name: format!("{}.{}", name, fname),
                                    entity_type: "Field".into(),
                                    definition: fdef,
                                    file: file.into(),
                                    line_start: child.start_position().row + 1,
                                    line_end: child.end_position().row + 1,
                                });
                            }
                        }
                    }
                }
            }
        }
        "enum_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                entities.push(CodeEntity {
                    name,
                    entity_type: "Enum".into(),
                    definition: first_line(node_text(node, source)).trim().to_string(),
                    file: file.into(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });
            }
        }
        "trait_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                entities.push(CodeEntity {
                    name,
                    entity_type: "Trait".into(),
                    definition: first_line(node_text(node, source)).trim().to_string(),
                    file: file.into(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });
            }
        }
        "use_declaration" => {
            let text = node_text(node, source).trim().to_string();
            entities.push(CodeEntity {
                name: text.clone(),
                entity_type: "Import".into(),
                definition: text,
                file: file.into(),
                line_start: node.start_position().row + 1,
                line_end: node.end_position().row + 1,
            });
        }
        "const_item" | "static_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                entities.push(CodeEntity {
                    name,
                    entity_type: "Const".into(),
                    definition: first_line(node_text(node, source)).trim().to_string(),
                    file: file.into(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });
            }
        }
        "impl_item" => {
            // Get the type being implemented
            let type_name = node
                .child_by_field_name("type")
                .map(|n| node_text(&n, source).to_string());

            // Recurse into impl body with type context
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    extract_from_node(
                        &child,
                        source,
                        file,
                        entities,
                        type_name.as_deref(),
                    );
                }
            }
            return; // Don't recurse normally — we handled children above
        }
        _ => {}
    }

    // Recurse into children (except impl_item which is handled above)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_from_node(&child, source, file, entities, impl_type);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_functions() {
        let code = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}

fn private_helper() -> bool {
    true
}
"#;
        let entities = parse_rust_code(code, "src/test.rs");
        let fns: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == "Function")
            .collect();
        assert_eq!(fns.len(), 2);
        assert!(fns.iter().any(|f| f.name == "hello"));
        assert!(fns.iter().any(|f| f.name == "private_helper"));
    }

    #[test]
    fn test_parse_structs_and_fields() {
        let code = r#"
pub struct Node {
    pub id: u64,
    pub name: String,
    confidence: f32,
}
"#;
        let entities = parse_rust_code(code, "src/model.rs");
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Struct" && e.name == "Node"));
        let fields: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == "Field")
            .collect();
        assert_eq!(fields.len(), 3);
        assert!(fields.iter().any(|f| f.name == "Node.id"));
        assert!(fields.iter().any(|f| f.name == "Node.name"));
        assert!(fields.iter().any(|f| f.name == "Node.confidence"));
    }

    #[test]
    fn test_parse_use_statements() {
        let code = r#"
use crate::model::Node;
use std::collections::HashMap;
"#;
        let entities = parse_rust_code(code, "src/graph.rs");
        let imports: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == "Import")
            .collect();
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn test_parse_impl_methods() {
        let code = r#"
impl Node {
    pub fn new(id: u64) -> Self {
        Node { id }
    }

    fn helper(&self) -> bool {
        true
    }
}
"#;
        let entities = parse_rust_code(code, "src/model.rs");
        let methods: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == "Method")
            .collect();
        assert_eq!(methods.len(), 2);
        assert!(methods.iter().any(|m| m.name == "new"));
        assert!(methods.iter().any(|m| m.name == "helper"));
        // Methods should reference the impl type
        assert!(methods[0].definition.contains("Node::"));
    }

    #[test]
    fn test_parse_enum() {
        let code = r#"
pub enum Source {
    Document,
    Memory,
    Inferred,
}
"#;
        let entities = parse_rust_code(code, "src/model.rs");
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Enum" && e.name == "Source"));
    }

    #[test]
    fn test_parse_trait() {
        let code = r#"
pub trait Serializable {
    fn serialize(&self) -> Vec<u8>;
}
"#;
        let entities = parse_rust_code(code, "src/traits.rs");
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Trait" && e.name == "Serializable"));
    }

    #[test]
    fn test_parse_const() {
        let code = r#"
pub const MAX_SIZE: usize = 4096;
static COUNTER: AtomicUsize = AtomicUsize::new(0);
"#;
        let entities = parse_rust_code(code, "src/config.rs");
        let consts: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == "Const")
            .collect();
        assert_eq!(consts.len(), 2);
    }

    #[test]
    fn test_line_numbers() {
        let code = "fn first() {}\n\nfn second() {}";
        let entities = parse_rust_code(code, "test.rs");
        let first = entities.iter().find(|e| e.name == "first").unwrap();
        let second = entities.iter().find(|e| e.name == "second").unwrap();
        assert_eq!(first.line_start, 1);
        assert_eq!(second.line_start, 3);
    }

    #[test]
    fn test_empty_source() {
        let entities = parse_rust_code("", "empty.rs");
        assert!(entities.is_empty());
    }

    #[test]
    fn test_file_path_preserved() {
        let code = "fn test() {}";
        let entities = parse_rust_code(code, "src/my_module.rs");
        assert_eq!(entities[0].file, "src/my_module.rs");
    }

    #[test]
    fn test_real_world_code() {
        // A more realistic snippet similar to our codebase
        let code = r#"
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub nodes: HashMap<u64, Node>,
    pub edges: Vec<Edge>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: Node) -> Result<u64, String> {
        Ok(0)
    }
}

pub enum GraphError {
    NotFound(String),
    Invalid(String),
}
"#;
        let entities = parse_rust_code(code, "src/graph.rs");

        // Should find: 2 imports, 1 struct, 2 fields, 2 methods, 1 enum
        assert!(entities.iter().any(|e| e.entity_type == "Import"));
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Struct" && e.name == "KnowledgeGraph"));
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Field" && e.name == "KnowledgeGraph.nodes"));
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Method" && e.name == "new"));
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Method" && e.name == "add_node"));
        assert!(entities
            .iter()
            .any(|e| e.entity_type == "Enum" && e.name == "GraphError"));
    }

    // ── v2 reference extraction tests ─────────────────────────

    #[test]
    fn test_extract_call_references() {
        let code = r#"
fn caller() {
    let result = chunk_text("hello", 4000, 500);
    let x = other_func();
}
"#;
        let (_, refs) = parse_rust_code_v2(code, "src/test.rs");
        let calls: Vec<_> = refs.iter().filter(|r| r.ref_type == RefType::Calls).collect();
        assert!(calls.iter().any(|r| r.target_name == "chunk_text"), "Should find chunk_text call");
        assert!(calls.iter().any(|r| r.target_name == "other_func"), "Should find other_func call");
    }

    #[test]
    fn test_extract_field_read_references() {
        let code = r#"
fn reader(node: Node) {
    let c = node.confidence;
    let t = node.tier;
}
"#;
        let (_, refs) = parse_rust_code_v2(code, "src/test.rs");
        let reads: Vec<_> = refs.iter().filter(|r| r.ref_type == RefType::ReadsField).collect();
        assert!(reads.iter().any(|r| r.target_name == "confidence"), "Should find confidence read");
        assert!(reads.iter().any(|r| r.target_name == "tier"), "Should find tier read");
    }

    #[test]
    fn test_extract_field_write_references() {
        let code = r#"
fn writer(node: &mut Node) {
    node.confidence = 0.9;
}
"#;
        let (_, refs) = parse_rust_code_v2(code, "src/test.rs");
        let writes: Vec<_> = refs.iter().filter(|r| r.ref_type == RefType::WritesField).collect();
        assert!(writes.iter().any(|r| r.target_name == "confidence"), "Should find confidence write");
    }

    #[test]
    fn test_extract_type_references() {
        let code = r#"
fn processor(nodes: Vec<Node>, edges: &[Edge]) -> Option<NodeId> {
    None
}
"#;
        let (_, refs) = parse_rust_code_v2(code, "src/test.rs");
        let types: Vec<_> = refs.iter().filter(|r| r.ref_type == RefType::UsesType).collect();
        assert!(types.iter().any(|r| r.target_name == "Node"), "Should find Node type usage");
        assert!(types.iter().any(|r| r.target_name == "Edge"), "Should find Edge type usage");
        assert!(types.iter().any(|r| r.target_name == "NodeId"), "Should find NodeId type usage");
    }

    #[test]
    fn test_extract_method_call_references() {
        let code = r#"
fn user(kg: &mut KnowledgeGraph) {
    kg.add_node(node);
    let n = kg.lookup("test");
}
"#;
        let (_, refs) = parse_rust_code_v2(code, "src/test.rs");
        let methods: Vec<_> = refs.iter().filter(|r| r.ref_type == RefType::MethodCall).collect();
        assert!(methods.iter().any(|r| r.target_name == "add_node"), "Should find add_node method call");
        assert!(methods.iter().any(|r| r.target_name == "lookup"), "Should find lookup method call");
    }

    #[test]
    fn test_v2_returns_both_entities_and_refs() {
        let code = r#"
pub fn hello() -> bool { true }
fn caller() { hello(); }
"#;
        let (entities, refs) = parse_rust_code_v2(code, "src/test.rs");
        assert!(!entities.is_empty(), "Should return entities");
        assert!(!refs.is_empty(), "Should return references");
        assert!(entities.iter().any(|e| e.name == "hello"));
        assert!(refs.iter().any(|r| r.target_name == "hello" && r.ref_type == RefType::Calls));
    }

    #[test]
    fn test_filters_rust_primitives() {
        let code = r#"
fn test(s: String, v: Vec<u64>, h: HashMap<String, bool>) {}
"#;
        let (_, refs) = parse_rust_code_v2(code, "src/test.rs");
        let types: Vec<_> = refs.iter().filter(|r| r.ref_type == RefType::UsesType).collect();
        // String, Vec, HashMap, u64, bool should all be filtered
        assert!(types.is_empty(), "Primitives should be filtered, got: {:?}", types);
    }

    #[test]
    fn test_scoped_call() {
        let code = r#"
fn test() {
    let x = Module::function();
}
"#;
        let (_, refs) = parse_rust_code_v2(code, "src/test.rs");
        assert!(refs.iter().any(|r| r.target_name == "function" && r.ref_type == RefType::Calls));
    }

    #[test]
    fn test_reference_line_numbers() {
        let code = "fn a() {}\nfn b() { a(); }";
        let (_, refs) = parse_rust_code_v2(code, "test.rs");
        let call = refs.iter().find(|r| r.target_name == "a").unwrap();
        assert_eq!(call.source_line, 2);
    }

    #[test]
    fn test_real_codebase_references() {
        let code = std::fs::read_to_string("src/graph.rs").unwrap();
        let (entities, refs) = parse_rust_code_v2(&code, "src/graph.rs");
        assert!(!entities.is_empty());
        assert!(!refs.is_empty(), "graph.rs should have references");
        // graph.rs uses Node, Edge, etc.
        assert!(refs.iter().any(|r| r.ref_type == RefType::UsesType), "Should find type usages");
    }
}
