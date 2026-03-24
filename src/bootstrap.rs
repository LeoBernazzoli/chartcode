use crate::chunker;
use crate::claude_parser;
use crate::config::GraphocodeConfig;
use crate::graph::KnowledgeGraph;
use crate::model::{Node, Source};
use crate::tier::ImportanceTier;
use crate::treesitter;
use std::collections::{HashMap, HashSet};
use std::path::Path;

fn normalize_code_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .strip_prefix("./")
        .unwrap_or(&normalized)
        .to_string()
}

/// Check if a file path is inside a dependency/build directory that should be skipped.
fn is_dependency_path(path: &str) -> bool {
    let normalized = normalize_code_path(path);
    let skip_dirs = [
        "node_modules",
        ".next",
        "dist",
        "build",
        "target",
        "__pycache__",
        ".venv",
        "venv",
        "env",
        ".git",
        "vendor",
        ".tox",
        "site-packages",
        ".mypy_cache",
        ".pytest_cache",
        "coverage",
        ".nuxt",
        ".output",
        "out",
        ".svelte-kit",
    ];
    normalized
        .split('/')
        .any(|component| skip_dirs.contains(&component))
}

#[derive(Debug)]
pub struct BootstrapReport {
    pub files_indexed: usize,
    pub code_entities: usize,
    pub conversations_found: usize,
    pub conversation_texts: Vec<(String, String)>, // (session_id, text) for Haiku extraction
    pub document_chunks: Vec<(String, String)>,    // (doc_path, chunk_text) for Haiku extraction
}

#[derive(Debug)]
struct ParsedCodeFile {
    path: String,
    entities: Vec<treesitter::CodeEntity>,
    references: Vec<treesitter::CodeReference>,
    imports: Vec<String>,
}

#[derive(Debug, Clone)]
struct IndexedEntity {
    id: u64,
    name: String,
    node_type: String,
    file: String,
}

fn entity_lookup_keys(name: &str) -> Vec<String> {
    let short = name
        .rsplit("::")
        .next()
        .unwrap_or(name)
        .rsplit('.')
        .next()
        .unwrap_or(name);
    if short == name {
        vec![name.to_string()]
    } else {
        vec![name.to_string(), short.to_string()]
    }
}

fn insert_lookup_index(index: &mut HashMap<String, Vec<usize>>, key: String, candidate_idx: usize) {
    let entry = index.entry(key).or_default();
    if entry.last().copied() != Some(candidate_idx) {
        entry.push(candidate_idx);
    }
}

fn collect_parsed_code_files(base_dir: &Path) -> Vec<ParsedCodeFile> {
    let supported_extensions = ["rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "cs"];
    let mut parsed_files = Vec::new();

    for entry in ignore::WalkBuilder::new(base_dir)
        .hidden(false)
        .build()
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !supported_extensions.contains(&ext) {
            continue;
        }
        let relative_path = path.strip_prefix(base_dir).unwrap_or(path);
        let raw_path = relative_path.to_string_lossy().to_string();
        if is_dependency_path(&raw_path) {
            continue;
        }
        let code = match std::fs::read_to_string(path) {
            Ok(code) => code,
            Err(_) => continue,
        };
        let path_str = normalize_code_path(&raw_path);
        let (entities, references) = treesitter::parse_file(&code, &path_str);
        let imports = entities
            .iter()
            .filter(|entity| entity.entity_type == "Import")
            .map(|entity| entity.name.clone())
            .collect();
        parsed_files.push(ParsedCodeFile {
            path: path_str,
            entities,
            references,
            imports,
        });
    }

    parsed_files
}

