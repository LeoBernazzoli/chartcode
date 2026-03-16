use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

use crate::graph::{self, ExtractionResult, ExtractedEntity, ExtractedRelation};
use crate::model::*;
use crate::prompt;
use crate::storage;

/// Python-facing KnowledgeGraph wrapper.
#[pyclass]
pub struct PyKnowledgeGraph {
    inner: graph::KnowledgeGraph,
    path: PathBuf,
}

#[pymethods]
impl PyKnowledgeGraph {
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let p = PathBuf::from(path);
        let kg = storage::load_or_create(&p)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        Ok(Self { inner: kg, path: p })
    }

    /// Save the graph to disk.
    fn save(&self) -> PyResult<()> {
        storage::save(&self.inner, &self.path)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    // ── Ontology ────────────────────────────────────────────────

    /// Get a prompt for the agent to analyze content and suggest ontology.
    fn analyze_content(&self, text: &str) -> String {
        prompt::analyze_content(text, &self.inner.ontology).prompt
    }

    /// Update the ontology from the agent's response (JSON string).
    fn update_ontology(&mut self, json_response: &str) -> PyResult<()> {
        let parsed: serde_json::Value = serde_json::from_str(json_response)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON: {}", e)))?;

        let mut new_ontology = Ontology::default();

        if let Some(domain) = parsed.get("domain").and_then(|v| v.as_str()) {
            new_ontology.domain = domain.to_string();
        }

        if let Some(types) = parsed.get("suggested_entity_types").and_then(|v| v.as_array()) {
            for t in types {
                new_ontology.node_types.push(NodeType {
                    name: t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    description: t.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    parent: t.get("parent").and_then(|v| v.as_str()).map(String::from),
                });
            }
        }

        if let Some(types) = parsed.get("suggested_relation_types").and_then(|v| v.as_array()) {
            for t in types {
                new_ontology.edge_types.push(EdgeType {
                    name: t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    description: t.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    from_types: t.get("from_types")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                    to_types: t.get("to_types")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                });
            }
        }

        self.inner.ontology.merge(&new_ontology);
        Ok(())
    }

    // ── Extraction ──────────────────────────────────────────────

    /// Get a prompt for the agent to extract entities and relations.
    fn prepare_extraction(&self, text: &str) -> String {
        let existing: Vec<String> = self.inner.nodes.values()
            .map(|n| format!("{} ({})", n.name, n.node_type))
            .collect();
        prompt::prepare_extraction(text, &self.inner.ontology, &existing).prompt
    }

    /// Ingest the agent's extraction response (JSON string).
    /// Returns a dict with {added, merged, rejected, edges_added, errors}.
    fn ingest<'py>(&mut self, py: Python<'py>, json_response: &str) -> PyResult<Bound<'py, PyDict>> {
        let parsed: serde_json::Value = serde_json::from_str(json_response)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON: {}", e)))?;

        let extraction = parse_extraction_result(&parsed, &Source::Document {
            name: "unknown".into(),
            page: None,
        })?;

        let report = self.inner.ingest(&extraction);

        let dict = PyDict::new(py);
        dict.set_item("added", report.added)?;
        dict.set_item("merged", report.merged)?;
        dict.set_item("rejected", report.rejected)?;
        dict.set_item("edges_added", report.edges_added)?;
        dict.set_item("edges_deduped", report.edges_deduped)?;
        dict.set_item("errors", &report.errors)?;
        Ok(dict)
    }

    /// Ingest with a specific document source.
    fn ingest_document<'py>(
        &mut self,
        py: Python<'py>,
        json_response: &str,
        document_name: &str,
        page: Option<u32>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let parsed: serde_json::Value = serde_json::from_str(json_response)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON: {}", e)))?;

        let source = Source::Document {
            name: document_name.to_string(),
            page,
        };
        let extraction = parse_extraction_result(&parsed, &source)?;
        let report = self.inner.ingest(&extraction);

        self.inner.add_document(document_name);

        let dict = PyDict::new(py);
        dict.set_item("added", report.added)?;
        dict.set_item("merged", report.merged)?;
        dict.set_item("rejected", report.rejected)?;
        dict.set_item("edges_added", report.edges_added)?;
        dict.set_item("edges_deduped", report.edges_deduped)?;
        dict.set_item("errors", &report.errors)?;
        Ok(dict)
    }

    // ── Memory ──────────────────────────────────────────────────

    /// Get a prompt for the agent to extract from a memory/fact.
    fn prepare_memory(&self, text: &str) -> String {
        prompt::prepare_memory(text, &self.inner).prompt
    }

    /// Ingest a memory extraction result.
    fn ingest_memory<'py>(&mut self, py: Python<'py>, json_response: &str) -> PyResult<Bound<'py, PyDict>> {
        let parsed: serde_json::Value = serde_json::from_str(json_response)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON: {}", e)))?;

        let extraction = parse_extraction_result(&parsed, &Source::Memory)?;
        let report = self.inner.ingest(&extraction);

        let dict = PyDict::new(py);
        dict.set_item("added", report.added)?;
        dict.set_item("merged", report.merged)?;
        dict.set_item("rejected", report.rejected)?;
        dict.set_item("edges_added", report.edges_added)?;
        dict.set_item("edges_deduped", report.edges_deduped)?;
        dict.set_item("errors", &report.errors)?;
        Ok(dict)
    }

    // ── Chunking ─────────────────────────────────────────────────

    /// Split text into overlapping chunks for extraction.
    /// Returns a list of chunk strings.
    #[pyo3(signature = (text, chunk_size=4000, overlap=500))]
    fn chunk_text(&self, text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
        crate::chunker::chunk_text(text, chunk_size, overlap)
            .into_iter()
            .map(|c| c.text)
            .collect()
    }

    // ── Navigation ──────────────────────────────────────────────

    /// Lookup an entity by name. Returns JSON string or None.
    fn lookup(&self, name: &str) -> Option<String> {
        self.inner.lookup(name).map(|n| serde_json::to_string(n).unwrap_or_default())
    }

    /// Get neighbors of an entity. Returns JSON array string.
    fn neighbors(&self, name: &str) -> PyResult<String> {
        let node = self.inner.lookup(name)
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("Entity '{}' not found", name)))?;
        let neighbors = self.inner.neighbors(node.id);
        Ok(serde_json::to_string(&neighbors).unwrap_or_else(|_| "[]".into()))
    }

    /// Get neighbors filtered by entity type.
    fn neighbors_by_type(&self, name: &str, type_filter: &str) -> PyResult<String> {
        let node = self.inner.lookup(name)
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("Entity '{}' not found", name)))?;
        let neighbors = self.inner.neighbors_by_type(node.id, type_filter);
        Ok(serde_json::to_string(&neighbors).unwrap_or_else(|_| "[]".into()))
    }

    /// Follow a specific relation type from an entity.
    fn follow(&self, name: &str, relation_type: &str) -> PyResult<String> {
        let node = self.inner.lookup(name)
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("Entity '{}' not found", name)))?;
        let result = self.inner.follow(node.id, relation_type);
        Ok(serde_json::to_string(&result).unwrap_or_else(|_| "[]".into()))
    }

    /// Find path between two entities. Returns JSON string.
    fn path(&self, from_name: &str, to_name: &str) -> String {
        let result = self.inner.path(from_name, to_name);
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"found":false}"#.into())
    }

    /// Explore an entity: get it + all connections + evidence. Returns JSON string.
    fn explore(&self, name: &str) -> Option<String> {
        self.inner.explore(name).map(|r| serde_json::to_string(&r).unwrap_or_default())
    }

    /// Find path between two entities and return as readable string.
    fn connect(&self, a: &str, b: &str) -> String {
        let result = self.inner.path(a, b);
        if !result.found {
            return format!("No path found between '{}' and '{}'.", a, b);
        }
        let mut parts: Vec<String> = Vec::new();
        for (i, node) in result.nodes.iter().enumerate() {
            parts.push(node.name.clone());
            if i < result.edges.len() {
                parts.push(format!("--[{}]-->", result.edges[i].relation_type));
            }
        }
        parts.join(" ")
    }

    // ── Stats ───────────────────────────────────────────────────

    /// Get graph stats as JSON string.
    fn stats(&self) -> String {
        serde_json::to_string(&self.inner.stats()).unwrap_or_else(|_| "{}".into())
    }

    /// Get main topics as JSON string.
    fn topics(&self) -> String {
        serde_json::to_string(&self.inner.topics()).unwrap_or_else(|_| "{}".into())
    }

    /// Get recent entities as JSON array string.
    fn recent(&self, limit: Option<usize>) -> String {
        let nodes = self.inner.recent(limit.unwrap_or(10));
        serde_json::to_string(&nodes).unwrap_or_else(|_| "[]".into())
    }

    /// Export entire graph as JSON string.
    fn export_json(&self) -> String {
        serde_json::to_string_pretty(&self.inner).unwrap_or_else(|_| "{}".into())
    }

    /// Get ontology as JSON string.
    fn get_ontology(&self) -> String {
        serde_json::to_string(&self.inner.ontology).unwrap_or_else(|_| "{}".into())
    }

    /// Connect orphan nodes by scanning definitions for entity mentions.
    /// Returns the number of new connections created.
    fn connect_orphans(&mut self) -> usize {
        self.inner.connect_orphans()
    }

    /// Discover implicit connections across ALL nodes by scanning definitions.
    /// Returns the number of new connections created.
    fn discover_connections(&mut self) -> usize {
        self.inner.discover_connections()
    }

    /// Get graph quality metrics as JSON string.
    fn quality_metrics(&self) -> String {
        serde_json::to_string(&self.inner.quality_metrics()).unwrap_or_else(|_| "{}".into())
    }
}

