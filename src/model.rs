use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::tier::ImportanceTier;

// ── Core Types ──────────────────────────────────────────────────

pub type NodeId = u64;
pub type Timestamp = u64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Source {
    Document { name: String, page: Option<u32> },
    Memory,
    Inferred,
    CodeAnalysis { file: String },
    Conversation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub document: String,
    pub page: Option<u32>,
    pub text_snippet: String,
    pub offset_start: usize,
    pub offset_end: usize,
}

// ── Node ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub node_type: String,
    pub definition: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub aliases: Vec<String>,
    pub confidence: f32,
    pub source: Source,
    pub created_at: Timestamp,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub tier: ImportanceTier,
    #[serde(default)]
    pub superseded_by: Option<NodeId>,
    #[serde(default)]
    pub last_referenced: Timestamp,
}

impl Node {
    pub fn new(
        id: NodeId,
        name: String,
        node_type: String,
        definition: String,
        confidence: f32,
        source: Source,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            id,
            name,
            node_type,
            definition,
            properties: HashMap::new(),
            aliases: Vec::new(),
            confidence,
            source,
            created_at: now,
            evidence: Vec::new(),
            tier: ImportanceTier::default(),
            superseded_by: None,
            last_referenced: now,
        }
    }

    /// Normalized name for matching (lowercase, trimmed).
    pub fn normalized_name(&self) -> String {
        self.name.trim().to_lowercase()
    }

    /// All names including aliases, normalized.
    pub fn all_names_normalized(&self) -> Vec<String> {
        let mut names = vec![self.normalized_name()];
        for alias in &self.aliases {
            let n = alias.trim().to_lowercase();
            if !n.is_empty() && !names.contains(&n) {
                names.push(n);
            }
        }
        names
    }
}

// ── Edge ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: u64,
    pub from: NodeId,
    pub to: NodeId,
    pub relation_type: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub confidence: f32,
    pub source: Source,
    pub evidence: Vec<Evidence>,
}

impl Edge {
    pub fn new(
        id: u64,
        from: NodeId,
        to: NodeId,
        relation_type: String,
        confidence: f32,
        source: Source,
    ) -> Self {
        Self {
            id,
            from,
            to,
            relation_type,
            properties: HashMap::new(),
            confidence,
            source,
            evidence: Vec::new(),
        }
    }
}

// ── Ontology ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeType {
    pub name: String,
    pub description: String,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EdgeType {
    pub name: String,
    pub description: String,
    pub from_types: Vec<String>,
    pub to_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Ontology {
    pub domain: String,
    pub node_types: Vec<NodeType>,
    pub edge_types: Vec<EdgeType>,
}

impl Ontology {
    /// Merge new types into existing ontology without duplicates.
    pub fn merge(&mut self, other: &Ontology) {
        if self.domain.is_empty() {
            self.domain = other.domain.clone();
        }
        for nt in &other.node_types {
            let exists = self
                .node_types
                .iter()
                .any(|t| t.name.to_lowercase() == nt.name.to_lowercase());
            if !exists {
                self.node_types.push(nt.clone());
            }
        }
        for et in &other.edge_types {
            let exists = self
                .edge_types
                .iter()
                .any(|t| t.name.to_lowercase() == et.name.to_lowercase());
            if !exists {
                self.edge_types.push(et.clone());
            }
        }
    }

    /// Check if a node type is valid (exists in ontology).
    /// If ontology is empty, any type is valid.
    pub fn is_valid_node_type(&self, type_name: &str) -> bool {
        if self.node_types.is_empty() {
            return true;
        }
        self.node_types
            .iter()
            .any(|t| t.name.to_lowercase() == type_name.to_lowercase())
    }

    /// Check if an edge type is valid.
    /// "mentions" is always valid (system-level type for orphan connection).
    pub fn is_valid_edge_type(&self, type_name: &str) -> bool {
        let name_lower = type_name.to_lowercase();
        if name_lower == "mentions" {
            return true;
        }
        if self.edge_types.is_empty() {
            return true;
        }
        self.edge_types
            .iter()
            .any(|t| t.name.to_lowercase() == name_lower)
    }
}