fn build_unique_global_lookup(
    entities_by_lookup: &HashMap<String, Vec<usize>>,
    indexed_entities: &[IndexedEntity],
) -> HashMap<String, usize> {
    let mut unique = HashMap::new();
    for (key, candidates) in entities_by_lookup {
        let mut matched_idx: Option<usize> = None;
        let mut ambiguous = false;
        for candidate_idx in candidates {
            let candidate = &indexed_entities[*candidate_idx];
            if candidate.node_type == "Import" || candidate.node_type == "File" {
                continue;
            }
            if let Some(existing_idx) = matched_idx {
                if indexed_entities[existing_idx].id != candidate.id {
                    ambiguous = true;
                    break;
                }
            } else {
                matched_idx = Some(*candidate_idx);
            }
        }
        if !ambiguous {
            if let Some(candidate_idx) = matched_idx {
                unique.insert(key.clone(), candidate_idx);
            }
        }
    }
    unique
}

/// CHANNEL 1: Index all code files using tree-sitter. Deterministic, 0 tokens.
/// V2: extracts both definitions AND references for complete code graph.
pub fn bootstrap_code(kg: &mut KnowledgeGraph, config: &GraphocodeConfig) -> (usize, usize) {
    let base_dir = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    bootstrap_code_at(kg, config, &base_dir)
}