/// Parse the agent's JSON response into an ExtractionResult.
fn parse_extraction_result(
    parsed: &serde_json::Value,
    default_source: &Source,
) -> PyResult<ExtractionResult> {
    let mut entities = Vec::new();
    let mut relations = Vec::new();

    if let Some(ent_array) = parsed.get("entities").and_then(|v| v.as_array()) {
        for e in ent_array {
            // Use evidence_text if provided, otherwise use definition as evidence
            let evidence_text = e.get("evidence_text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let definition = e.get("definition")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let best_evidence = if !evidence_text.is_empty() {
                evidence_text
            } else {
                definition
            };

            let evidence = if !best_evidence.is_empty() {
                vec![Evidence {
                    document: match default_source {
                        Source::Document { name, .. } => name.clone(),
                        _ => "memory".to_string(),
                    },
                    page: match default_source {
                        Source::Document { page, .. } => *page,
                        _ => None,
                    },
                    text_snippet: best_evidence.to_string(),
                    offset_start: 0,
                    offset_end: 0,
                }]
            } else {
                Vec::new()
            };

            entities.push(ExtractedEntity {
                name: e.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                entity_type: e.get("type").and_then(|v| v.as_str()).unwrap_or("concept").to_string(),
                definition: e.get("definition").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                aliases: e.get("aliases")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                confidence: e.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32,
                source: default_source.clone(),
                evidence,
            });
        }
    }

    if let Some(rel_array) = parsed.get("relations").and_then(|v| v.as_array()) {
        for r in rel_array {
            let evidence_text = r.get("evidence_text")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let evidence = if !evidence_text.is_empty() {
                vec![Evidence {
                    document: match default_source {
                        Source::Document { name, .. } => name.clone(),
                        _ => "memory".to_string(),
                    },
                    page: match default_source {
                        Source::Document { page, .. } => *page,
                        _ => None,
                    },
                    text_snippet: evidence_text.to_string(),
                    offset_start: 0,
                    offset_end: 0,
                }]
            } else {
                Vec::new()
            };

            relations.push(ExtractedRelation {
                source: r.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                target: r.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                relation_type: r.get("type").and_then(|v| v.as_str()).unwrap_or("related_to").to_string(),
                confidence: r.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32,
                source_ref: default_source.clone(),
                evidence,
            });
        }
    }

    Ok(ExtractionResult {
        entities,
        relations,
    })
}

