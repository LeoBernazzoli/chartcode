use tree_sitter::{Language, Parser};

/// Detect language from file extension and parse accordingly.
/// Returns (entities, references). Works for Rust, Python, TypeScript, JavaScript, Go, Java, C#.
pub fn parse_file(source: &str, file_path: &str) -> (Vec<CodeEntity>, Vec<CodeReference>) {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let lang_config = match ext {
        "rs" => Some(LangConfig {
            language: tree_sitter_rust::LANGUAGE.into(),
            style: LangStyle::Rust,
        }),
        "py" => Some(LangConfig {
            language: tree_sitter_python::LANGUAGE.into(),
            style: LangStyle::Python,
        }),
        "ts" | "tsx" => Some(LangConfig {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            style: LangStyle::TypeScript,
        }),
        "js" | "jsx" => Some(LangConfig {
            language: tree_sitter_javascript::LANGUAGE.into(),
            style: LangStyle::JavaScript,
        }),
        "go" => Some(LangConfig {
            language: tree_sitter_go::LANGUAGE.into(),
            style: LangStyle::Go,
        }),
        "java" => Some(LangConfig {
            language: tree_sitter_java::LANGUAGE.into(),
            style: LangStyle::Java,
        }),
        "cs" => Some(LangConfig {
            language: tree_sitter_c_sharp::LANGUAGE.into(),
            style: LangStyle::CSharp,
        }),
        _ => None,
    };

    match lang_config {
        Some(config) => parse_with_config(source, file_path, &config),
        None => (Vec::new(), Vec::new()),
    }
}

struct LangConfig {
    language: Language,
    style: LangStyle,
}

#[derive(Clone, Copy)]
enum LangStyle {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
    Java,
    CSharp,
}

/// Generic parser that uses tree-sitter with a language config.
fn parse_with_config(
    source: &str,
    file_path: &str,
    config: &LangConfig,
) -> (Vec<CodeEntity>, Vec<CodeReference>) {
    let mut parser = Parser::new();
    parser
        .set_language(&config.language)
        .expect("Error loading grammar");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return (Vec::new(), Vec::new()),
    };

    let root = tree.root_node();
    let bytes = source.as_bytes();
    let mut entities = Vec::new();
    let mut references = Vec::new();

    extract_generic(&root, bytes, file_path, &mut entities, &mut references, config.style, None);

    (entities, references)
}

/// Universal AST walker — extracts entities and references for any supported language.
fn extract_generic(
    node: &tree_sitter::Node,
    source: &[u8],
    file: &str,
    entities: &mut Vec<CodeEntity>,
    refs: &mut Vec<CodeReference>,
    style: LangStyle,
    parent_type: Option<&str>,
) {
    let kind = node.kind();

    // ── ENTITY EXTRACTION ──
    match style {
        LangStyle::Rust => extract_entity_rust(node, source, file, entities, parent_type),
        _ => extract_entity_universal(node, source, file, entities, style, parent_type),
    }

    // ── REFERENCE EXTRACTION (universal across languages) ──
    extract_ref_universal(node, source, file, refs, style);

    // ── RECURSE ──
    // For Rust impl blocks, pass the type name as parent context
    let new_parent = if matches!(style, LangStyle::Rust) && kind == "impl_item" {
        let type_name = node
            .child_by_field_name("type")
            .map(|n| node_text(&n, source).to_string());
        // Recurse into impl body with type context
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                extract_generic(
                    &child,
                    source,
                    file,
                    entities,
                    refs,
                    style,
                    type_name.as_deref(),
                );
            }
        }
        return; // Don't recurse normally for impl_item
    } else if is_class_like(kind, style) {
        Some(get_name(node, source, style))
    } else {
        parent_type.map(|s| s.to_string())
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_generic(
            &child,
            source,
            file,
            entities,
            refs,
            style,
            new_parent.as_deref(),
        );
    }
}

fn is_class_like(kind: &str, style: LangStyle) -> bool {
    match style {
        LangStyle::Python => kind == "class_definition",
        LangStyle::TypeScript | LangStyle::JavaScript => {
            kind == "class_declaration" || kind == "interface_declaration" || kind == "enum_declaration"
        }
        LangStyle::Java | LangStyle::CSharp => {
            kind == "class_declaration" || kind == "interface_declaration" || kind == "enum_declaration"
        }
        LangStyle::Go => kind == "type_declaration",
        LangStyle::Rust => false,
    }
}