pub fn bootstrap_code_at(
    kg: &mut KnowledgeGraph,
    config: &GraphocodeConfig,
    base_dir: &Path,
) -> (usize, usize) {
    let parsed_files = collect_parsed_code_files(base_dir);
    let files = parsed_files.len();
    let mut entities = 0;
    let _ = config;

    let all_files_set: HashSet<String> = parsed_files.iter().map(|file| file.path.clone()).collect();

    let mut indexed_entities: Vec<IndexedEntity> = Vec::new();
    let mut entities_by_file: HashMap<String, Vec<usize>> = HashMap::new();
    let mut entities_by_lookup: HashMap<String, Vec<usize>> = HashMap::new();

    // Pass 1: definitions only (so all entities exist before we resolve references)
    for parsed_file in &parsed_files {
        entities += parsed_file.entities.len();
        for entity in &parsed_file.entities {
            let mut node = Node::new(
                0,
                entity.name.clone(),
                entity.entity_type.clone(),
                entity.definition.clone(),
                1.0,
                Source::CodeAnalysis {
                    file: parsed_file.path.clone(),
                },
            );
            node.tier = ImportanceTier::Minor;
            let node_id = match kg.add_node(node) {
                Ok(id) => id,
                Err(_) => continue,
            };

            if entity.entity_type != "Import" {
                let candidate_idx = indexed_entities.len();
                indexed_entities.push(IndexedEntity {
                    id: node_id,
                    name: entity.name.clone(),
                    node_type: entity.entity_type.clone(),
                    file: parsed_file.path.clone(),
                });
                entities_by_file
                    .entry(parsed_file.path.clone())
                    .or_default()
                    .push(candidate_idx);
                for key in entity_lookup_keys(&entity.name) {
                    insert_lookup_index(&mut entities_by_lookup, key, candidate_idx);
                }
            }
        }
    }

    // Pass 2: Build import map + tiered reference resolution
    //
    // Inspired by GitNexus: resolve references using import tracking
    // Tier 1: same file (confidence 0.95)
    // Tier 2: import-scoped — target defined in a file we import (confidence 0.9)
    // Tier 3: global — search all files (confidence 0.5, only for unique matches)

    // 2a. Build import map: file → set of files it imports from
    let mut import_map: HashMap<String, HashSet<String>> = HashMap::new();

    // Build monorepo package map: @scope/name → directory entry point
    // Scan for package.json files near code files
    let mut package_map: HashMap<String, String> = HashMap::new();

    // Collect unique directories from code files, then check for package.json in each
    let mut checked_dirs: HashSet<String> = HashSet::new();
    for parsed_file in &parsed_files {
        let mut dir = std::path::Path::new(&parsed_file.path).parent();
        while let Some(d) = dir {
            let ds = normalize_code_path(&d.to_string_lossy());
            if checked_dirs.contains(&ds) {
                break;
            }
            checked_dirs.insert(ds.clone());
            let pkg_path = base_dir.join(d).join("package.json");
            if pkg_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&pkg_path) {
                    if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
                            let pkg_dir = normalize_code_path(
                                &pkg_path
                                    .parent()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                            );

                            // Find entry point: exports["."] or main or src/index.ts
                            let entry = pkg
                                .get("exports")
                                .and_then(|e| {
                                    e.get(".")
                                        .and_then(|dot| {
                                            dot.as_str().map(|s| s.to_string()).or_else(|| {
                                                dot.get("import")
                                                    .or_else(|| dot.get("default"))
                                                    .or_else(|| dot.get("require"))
                                                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                                            })
                                        })
                                        .or_else(|| e.as_str().map(|s| s.to_string()))
                                })
                                .or_else(|| {
                                    pkg.get("main").and_then(|m| m.as_str().map(|s| s.to_string()))
                                })
                                .unwrap_or_else(|| "src/index.ts".to_string());

                            let entry_clean = entry.trim_start_matches("./");
                            let entry_path = normalize_code_path(&if pkg_dir.is_empty() {
                                entry_clean.to_string()
                            } else {
                                format!("{}/{}", pkg_dir, entry_clean)
                            });

                            let resolved = if all_files_set.contains(&entry_path) {
                                Some(entry_path)
                            } else {
                                let base = entry_path
                                    .trim_end_matches(".js")
                                    .trim_end_matches(".cjs")
                                    .trim_end_matches(".mjs")
                                    .trim_end_matches(".ts")
                                    .trim_end_matches(".tsx")
                                    .to_string();
                                let src_base = if pkg_dir.is_empty() {
                                    "src/index".to_string()
                                } else {
                                    format!("{}/src/index", pkg_dir)
                                };
                                [".ts", ".tsx", ".js", "/index.ts", "/index.js"]
                                    .iter()
                                    .find_map(|ext| {
                                        let candidate = normalize_code_path(&format!("{}{}", base, ext));
                                        if all_files_set.contains(&candidate) {
                                            return Some(candidate);
                                        }
                                        let src_candidate =
                                            normalize_code_path(&format!("{}{}", src_base, ext));
                                        if all_files_set.contains(&src_candidate) {
                                            return Some(src_candidate);
                                        }
                                        None
                                    })
                            };

                            if let Some(resolved_path) = resolved {
                                package_map.insert(name.to_string(), resolved_path.clone());
                                if let Some(exports) = pkg.get("exports").and_then(|e| e.as_object())
                                {
                                    for (subpath, value) in exports {
                                        if subpath == "." || subpath == "./package.json" {
                                            continue;
                                        }
                                        let sub_entry = value
                                            .as_str()
                                            .map(|s| s.to_string())
                                            .or_else(|| {
                                                value
                                                    .get("import")
                                                    .or_else(|| value.get("default"))
                                                    .and_then(|v| {
                                                        v.as_str().map(|s| s.to_string())
                                                    })
                                            });
                                        if let Some(sub) = sub_entry {
                                            let sub_path = normalize_code_path(&if pkg_dir.is_empty()
                                            {
                                                sub.trim_start_matches("./").to_string()
                                            } else {
                                                format!(
                                                    "{}/{}",
                                                    pkg_dir,
                                                    sub.trim_start_matches("./")
                                                )
                                            });
                                            let sub_key = format!(
                                                "{}/{}",
                                                name,
                                                subpath.trim_start_matches("./")
                                            );
                                            if all_files_set.contains(&sub_path) {
                                                package_map.insert(sub_key, sub_path);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } // if exists
            dir = d.parent();
        } // while dir
    } // for file_list
    if !package_map.is_empty() {
        eprintln!("  Monorepo packages: {}", package_map.len());
    }

    for parsed_file in &parsed_files {
        let mut imported_files = HashSet::new();
        for imp in &parsed_file.imports {
            // Extract module name from import statement
            // Python: "from models import User" → "models"
            // Python: "import os" → "os"
            // TS/JS: "import { X } from './models'" → "./models"
            // Rust: "use crate::model::Node" → "model"
            let module_name = extract_module_from_import(imp, &parsed_file.path);
            // Try package map first (monorepo: @trpc/server → packages/server/src/index.ts)
            if let Some(pkg_file) = package_map.get(&module_name) {
                imported_files.insert(pkg_file.clone());
            }
            if let Some(resolved) =
                resolve_import_to_file(&module_name, &parsed_file.path, &all_files_set)
            {
                imported_files.insert(resolved.clone());

                // Python: "from package import submodule" — also resolve submodule files
                // e.g. "from rest_framework import serializers" → rest_framework/serializers.py
                let first_line = imp.lines().next().unwrap_or("");
                if first_line.contains(" import ") && !first_line.starts_with("import ") {
                    let after_import = first_line.split(" import ").last().unwrap_or("");
                    let resolved_dir = std::path::Path::new(&resolved)
                        .parent()
                        .map(|p| normalize_code_path(&p.to_string_lossy()));
                    if let Some(rdir) = resolved_dir {
                        // If resolved is __init__.py, the dir is the package dir
                        let pkg_dir = if resolved.ends_with("__init__.py")
                            || resolved.ends_with("index.ts")
                            || resolved.ends_with("index.js")
                        {
                            rdir
                        } else {
                            String::new()
                        };
                        if !pkg_dir.is_empty() {
                            for name_part in after_import.split(',') {
                                let name = name_part.trim().split_whitespace().next().unwrap_or("").trim_matches('(').trim();
                                if !name.is_empty() && name.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
                                    // Might be a submodule — try to resolve as file
                                    if let Some(sub) =
                                        resolve_import_to_file(name, &resolved, &all_files_set)
                                    {
                                        imported_files.insert(sub);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !imported_files.is_empty() {
            import_map.insert(parsed_file.path.clone(), imported_files);
        }
    }

    let mut transitive_import_map: HashMap<String, HashSet<String>> = HashMap::new();
    for (file, imports) in &import_map {
        let mut transitive_files = HashSet::new();
        for imported_file in imports {
            if let Some(second_hop) = import_map.get(imported_file) {
                transitive_files.extend(second_hop.iter().cloned());
            }
        }
        if !transitive_files.is_empty() {
            transitive_import_map.insert(file.clone(), transitive_files);
        }
    }

    let unique_global_lookup = build_unique_global_lookup(&entities_by_lookup, &indexed_entities);
    let mut file_node_ids: HashMap<String, u64> = HashMap::new();

    // 2b. Create file nodes and resolve references with tier system
    for parsed_file in &parsed_files {
        // Create file-level node (use exact match, not substring lookup)
        let file_node_id = if let Some(existing) = file_node_ids.get(&parsed_file.path) {
            *existing
        } else {
            let mut fnode = Node::new(
                0,
                parsed_file.path.clone(),
                "File".to_string(),
                format!("Source file {}", parsed_file.path),
                1.0,
                Source::CodeAnalysis {
                    file: parsed_file.path.clone(),
                },
            );
            fnode.tier = ImportanceTier::Minor;
            let node_id = kg.add_node(fnode).unwrap_or(0);
            file_node_ids.insert(parsed_file.path.clone(), node_id);
            node_id
        };

        let imported_files = import_map.get(parsed_file.path.as_str());
        let transitive_files = transitive_import_map.get(parsed_file.path.as_str());

        // Handle "from X import *" — create edges to ALL public entities in X
        for imp in &parsed_file.imports {
            let first_line = imp.lines().next().unwrap_or("");
            if first_line.contains("import *") || first_line.contains("import *") {
                // Find which file this wildcard import resolves to
                let module_name = extract_module_from_import(imp, &parsed_file.path);
                let resolved = package_map
                    .get(&module_name)
                    .cloned()
                    .or_else(|| {
                        resolve_import_to_file(&module_name, &parsed_file.path, &all_files_set)
                    });

                if let Some(src_file) = resolved {
                    // Create edges to all public entities in that file
                    let public_entities = entities_by_file
                        .get(&src_file)
                        .into_iter()
                        .flat_map(|candidates| candidates.iter())
                        .filter_map(|candidate_idx| {
                            let candidate = &indexed_entities[*candidate_idx];
                            if candidate.node_type == "File"
                                || candidate.node_type == "Import"
                                || candidate.name.starts_with('_')
                            {
                                None
                            } else {
                                Some(candidate.id)
                            }
                        });

                    for target_id in public_entities {
                        if file_node_id != 0 && file_node_id != target_id {
                            let edge = crate::model::Edge::new(
                                0,
                                file_node_id,
                                target_id,
                                "uses_type".to_string(),
                                1.0,
                                Source::CodeAnalysis {
                                    file: parsed_file.path.clone(),
                                },
                            );
                            let _ = kg.add_edge(edge);
                        }
                    }
                }
            }
        }

        for reference in &parsed_file.references {
            let target_name = &reference.target_name;
            let lookup_candidates = match entities_by_lookup.get(target_name) {
                Some(candidates) => candidates,
                None => continue,
            };

            // Tier 1: same file — exact match in this file
            let tier1 = lookup_candidates.iter().copied().find(|candidate_idx| {
                let candidate = &indexed_entities[*candidate_idx];
                candidate.file == parsed_file.path
                    && candidate.node_type != "File"
                    && candidate.node_type != "Import"
            });

            // Tier 2: import-scoped — defined in a file we import (or re-exported from)
            let tier2 = if tier1.is_none() {
                if let Some(imp_files) = imported_files {
                    // Direct: entity defined in imported file
                    let direct = lookup_candidates.iter().copied().find(|candidate_idx| {
                        let candidate = &indexed_entities[*candidate_idx];
                        candidate.node_type != "File"
                            && candidate.node_type != "Import"
                            && imp_files.contains(&candidate.file)
                    });

                    // If not found directly, follow re-exports (1 hop):
                    // Check files imported BY our imported files
                    if direct.is_some() {
                        direct
                    } else {
                        if let Some(transitive_files) = transitive_files {
                            lookup_candidates.iter().copied().find(|candidate_idx| {
                                let candidate = &indexed_entities[*candidate_idx];
                                candidate.node_type != "File"
                                    && candidate.node_type != "Import"
                                    && transitive_files.contains(&candidate.file)
                            })
                        } else {
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Tier 3: global — unique match across all files (skip if ambiguous)
            let tier3 = if tier1.is_none() && tier2.is_none() && target_name.len() >= 4 {
                unique_global_lookup.get(target_name).copied()
            } else {
                None
            };

            let target_node = tier1.or(tier2).or(tier3);

            if let Some(target_idx) = target_node {
                let target_id = indexed_entities[target_idx].id;
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
                            file: parsed_file.path.clone(),
                        },
                    );
                    let _ = kg.add_edge(edge);
                }
            }
        }
    }

    (files, entities)
}

/// Extract module name from an import statement.
fn extract_module_from_import(import_text: &str, _current_file: &str) -> String {
    // Use only first line for multiline imports
    let text = import_text.lines().next().unwrap_or("").trim();

    // Python: "from models import User" → "models"
    if text.starts_with("from ") {
        if let Some(module) = text.strip_prefix("from ") {
            if let Some(idx) = module.find(" import") {
                return module[..idx].trim().to_string();
            }
        }
    }

    // Python: "import os" → "os"
    if text.starts_with("import ") {
        return text
            .strip_prefix("import ")
            .unwrap_or("")
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
    }

    // TS/JS: "import { X } from './models'" or "import X from 'module'"
    if text.contains("from ") {
        if let Some(from_part) = text.split("from ").last() {
            return from_part
                .trim()
                .trim_matches(|c| c == '\'' || c == '"' || c == ';')
                .to_string();
        }
    }

    // Rust: "use crate::model::Node" → "model"
    if text.starts_with("use ") {
        let parts: Vec<&str> = text
            .strip_prefix("use ")
            .unwrap_or("")
            .trim_end_matches(';')
            .split("::")
            .collect();
        if parts.len() >= 2 {
            // Skip "crate", "self", "super"
            for part in &parts {
                let p = part.trim();
                if p != "crate" && p != "self" && p != "super" && !p.is_empty() {
                    return p.to_string();
                }
            }
        }
    }

    text.to_string()
}

/// Resolve a module name to a file path.
fn resolve_import_to_file(
    module_name: &str,
    current_file: &str,
    all_files: &HashSet<String>,
) -> Option<String> {
    if module_name.is_empty() {
        return None;
    }

    let current_dir = std::path::Path::new(current_file)
        .parent()
        .map(|p| normalize_code_path(&p.to_string_lossy()))
        .unwrap_or_default();

    // Relative import: ./models, ../utils, .module
    let is_relative = module_name.starts_with('.') || module_name.starts_with("./");

    if is_relative {
        let clean = module_name
            .trim_start_matches("./")
            .trim_start_matches('.');
        let base = if current_dir.is_empty() {
            clean.to_string()
        } else {
            format!("{}/{}", current_dir, clean)
        };

        // Try extensions
        for ext in &[".py", ".ts", ".tsx", ".js", ".jsx", ".rs", ".go", ".java", ".cs"] {
            let candidate = normalize_code_path(&format!("{}{}", base, ext));
            if all_files.contains(&candidate) {
                return Some(candidate);
            }
        }
        // Try as directory with index
        for idx in &["index.ts", "index.js", "index.tsx", "__init__.py", "mod.rs"] {
            let candidate = normalize_code_path(&format!("{}/{}", base, idx));
            if all_files.contains(&candidate) {
                return Some(candidate);
            }
        }
    }

    // Bare module: try proximity (same directory first, like Python)
    let module_path = module_name.replace('.', "/");

    // Same directory
    if !current_dir.is_empty() {
        for ext in &[".py", ".ts", ".tsx", ".js", ".jsx", ".rs", ".go", ".java", ".cs"] {
            let candidate = normalize_code_path(&format!("{}/{}{}", current_dir, module_path, ext));
            if all_files.contains(&candidate) {
                return Some(candidate);
            }
        }
        // Package directory
        for idx in &["__init__.py", "index.ts", "index.js", "mod.rs"] {
            let candidate =
                normalize_code_path(&format!("{}/{}/{}", current_dir, module_path, idx));
            if all_files.contains(&candidate) {
                return Some(candidate);
            }
        }
    }

    // Global suffix search: find any file ending with the module path
    let suffix_py = format!("{}.py", module_path);
    let suffix_ts = format!("{}.ts", module_path);
    let suffix_tsx = format!("{}.tsx", module_path);
    let suffix_rs = format!("{}.rs", module_path);
    let suffix_init = format!("{}/__init__.py", module_path); // Python packages
    let suffix_index_ts = format!("{}/index.ts", module_path); // TS packages
    let suffix_index_js = format!("{}/index.js", module_path); // JS packages
    let suffix_mod_rs = format!("{}/mod.rs", module_path); // Rust modules
    for file in all_files {
        if file.ends_with(&suffix_py)
            || file.ends_with(&suffix_ts)
            || file.ends_with(&suffix_tsx)
            || file.ends_with(&suffix_rs)
            || file.ends_with(&suffix_init)
            || file.ends_with(&suffix_index_ts)
            || file.ends_with(&suffix_index_js)
            || file.ends_with(&suffix_mod_rs)
        {
            return Some(file.clone());
        }
    }

    None
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
/// for semantic extraction (done by the /chartcode:start skill which orchestrates LLM calls).
pub fn bootstrap(
    kg: &mut KnowledgeGraph,
    config: &GraphocodeConfig,
    project_path: &Path,
) -> BootstrapReport {
    let (files, code_entities) = bootstrap_code_at(kg, config, project_path);

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
    fn test_normalize_code_path() {
        assert_eq!(normalize_code_path("./src/main.rs"), "src/main.rs");
        assert_eq!(normalize_code_path("src\\main.rs"), "src/main.rs");
        assert_eq!(normalize_code_path("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn test_entity_lookup_keys_include_short_name_once() {
        assert_eq!(entity_lookup_keys("Node"), vec!["Node".to_string()]);
        assert_eq!(
            entity_lookup_keys("User.password_hash"),
            vec!["User.password_hash".to_string(), "password_hash".to_string()]
        );
    }

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