// ── Claude Code Integration ─────────────────────────────────

/// List available Claude Code projects.
#[pyfunction]
fn list_claude_projects() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let projects_dir = std::path::PathBuf::from(&home).join(".claude").join("projects");
    let mut projects = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Convert back to path: -Users-foo-bar -> /Users/foo/bar
                let path = name.replace('-', "/");
                let path = if path.starts_with('/') { path } else { format!("/{}", path) };
                projects.push(path);
            }
        }
    }
    projects.sort();
    projects
}

/// Parse Claude Code conversations for a project path.
/// Returns a list of (session_id, message_count, text_preview) tuples.
#[pyfunction]
fn parse_claude_project(project_path: &str) -> Vec<(String, usize, String)> {
    let path = std::path::Path::new(project_path);
    let files = crate::claude_parser::find_conversations(path);
    let mut results = Vec::new();

    for file in files {
        if let Some(conv) = crate::claude_parser::parse_conversation(&file) {
            let preview = conv.substantive_text(200);
            results.push((conv.session_id, conv.messages.len(), preview));
        }
    }
    results
}

/// Get the full text of a Claude Code conversation, ready for KG extraction.
/// mode: "all" (default), "user" (user messages only), "substantive" (sampled)
#[pyfunction]
#[pyo3(signature = (project_path, session_id, max_chars, mode="substantive"))]
fn get_claude_conversation_text(project_path: &str, session_id: &str, max_chars: usize, mode: &str) -> Option<String> {
    let path = std::path::Path::new(project_path);
    let files = crate::claude_parser::find_conversations(path);

    for file in files {
        let filename = file.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("");
        if filename.contains(session_id) {
            if let Some(conv) = crate::claude_parser::parse_conversation(&file) {
                return Some(match mode {
                    "user" => conv.user_messages_text(max_chars),
                    "all" => conv.to_text(max_chars),
                    _ => conv.substantive_text(max_chars),
                });
            }
        }
    }
    None
}

/// Get ALL conversation texts for a project, concatenated.
#[pyfunction]
fn get_all_claude_conversations(project_path: &str, max_chars_per_conv: usize) -> Vec<(String, String)> {
    let path = std::path::Path::new(project_path);
    let files = crate::claude_parser::find_conversations(path);
    let mut results = Vec::new();

    for file in files {
        if let Some(conv) = crate::claude_parser::parse_conversation(&file) {
            let text = conv.substantive_text(max_chars_per_conv);
            if !text.is_empty() {
                results.push((conv.session_id, text));
            }
        }
    }
    results
}

/// PyO3 module definition.
#[pymodule]
fn autoclaw(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyKnowledgeGraph>()?;
    m.add_function(wrap_pyfunction!(list_claude_projects, m)?)?;
    m.add_function(wrap_pyfunction!(parse_claude_project, m)?)?;
    m.add_function(wrap_pyfunction!(get_claude_conversation_text, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_claude_conversations, m)?)?;
    Ok(())
}