fn get_name(node: &tree_sitter::Node, source: &[u8], _style: LangStyle) -> String {
    node.child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string())
        .unwrap_or_default()
}

/// Extract entities for Python, TS, JS, Go, Java, C#
fn extract_entity_universal(
    node: &tree_sitter::Node,
    source: &[u8],
    file: &str,
    entities: &mut Vec<CodeEntity>,
    style: LangStyle,
    parent_type: Option<&str>,
) {
    let kind = node.kind();

    let (entity_type, should_extract) = match style {
        LangStyle::Python => match kind {
            "function_definition" => {
                if parent_type.is_some() {
                    ("Method", true)
                } else {
                    ("Function", true)
                }
            }
            "class_definition" => ("Struct", true),
            "import_statement" | "import_from_statement" => ("Import", true),
            // Class/module-level assignments: MAX_RETRIES = 3, API_KEY = "secret"
            "expression_statement" => {
                // Only extract if direct child of class_body or module (not inside a function)
                let parent_kind = node.parent().map(|p| p.kind()).unwrap_or("");
                let is_class_or_module_level = parent_kind == "block" && {
                    // Check grandparent: should be class_definition or module
                    node.parent()
                        .and_then(|p| p.parent())
                        .map(|gp| gp.kind() == "class_definition" || gp.kind() == "module")
                        .unwrap_or(false)
                } || parent_kind == "module";

                if !is_class_or_module_level {
                    ("", false) // Skip assignments inside methods
                } else {
                    let first_child = node.child(0);
                    if let Some(child) = first_child {
                        if child.kind() == "assignment" {
                            // Skip self.x = ... assignments
                            let left = child.child_by_field_name("left")
                                .map(|n| node_text(&n, source))
                                .unwrap_or("");
                            if left.starts_with("self.") {
                                ("", false)
                            } else if parent_type.is_some() {
                                ("Field", true)
                            } else {
                                ("Const", true)
                            }
                        } else if child.kind() == "type" {
                            ("Field", true)
                        } else {
                            ("", false)
                        }
                    } else {
                        ("", false)
                    }
                }
            }
            // Type-annotated fields: name: str, email: str = "default"
            "typed_parameter" | "typed_default_parameter" => {
                if parent_type.is_some() {
                    ("Field", true)
                } else {
                    ("", false)
                }
            }
            _ => ("", false),
        },
        LangStyle::TypeScript | LangStyle::JavaScript => match kind {
            "function_declaration" => ("Function", true),
            "method_definition" => ("Method", true),
            "class_declaration" => ("Struct", true),
            "import_statement" => ("Import", true),
            "interface_declaration" => ("Trait", true),
            "enum_declaration" => ("Enum", true),
            "type_alias_declaration" => ("Struct", true),
            // Class fields: private db: Database, public timeout: number
            "public_field_definition" | "field_definition" => ("Field", true),
            // Enum members: Active = "active"
            // (handled via parent enum, but we can extract individual members)
            "lexical_declaration" => {
                // const at top level or class level
                let text = node_text(node, source);
                if text.starts_with("const") {
                    ("Const", true)
                } else {
                    ("", false)
                }
            }
            // Interface properties: id: number, name: string
            "property_signature" => {
                if parent_type.is_some() {
                    ("Field", true)
                } else {
                    ("", false)
                }
            }
            // Enum members: Active = "active"
            "enum_assignment" => {
                if parent_type.is_some() {
                    ("Field", true) // enum variant as field
                } else {
                    ("", false)
                }
            }
            "export_statement" => {
                // Check if it exports a function/class declaration
                // Skip — the child declaration will be caught
                ("", false)
            }
            _ => ("", false),
        },
        LangStyle::Go => match kind {
            "function_declaration" => ("Function", true),
            "method_declaration" => ("Method", true),
            "type_declaration" => ("Struct", true),
            "import_declaration" => ("Import", true),
            "const_declaration" => ("Const", true),
            _ => ("", false),
        },
        LangStyle::Java => match kind {
            "method_declaration" => ("Method", true),
            "class_declaration" => ("Struct", true),
            "interface_declaration" => ("Trait", true),
            "enum_declaration" => ("Enum", true),
            "import_declaration" => ("Import", true),
            "field_declaration" => ("Field", true),
            _ => ("", false),
        },
        LangStyle::CSharp => match kind {
            "method_declaration" => ("Method", true),
            "class_declaration" => ("Struct", true),
            "interface_declaration" => ("Trait", true),
            "enum_declaration" => ("Enum", true),
            "using_directive" => ("Import", true),
            "field_declaration" | "property_declaration" => ("Field", true),
            _ => ("", false),
        },
        LangStyle::Rust => ("", false), // Handled by extract_entity_rust
    };

    if !should_extract {
        return;
    }

    let name = if entity_type == "Import" {
        node_text(node, source).trim().to_string()
    } else if kind == "expression_statement" {
        // Python: extract name from assignment target
        // e.g. "MAX_RETRIES = 3" → "MAX_RETRIES", "name: str" → "name"
        let first_child = node.child(0);
        if let Some(child) = first_child {
            if child.kind() == "assignment" {
                // assignment → left is the target
                child.child_by_field_name("left")
                    .map(|n| node_text(&n, source).to_string())
                    .unwrap_or_default()
            } else if child.kind() == "type" {
                // type annotation: name: str
                let mut cursor = child.walk();
                child.children(&mut cursor)
                    .find(|c| c.kind() == "identifier")
                    .map(|c| node_text(&c, source).to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else if kind == "lexical_declaration" {
        // TypeScript/JS: const MAX = 3 → extract from variable_declarator
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|c| c.kind() == "variable_declarator")
            .and_then(|vd| vd.child_by_field_name("name"))
            .map(|n| node_text(&n, source).to_string())
            .unwrap_or_default()
    } else if kind == "property_signature" || kind == "public_field_definition" || kind == "field_definition" || kind == "enum_assignment" {
        // TS interface/class fields: name might be the first child identifier
        node.child_by_field_name("name")
            .map(|n| node_text(&n, source).to_string())
            .or_else(|| {
                // Fallback: look for property_identifier or identifier
                let mut cursor = node.walk();
                node.children(&mut cursor)
                    .find(|c| c.kind() == "property_identifier" || c.kind() == "identifier")
                    .map(|c| node_text(&c, source).to_string())
            })
            .unwrap_or_default()
    } else {
        node.child_by_field_name("name")
            .map(|n| node_text(&n, source).to_string())
            .unwrap_or_else(|| {
                // Fallback: first identifier child
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "identifier" || child.kind() == "type_identifier" {
                        return node_text(&child, source).to_string();
                    }
                }
                String::new()
            })
    };

    if name.is_empty() {
        return;
    }

    // Prefix field names with parent type (like Rust Node.confidence)
    let name = if entity_type == "Field" && parent_type.is_some() {
        format!("{}.{}", parent_type.unwrap(), name)
    } else {
        name
    };

    let sig = first_line(node_text(node, source)).trim().to_string();
    let def = if entity_type != "Field" {
        if let Some(t) = parent_type {
            format!("{}::{} — {}", t, name, sig)
        } else {
            sig
        }
    } else {
        sig
    };

    entities.push(CodeEntity {
        name,
        entity_type: entity_type.into(),
        definition: def,
        file: file.into(),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
    });
}

/// Extract references — mostly universal across languages.
fn extract_ref_universal(
    node: &tree_sitter::Node,
    source: &[u8],
    file: &str,
    refs: &mut Vec<CodeReference>,
    style: LangStyle,
) {
    let kind = node.kind();

    // ── FUNCTION/METHOD CALLS ──
    let is_call = match style {
        LangStyle::Rust => kind == "call_expression",
        LangStyle::Python => kind == "call",
        LangStyle::TypeScript | LangStyle::JavaScript => kind == "call_expression",
        LangStyle::Go => kind == "call_expression",
        LangStyle::Java | LangStyle::CSharp => kind == "method_invocation" || kind == "invocation_expression",
    };

    if is_call {
        // Get the function being called
        let func_node = match style {
            LangStyle::Python => node.child_by_field_name("function"),
            _ => node.child_by_field_name("function"),
        };

        if let Some(func) = func_node {
            match func.kind() {
                // Simple call: func()
                "identifier" => {
                    let name = node_text(&func, source).to_string();
                    if !is_builtin(&name, style) {
                        refs.push(CodeReference {
                            source_file: file.into(),
                            source_line: node.start_position().row + 1,
                            target_name: name,
                            ref_type: RefType::Calls,
                        });
                    }
                }
                // Method call: obj.method() or attribute access call
                "field_expression" | "member_expression" | "attribute" => {
                    if let Some(method) = func.child_by_field_name("field")
                        .or_else(|| func.child_by_field_name("attribute"))
                        .or_else(|| func.child_by_field_name("property"))
                    {
                        refs.push(CodeReference {
                            source_file: file.into(),
                            source_line: node.start_position().row + 1,
                            target_name: node_text(&method, source).to_string(),
                            ref_type: RefType::MethodCall,
                        });
                    }
                }
                // Scoped call: Module::func() or Module.func()
                "scoped_identifier" => {
                    let text = node_text(&func, source);
                    if let Some(name) = text.rsplit(&['.', ':'][..]).next() {
                        if !name.is_empty() {
                            refs.push(CodeReference {
                                source_file: file.into(),
                                source_line: node.start_position().row + 1,
                                target_name: name.to_string(),
                                ref_type: RefType::Calls,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ── FIELD ACCESS ──
    let is_field_access = match style {
        LangStyle::Rust => kind == "field_expression",
        LangStyle::Python => kind == "attribute",
        LangStyle::TypeScript | LangStyle::JavaScript => kind == "member_expression",
        LangStyle::Go => kind == "selector_expression",
        LangStyle::Java | LangStyle::CSharp => kind == "member_access_expression" || kind == "field_access",
    };

    if is_field_access {
        // Skip if this is the function part of a call (handled above)
        let is_call_function = node
            .parent()
            .map(|p| {
                let p_kind = p.kind();
                (p_kind == "call_expression" || p_kind == "call" || p_kind == "method_invocation"
                    || p_kind == "invocation_expression")
                    && p.child_by_field_name("function")
                        .map(|f| f.id() == node.id())
                        .unwrap_or(false)
            })
            .unwrap_or(false);

        if !is_call_function {
            let field = match style {
                LangStyle::Rust => node.child_by_field_name("field"),
                LangStyle::Python => node.child_by_field_name("attribute"),
                LangStyle::Go => node.child_by_field_name("field"),
                _ => node.child_by_field_name("property"),
            };

            if let Some(f) = field {
                let name = node_text(&f, source).to_string();
                let is_write = node
                    .parent()
                    .map(|p| {
                        (p.kind() == "assignment_expression"
                            || p.kind() == "assignment"
                            || p.kind() == "augmented_assignment")
                            && p.child_by_field_name("left")
                                .map(|l| l.id() == node.id())
                                .unwrap_or(false)
                    })
                    .unwrap_or(false);

                refs.push(CodeReference {
                    source_file: file.into(),
                    source_line: node.start_position().row + 1,
                    target_name: name,
                    ref_type: if is_write {
                        RefType::WritesField
                    } else {
                        RefType::ReadsField
                    },
                });
            }
        }
    }

    // ── TYPE REFERENCES (Rust + TS + Java + C# + Go) ──
    if kind == "type_identifier" || kind == "type_annotation" {
        let name = if kind == "type_identifier" {
            node_text(node, source).to_string()
        } else {
            // For type_annotation, find the type_identifier child
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .find(|c| c.kind() == "type_identifier" || c.kind() == "identifier")
                .map(|c| node_text(&c, source).to_string())
                .unwrap_or_default()
        };

        if !name.is_empty() && !is_primitive_type(&name, style) {
            refs.push(CodeReference {
                source_file: file.into(),
                source_line: node.start_position().row + 1,
                target_name: name,
                ref_type: RefType::UsesType,
            });
        }
    }

    // ── KEYWORD ARGUMENTS (Python: password_hash=value → WritesField) ──
    if kind == "keyword_argument" {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = node_text(&name_node, source).to_string();
            if name.len() >= 3 && !is_builtin(&name, style) {
                refs.push(CodeReference {
                    source_file: file.into(),
                    source_line: node.start_position().row + 1,
                    target_name: name,
                    ref_type: RefType::WritesField,
                });
            }
        }
    }

    // ── IMPORTED NAMES as UsesType references ──
    // Python: "from models import User, APIKey" → UsesType for User and APIKey
    // TS/JS: "import { User, Config } from './models'" → UsesType for User and Config
    if kind == "import_from_statement" || kind == "import_statement" {
        // Walk children to find imported identifiers
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // Python: dotted_name or aliased_import inside import_from_statement
            if child.kind() == "dotted_name" || child.kind() == "aliased_import" {
                // For aliased_import, get the name (not the alias)
                let name = if child.kind() == "aliased_import" {
                    child.child_by_field_name("name")
                        .map(|n| node_text(&n, source).to_string())
                        .unwrap_or_default()
                } else {
                    node_text(&child, source).to_string()
                };
                // Only add if it looks like a type/class name (starts with uppercase)
                // to avoid adding module names as references
                if !name.is_empty() && name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    refs.push(CodeReference {
                        source_file: file.into(),
                        source_line: node.start_position().row + 1,
                        target_name: name,
                        ref_type: RefType::UsesType,
                    });
                }
            }
            // TS/JS: import_specifier inside import_clause
            if child.kind() == "import_specifier" {
                let name = child.child_by_field_name("name")
                    .map(|n| node_text(&n, source).to_string())
                    .unwrap_or_else(|| node_text(&child, source).to_string());
                if !name.is_empty() && name.len() >= 2 {
                    refs.push(CodeReference {
                        source_file: file.into(),
                        source_line: node.start_position().row + 1,
                        target_name: name,
                        ref_type: RefType::UsesType,
                    });
                }
            }
        }
    }
    // TS/JS export statements: "export { TRPCError } from '...'" or "export * from '...'"
    if kind == "export_specifier" {
        let name = node.child_by_field_name("name")
            .map(|n| node_text(&n, source).to_string())
            .unwrap_or_else(|| node_text(node, source).to_string());
        if !name.is_empty() && name.len() >= 2 && !is_builtin(&name, style) {
            refs.push(CodeReference {
                source_file: file.into(),
                source_line: node.start_position().row + 1,
                target_name: name,
                ref_type: RefType::UsesType,
            });
        }
    }

    // TS/JS named imports in import_clause
    if kind == "import_specifier" {
        let name = node.child_by_field_name("name")
            .map(|n| node_text(&n, source).to_string())
            .unwrap_or_else(|| node_text(node, source).to_string());
        if !name.is_empty() && name.len() >= 2 && !is_builtin(&name, style) {
            refs.push(CodeReference {
                source_file: file.into(),
                source_line: node.start_position().row + 1,
                target_name: name,
                ref_type: RefType::UsesType,
            });
        }
    }
}

fn is_builtin(name: &str, style: LangStyle) -> bool {
    match style {
        LangStyle::Rust => {
            ["println", "eprintln", "format", "vec", "panic",
             "assert", "assert_eq", "assert_ne", "unreachable",
             "todo", "unimplemented", "dbg", "write", "writeln",
             "Some", "None", "Ok", "Err"].contains(&name)
        }
        LangStyle::Python => {
            ["print", "len", "range", "enumerate", "zip", "map", "filter",
             "str", "int", "float", "bool", "list", "dict", "set", "tuple",
             "isinstance", "issubclass", "type", "super", "property",
             "staticmethod", "classmethod", "hasattr", "getattr", "setattr",
             "open", "sorted", "reversed", "min", "max", "sum", "any", "all",
             "abs", "round", "id", "hash", "repr", "ord", "chr", "hex"].contains(&name)
        }
        LangStyle::TypeScript | LangStyle::JavaScript => {
            ["console", "require", "setTimeout", "setInterval",
             "parseInt", "parseFloat", "isNaN", "Array", "Object",
             "Promise", "JSON", "Math", "Date", "Error", "Map", "Set",
             "RegExp", "Number", "String", "Boolean", "Symbol"].contains(&name)
        }
        LangStyle::Go => {
            ["fmt", "log", "make", "len", "cap", "append", "copy",
             "delete", "new", "panic", "recover", "close", "print", "println"].contains(&name)
        }
        LangStyle::Java | LangStyle::CSharp => {
            ["System", "String", "Integer", "Boolean", "Double", "Float",
             "Object", "Class", "Math", "Arrays", "Collections",
             "Console", "Convert"].contains(&name)
        }
    }
}

fn is_primitive_type(name: &str, style: LangStyle) -> bool {
    match style {
        LangStyle::Rust => RUST_PRIMITIVES.contains(&name),
        LangStyle::Python => {
            ["str", "int", "float", "bool", "None", "list", "dict", "set",
             "tuple", "bytes", "type", "object", "Any", "Optional", "List",
             "Dict", "Set", "Tuple", "Union", "Callable"].contains(&name)
        }
        LangStyle::TypeScript | LangStyle::JavaScript => {
            ["string", "number", "boolean", "void", "null", "undefined",
             "any", "never", "unknown", "object", "Array", "Promise",
             "Record", "Partial", "Required", "Readonly", "Pick", "Omit"].contains(&name)
        }
        LangStyle::Go => {
            ["int", "int8", "int16", "int32", "int64",
             "uint", "uint8", "uint16", "uint32", "uint64",
             "float32", "float64", "string", "bool", "byte", "rune",
             "error", "any", "comparable"].contains(&name)
        }
        LangStyle::Java | LangStyle::CSharp => {
            ["int", "long", "short", "byte", "float", "double", "boolean",
             "char", "void", "String", "Object", "var", "dynamic"].contains(&name)
        }
    }
}

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
        // Python/TS keyword arguments: password_hash=value → WritesField
        "keyword_argument" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                if name.len() >= 3 {
                    refs.push(CodeReference {
                        source_file: file.into(),
                        source_line: node.start_position().row + 1,
                        target_name: name,
                        ref_type: RefType::WritesField,
                    });
                }
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

/// Rust-specific entity extraction (delegates to the v1 extract_from_node).
/// Only extracts entities for the current node — does NOT recurse (recursion handled by extract_generic).
fn extract_entity_rust(
    node: &tree_sitter::Node,
    source: &[u8],
    file: &str,
    entities: &mut Vec<CodeEntity>,
    impl_type: Option<&str>,
) {
    // Use the existing Rust-specific logic but only for entity extraction (not recursion)
    let kind = node.kind();
    match kind {
        "function_item" | "struct_item" | "enum_item" | "trait_item"
        | "use_declaration" | "const_item" | "static_item" => {
            // Create a temporary vec, extract, then move results
            let mut temp = Vec::new();
            extract_from_node_single(node, source, file, &mut temp, impl_type);
            entities.extend(temp);
        }
        _ => {}
    }
}

/// Extract entity from a single Rust AST node (no recursion).
fn extract_from_node_single(
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
                let etype = if impl_type.is_some() { "Method" } else { "Function" };
                let def = if let Some(t) = impl_type {
                    format!("{}::{} — {}", t, name, sig.trim())
                } else {
                    sig.trim().to_string()
                };
                entities.push(CodeEntity {
                    name, entity_type: etype.into(), definition: def,
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
                    name: name.clone(), entity_type: "Struct".into(),
                    definition: format!("struct {}", name), file: file.into(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    for child in body.children(&mut cursor) {
                        if child.kind() == "field_declaration" {
                            if let Some(field_name) = child.child_by_field_name("name") {
                                entities.push(CodeEntity {
                                    name: format!("{}.{}", name, node_text(&field_name, source)),
                                    entity_type: "Field".into(),
                                    definition: node_text(&child, source).trim().to_string(),
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
        "enum_item" | "trait_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let etype = if node.kind() == "enum_item" { "Enum" } else { "Trait" };
                entities.push(CodeEntity {
                    name, entity_type: etype.into(),
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
                name: text.clone(), entity_type: "Import".into(),
                definition: text, file: file.into(),
                line_start: node.start_position().row + 1,
                line_end: node.end_position().row + 1,
            });
        }
        "const_item" | "static_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                entities.push(CodeEntity {
                    name: node_text(&name_node, source).to_string(),
                    entity_type: "Const".into(),
                    definition: first_line(node_text(node, source)).trim().to_string(),
                    file: file.into(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });
            }
        }
        _ => {}
    }
}

/// Original v1 Rust extraction — used by parse_rust_code (v1 backward compat).
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

    // ── Multi-language tests via parse_file ──

    #[test]
    fn test_parse_file_rust() {
        let (entities, refs) = parse_file("pub fn hello() {} fn caller() { hello(); }", "test.rs");
        assert!(entities.iter().any(|e| e.name == "hello"));
        assert!(refs.iter().any(|r| r.target_name == "hello" && r.ref_type == RefType::Calls));
    }

    #[test]
    fn test_parse_file_python() {
        let code = r#"
class UserService:
    def get_user(self, user_id):
        return self.db.find(user_id)

def process_users():
    service = UserService()
    service.get_user(123)

import os
from pathlib import Path
"#;
        let (entities, refs) = parse_file(code, "app/service.py");
        assert!(entities.iter().any(|e| e.name == "UserService" && e.entity_type == "Struct"),
            "Should find class: {:?}", entities);
        assert!(entities.iter().any(|e| e.name == "process_users" && e.entity_type == "Function"),
            "Should find function: {:?}", entities);
        assert!(entities.iter().any(|e| e.name == "get_user" && e.entity_type == "Method"),
            "Should find method: {:?}", entities);
        assert!(entities.iter().any(|e| e.entity_type == "Import"),
            "Should find imports: {:?}", entities);
        // References
        assert!(refs.iter().any(|r| r.target_name == "get_user" && r.ref_type == RefType::MethodCall),
            "Should find method call: {:?}", refs);
    }

    #[test]
    fn test_parse_file_typescript() {
        let code = r#"
interface User {
    id: number;
    name: string;
}

class UserRepository {
    findById(id: number): User {
        return this.db.query(id);
    }
}

function processUser(repo: UserRepository): void {
    const user = repo.findById(1);
    console.log(user.name);
}

export const MAX_USERS = 100;
"#;
        let (entities, refs) = parse_file(code, "src/user.ts");
        assert!(entities.iter().any(|e| e.name == "User" && e.entity_type == "Trait"),
            "Should find interface: {:?}", entities);
        assert!(entities.iter().any(|e| e.name == "UserRepository" && e.entity_type == "Struct"),
            "Should find class: {:?}", entities);
        assert!(entities.iter().any(|e| e.name == "processUser" && e.entity_type == "Function"),
            "Should find function: {:?}", entities);
        // References
        assert!(refs.iter().any(|r| r.target_name == "findById" && r.ref_type == RefType::MethodCall),
            "Should find method call: {:?}", refs);
    }

    #[test]
    fn test_parse_file_javascript() {
        let code = r#"
class ApiClient {
    fetch(url) {
        return this.http.get(url);
    }
}

function main() {
    const client = new ApiClient();
    client.fetch("/api/users");
}
"#;
        let (entities, refs) = parse_file(code, "src/api.js");
        assert!(entities.iter().any(|e| e.name == "ApiClient"),
            "Should find class: {:?}", entities);
        assert!(refs.iter().any(|r| r.target_name == "fetch" && r.ref_type == RefType::MethodCall),
            "Should find method call: {:?}", refs);
    }

    #[test]
    fn test_parse_file_go() {
        let code = r#"
package main

import "fmt"

type Server struct {
    Port int
    Host string
}

func NewServer(port int) *Server {
    return &Server{Port: port}
}

func (s *Server) Start() {
    fmt.Println("Starting")
}
"#;
        let (entities, refs) = parse_file(code, "main.go");
        assert!(entities.iter().any(|e| e.name == "NewServer" && e.entity_type == "Function"),
            "Should find function: {:?}", entities);
        assert!(entities.iter().any(|e| e.entity_type == "Method"),
            "Should find method: {:?}", entities);
    }

    #[test]
    fn test_parse_file_java() {
        let code = r#"
import java.util.List;

public class UserService {
    private UserRepository repository;

    public User findUser(int id) {
        return repository.findById(id);
    }
}
"#;
        let (entities, refs) = parse_file(code, "UserService.java");
        assert!(entities.iter().any(|e| e.name == "UserService" && e.entity_type == "Struct"),
            "Should find class: {:?}", entities);
        assert!(entities.iter().any(|e| e.name == "findUser" && e.entity_type == "Method"),
            "Should find method: {:?}", entities);
        assert!(entities.iter().any(|e| e.entity_type == "Import"),
            "Should find import: {:?}", entities);
    }

    #[test]
    fn test_parse_file_unknown_extension() {
        let (entities, refs) = parse_file("hello world", "readme.txt");
        assert!(entities.is_empty());
        assert!(refs.is_empty());
    }

    #[test]
    fn test_parse_file_detects_language() {
        // Verify all supported extensions are recognized
        assert!(!parse_file("fn x(){}", "test.rs").0.is_empty());
        assert!(!parse_file("def x(): pass", "test.py").0.is_empty());
        assert!(!parse_file("function x(){}", "test.js").0.is_empty());
        assert!(!parse_file("function x(){}", "test.ts").0.is_empty());
        assert!(!parse_file("func x(){}", "test.go").0.is_empty());
    }
}
