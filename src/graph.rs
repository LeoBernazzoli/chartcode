use crate::model::*;
use crate::resolver::EntityResolver;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("node not found: {0}")]
    NodeNotFound(String),
    #[error("invalid node type '{0}': not in ontology")]
    InvalidNodeType(String),
    #[error("invalid edge type '{0}': not in ontology")]
    InvalidEdgeType(String),
    #[error("duplicate node name: {0}")]
    DuplicateNode(String),
}

/// Result of navigating from a node.
#[derive(Debug, Clone, Serialize)]
pub struct Neighbor {
    pub node: Node,
    pub relation_type: String,
    pub direction: Direction,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Direction {
    Outgoing,
    Incoming,
}

/// Result of exploring a topic.
#[derive(Debug, Clone, Serialize)]
pub struct ExploreResult {
    pub entity: Node,
    pub relations: Vec<Neighbor>,
    pub evidence: Vec<Evidence>,
    pub related_topics: Vec<String>,
}

/// Result of ingesting extracted data.
#[derive(Debug, Clone, Serialize)]
pub struct IngestReport {
    pub added: usize,
    pub merged: usize,
    pub rejected: usize,
    pub edges_added: usize,
    pub edges_deduped: usize,
    pub errors: Vec<String>,
}

/// Path between two entities.
#[derive(Debug, Clone, Serialize)]
pub struct PathResult {
    pub found: bool,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub length: usize,
}

/// Graph quality metrics.
#[derive(Debug, Clone, Serialize)]
pub struct QualityMetrics {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub orphan_count: usize,
    pub orphan_ratio: f64,
    pub related_to_ratio: f64,
    pub avg_degree: f64,
    pub documents: usize,
}

/// Graph statistics.
#[derive(Debug, Clone, Serialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub document_count: usize,
    pub memory_count: usize,
    pub node_types: HashMap<String, usize>,
    pub edge_types: HashMap<String, usize>,
}

// ── The Knowledge Graph ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub(crate) nodes: HashMap<NodeId, Node>,
    pub(crate) edges: Vec<Edge>,
    /// Adjacency: node_id -> list of edge indices
    pub(crate) adjacency: HashMap<NodeId, Vec<usize>>,
    pub ontology: Ontology,
    /// Name index: normalized_name -> list of node IDs
    pub(crate) index_by_name: HashMap<String, Vec<NodeId>>,
    /// Type index: type_name -> list of node IDs
    pub(crate) index_by_type: HashMap<String, Vec<NodeId>>,
    pub(crate) next_node_id: u64,
    pub(crate) next_edge_id: u64,
    pub(crate) documents: Vec<String>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            adjacency: HashMap::new(),
            ontology: Ontology::default(),
            index_by_name: HashMap::new(),
            index_by_type: HashMap::new(),
            next_node_id: 1,
            next_edge_id: 1,
            documents: Vec::new(),
        }
    }

    // ── Node Operations ─────────────────────────────────────────

    /// Insert a node into the graph. Returns the node ID.
    pub fn add_node(&mut self, mut node: Node) -> Result<NodeId, GraphError> {
        if !self.ontology.is_valid_node_type(&node.node_type) {
            return Err(GraphError::InvalidNodeType(node.node_type.clone()));
        }

        node.id = self.next_node_id;
        self.next_node_id += 1;
        let id = node.id;

        // Index by name
        let norm = node.normalized_name();
        self.index_by_name
            .entry(norm)
            .or_default()
            .push(id);

        // Index aliases
        for alias in &node.aliases {
            let norm_alias = alias.trim().to_lowercase();
            if !norm_alias.is_empty() {
                self.index_by_name
                    .entry(norm_alias)
                    .or_default()
                    .push(id);
            }
        }

        // Index by type
        self.index_by_type
            .entry(node.node_type.clone())
            .or_default()
            .push(id);

        self.nodes.insert(id, node);
        Ok(id)
    }

    /// Lookup a node by name (exact or alias match, case-insensitive).
    /// Falls back to substring and fuzzy matching if exact match fails.
    pub fn lookup(&self, name: &str) -> Option<&Node> {
        let norm = name.trim().to_lowercase();

        // 1. Exact match on name or alias
        if let Some(ids) = self.index_by_name.get(&norm) {
            if let Some(id) = ids.first() {
                return self.nodes.get(id);
            }
        }

        // 2. Try common variants: singular/plural
        let variants = [
            if norm.ends_with('s') {
                norm[..norm.len()-1].to_string()
            } else {
                format!("{}s", norm)
            },
        ];
        for variant in &variants {
            if let Some(ids) = self.index_by_name.get(variant.as_str()) {
                if let Some(id) = ids.first() {
                    return self.nodes.get(id);
                }
            }
        }

        // 3. Substring match: only if the search term is long enough (>=6 chars)
        //    and the match is significant (not just matching "C" inside "Claude")
        if norm.len() >= 6 {
            for node in self.nodes.values() {
                let node_norm = node.normalized_name();
                // Search term contained in node name (e.g., "cell division" in "Cell Division Process")
                if node_norm.contains(&norm) {
                    return Some(node);
                }
                // Node name contained in search term, only if node name is >=6 chars
                // (avoids "C" matching "Claude Code")
                if node_norm.len() >= 6 && norm.contains(&node_norm) {
                    return Some(node);
                }
            }
        }

        None
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    /// Iterate over all nodes.
    pub fn all_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    /// Get next available node ID.
    pub fn next_node_id(&self) -> u64 {
        self.next_node_id
    }

    /// Remove a node and its associated edges.
    pub fn remove_node(&mut self, id: NodeId) {
        if let Some(node) = self.nodes.remove(&id) {
            // Remove from name index
            let norm = node.normalized_name();
            if let Some(ids) = self.index_by_name.get_mut(&norm) {
                ids.retain(|&nid| nid != id);
            }
            // Remove from type index
            if let Some(ids) = self.index_by_type.get_mut(&node.node_type) {
                ids.retain(|&nid| nid != id);
            }
            // Remove edges referencing this node
            let old_len = self.edges.len();
            self.edges.retain(|e| e.from != id && e.to != id);
            if self.edges.len() != old_len {
                self.rebuild_adjacency();
            }
            // Remove from adjacency
            self.adjacency.remove(&id);
        }
    }

    /// Rebuild adjacency index from edges.
    fn rebuild_adjacency(&mut self) {
        self.adjacency.clear();
        for (idx, edge) in self.edges.iter().enumerate() {
            self.adjacency.entry(edge.from).or_default().push(idx);
            self.adjacency.entry(edge.to).or_default().push(idx);
        }
    }

    /// Re-index a single file: remove old code entities, parse with tree-sitter, add new ones.
    /// Deterministic, 0 tokens, milliseconds.
    pub fn reindex_file(&mut self, file_path: &str, source_code: &str) {
        // Remove all code entities for this file
        let to_remove: Vec<NodeId> = self
            .all_nodes()
            .filter(|n| {
                matches!(&n.source, Source::CodeAnalysis { file } if file == file_path)
            })
            .map(|n| n.id)
            .collect();
        for id in to_remove {
            self.remove_node(id);
        }

        // Re-parse with tree-sitter
        let entities = crate::treesitter::parse_rust_code(source_code, file_path);

        // Add new entities
        for entity in entities {
            let mut node = Node::new(
                0,
                entity.name,
                entity.entity_type,
                entity.definition,
                1.0,
                Source::CodeAnalysis {
                    file: file_path.to_string(),
                },
            );
            node.tier = crate::tier::ImportanceTier::Minor;
            let _ = self.add_node(node);
        }
    }

    /// V2: Re-index a file extracting both definitions AND references.
    pub fn reindex_file_v2(&mut self, file_path: &str, source_code: &str) {
        // Remove old code entities for this file
        let to_remove: Vec<NodeId> = self
            .all_nodes()
            .filter(|n| {
                matches!(&n.source, Source::CodeAnalysis { file } if file == file_path)
            })
            .map(|n| n.id)
            .collect();
        for id in to_remove {
            self.remove_node(id);
        }

        // Remove old reference edges FROM this file
        self.edges.retain(|e| {
            !matches!(&e.source, Source::CodeAnalysis { file } if file == file_path)
        });
        self.rebuild_adjacency();

        // Parse with v2 (entities + references)
        let (entities, references) =
            crate::treesitter::parse_rust_code_v2(source_code, file_path);

        // Add entities as nodes
        for entity in entities {
            let mut node = Node::new(
                0,
                entity.name,
                entity.entity_type,
                entity.definition,
                1.0,
                Source::CodeAnalysis {
                    file: file_path.to_string(),
                },
            );
            node.tier = crate::tier::ImportanceTier::Minor;
            let _ = self.add_node(node);
        }

        // Create a file-level node to serve as edge source
        let file_node_name = file_path.to_string();
        let file_node_id = if let Some(n) = self.lookup(&file_node_name) {
            n.id
        } else {
            let mut fnode = Node::new(
                0,
                file_node_name,
                "File".to_string(),
                format!("Source file {}", file_path),
                1.0,
                Source::CodeAnalysis {
                    file: file_path.to_string(),
                },
            );
            fnode.tier = crate::tier::ImportanceTier::Minor;
            self.add_node(fnode).unwrap_or(0)
        };

        // Add references as edges: file → target entity
        for reference in references {
            // Try exact lookup first, then try matching "Parent.field" pattern
            let target = self.lookup(&reference.target_name).map(|n| n.id).or_else(|| {
                // For field references like "hashed_password", search for "*.hashed_password"
                let suffix = format!(".{}", reference.target_name);
                self.all_nodes()
                    .find(|n| n.name.ends_with(&suffix))
                    .map(|n| n.id)
            });
            if let Some(target_id) = target {
                if file_node_id != 0 && file_node_id != target_id {
                    let ref_type_str = match reference.ref_type {
                        crate::treesitter::RefType::Calls => "calls",
                        crate::treesitter::RefType::ReadsField => "reads",
                        crate::treesitter::RefType::WritesField => "writes",
                        crate::treesitter::RefType::UsesType => "uses_type",
                        crate::treesitter::RefType::MethodCall => "calls",
                    };
                    let edge = Edge::new(
                        0,
                        file_node_id,
                        target_id,
                        ref_type_str.to_string(),
                        1.0,
                        Source::CodeAnalysis {
                            file: file_path.to_string(),
                        },
                    );
                    let _ = self.add_edge(edge);
                }
            }
        }
    }

    /// Get all references pointing to an entity by exact name.
    pub fn references_to(&self, entity_name: &str) -> Vec<crate::treesitter::CodeReference> {
        // Use exact name match first, fallback to lookup
        let target = self
            .all_nodes()
            .find(|n| n.name == entity_name)
            .map(|n| n.id)
            .or_else(|| self.lookup(entity_name).map(|n| n.id));

        let target = match target {
            Some(id) => id,
            None => return Vec::new(),
        };

        let mut refs = Vec::new();
        for edge in &self.edges {
            if edge.to == target {
                if let Source::CodeAnalysis { ref file } = edge.source {
                    refs.push(crate::treesitter::CodeReference {
                        source_file: file.clone(),
                        source_line: 0, // edge doesn't store line, but file is enough
                        target_name: entity_name.to_string(),
                        ref_type: match edge.relation_type.as_str() {
                            "reads" => crate::treesitter::RefType::ReadsField,
                            "writes" => crate::treesitter::RefType::WritesField,
                            "uses_type" => crate::treesitter::RefType::UsesType,
                            _ => crate::treesitter::RefType::Calls,
                        },
                    });
                }
            }
        }
        refs
    }

    /// Count inbound references to an entity by name.
    pub fn inbound_reference_count(&self, entity_name: &str) -> usize {
        let target = match self.lookup(entity_name) {
            Some(n) => n.id,
            None => return 0,
        };
        self.edges.iter().filter(|e| e.to == target).count()
    }

    /// Get all nodes of a given type.
    pub fn nodes_by_type(&self, type_name: &str) -> Vec<&Node> {
        self.index_by_type
            .get(type_name)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.nodes.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    // ── Edge Operations ─────────────────────────────────────────

    /// Add an edge between two nodes. Deduplicates: if an edge with the
    /// same (from, to, relation_type) already exists, keeps the one with
    /// higher confidence and merges evidence.
    pub fn add_edge(&mut self, mut edge: Edge) -> Result<u64, GraphError> {
        if !self.nodes.contains_key(&edge.from) {
            return Err(GraphError::NodeNotFound(format!("node id {}", edge.from)));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(GraphError::NodeNotFound(format!("node id {}", edge.to)));
        }
        if !self.ontology.is_valid_edge_type(&edge.relation_type) {
            return Err(GraphError::InvalidEdgeType(edge.relation_type.clone()));
        }

        // Self-loop check
        if edge.from == edge.to {
            return Err(GraphError::NodeNotFound("self-loop not allowed".into()));
        }

        // Duplicate check: same (from, to, relation_type)
        let rel_norm = edge.relation_type.to_lowercase();
        for existing in &mut self.edges {
            if existing.from == edge.from
                && existing.to == edge.to
                && existing.relation_type.to_lowercase() == rel_norm
            {
                // Merge: keep higher confidence, merge evidence
                if edge.confidence > existing.confidence {
                    existing.confidence = edge.confidence;
                }
                for ev in &edge.evidence {
                    let already = existing.evidence.iter().any(|e| {
                        e.text_snippet == ev.text_snippet
                    });
                    if !already {
                        existing.evidence.push(ev.clone());
                    }
                }
                return Ok(existing.id);
            }
        }

        edge.id = self.next_edge_id;
        self.next_edge_id += 1;
        let id = edge.id;
        let idx = self.edges.len();

        self.adjacency.entry(edge.from).or_default().push(idx);
        self.adjacency.entry(edge.to).or_default().push(idx);

        self.edges.push(edge);
        Ok(id)
    }

    // ── Navigation ──────────────────────────────────────────────

    /// Get all neighbors of a node (both directions).
    pub fn neighbors(&self, node_id: NodeId) -> Vec<Neighbor> {
        let edge_indices = match self.adjacency.get(&node_id) {
            Some(indices) => indices,
            None => return Vec::new(),
        };

        edge_indices
            .iter()
            .filter_map(|&idx| {
                let edge = self.edges.get(idx)?;
                let (other_id, direction) = if edge.from == node_id {
                    (edge.to, Direction::Outgoing)
                } else {
                    (edge.from, Direction::Incoming)
                };
                let other_node = self.nodes.get(&other_id)?;
                Some(Neighbor {
                    node: other_node.clone(),
                    relation_type: edge.relation_type.clone(),
                    direction,
                    confidence: edge.confidence,
                    evidence: edge.evidence.clone(),
                })
            })
            .collect()
    }

    /// Get neighbors filtered by type.
    pub fn neighbors_by_type(&self, node_id: NodeId, type_filter: &str) -> Vec<Neighbor> {
        self.neighbors(node_id)
            .into_iter()
            .filter(|n| n.node.node_type.to_lowercase() == type_filter.to_lowercase())
            .collect()
    }

    /// Follow a specific relation type from a node.
    pub fn follow(&self, node_id: NodeId, relation_type: &str) -> Vec<Neighbor> {
        self.neighbors(node_id)
            .into_iter()
            .filter(|n| {
                n.relation_type.to_lowercase() == relation_type.to_lowercase()
                    && n.direction == Direction::Outgoing
            })
            .collect()
    }

    /// Find shortest path between two nodes (BFS).
    pub fn path(&self, from_name: &str, to_name: &str) -> PathResult {
        let from_node = match self.lookup(from_name) {
            Some(n) => n,
            None => {
                return PathResult {
                    found: false,
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    length: 0,
                }
            }
        };
        let to_node = match self.lookup(to_name) {
            Some(n) => n,
            None => {
                return PathResult {
                    found: false,
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    length: 0,
                }
            }
        };

        let from_id = from_node.id;
        let to_id = to_node.id;

        if from_id == to_id {
            return PathResult {
                found: true,
                nodes: vec![from_node.clone()],
                edges: Vec::new(),
                length: 0,
            };
        }

        // BFS
        let mut visited: HashMap<NodeId, (NodeId, usize)> = HashMap::new(); // node -> (parent, edge_idx)
        let mut queue: std::collections::VecDeque<NodeId> = std::collections::VecDeque::new();
        visited.insert(from_id, (from_id, usize::MAX));
        queue.push_back(from_id);

        while let Some(current) = queue.pop_front() {
            if current == to_id {
                break;
            }
            if let Some(edge_indices) = self.adjacency.get(&current) {
                for &idx in edge_indices {
                    if let Some(edge) = self.edges.get(idx) {
                        let next = if edge.from == current {
                            edge.to
                        } else {
                            edge.from
                        };
                        if !visited.contains_key(&next) {
                            visited.insert(next, (current, idx));
                            queue.push_back(next);
                        }
                    }
                }
            }
        }

        if !visited.contains_key(&to_id) {
            return PathResult {
                found: false,
                nodes: Vec::new(),
                edges: Vec::new(),
                length: 0,
            };
        }

        // Reconstruct path
        let mut path_nodes: Vec<Node> = Vec::new();
        let mut path_edges: Vec<Edge> = Vec::new();
        let mut current = to_id;
        while current != from_id {
            path_nodes.push(self.nodes[&current].clone());
            let (parent, edge_idx) = visited[&current];
            if edge_idx != usize::MAX {
                path_edges.push(self.edges[edge_idx].clone());
            }
            current = parent;
        }
        path_nodes.push(self.nodes[&from_id].clone());
        path_nodes.reverse();
        path_edges.reverse();
        let length = path_edges.len();

        PathResult {
            found: true,
            nodes: path_nodes,
            edges: path_edges,
            length,
        }
    }

    /// Explore a topic: get entity + all connections + evidence.
    pub fn explore(&self, name: &str) -> Option<ExploreResult> {
        let node = self.lookup(name)?;
        let relations = self.neighbors(node.id);
        let evidence = node.evidence.clone();
        let related_topics: Vec<String> = relations
            .iter()
            .map(|n| n.node.name.clone())
            .collect();

        Some(ExploreResult {
            entity: node.clone(),
            relations,
            evidence,
            related_topics,
        })
    }

    // ── Ingestion ───────────────────────────────────────────────

    /// Resolve an entity name: try exact map, then graph lookup, then fuzzy.
    fn resolve_entity_name(
        &self,
        name: &str,
        name_to_id: &HashMap<String, NodeId>,
    ) -> Option<NodeId> {
        // 1. Exact match in current extraction batch
        if let Some(id) = name_to_id.get(name) {
            return Some(*id);
        }
        // 2. Lookup by name/alias in graph
        if let Some(node) = self.lookup(name) {
            return Some(node.id);
        }
        // 3. Fuzzy match against all nodes
        let resolver = EntityResolver::new(0.85);
        let existing: Vec<&Node> = self.nodes.values().collect();
        resolver.resolve(name, &existing)
    }

    /// Ingest extracted entities and relations from an agent.
    /// Handles entity resolution, dedup, and validation.
    pub fn ingest(&mut self, extraction: &ExtractionResult) -> IngestReport {
        let resolver = EntityResolver::new(0.85);
        let mut report = IngestReport {
            added: 0,
            merged: 0,
            rejected: 0,
            edges_added: 0,
            edges_deduped: 0,
            errors: Vec::new(),
        };

        // Reject entities with empty names
        let valid_entities: Vec<&ExtractedEntity> = extraction
            .entities
            .iter()
            .filter(|e| !e.name.trim().is_empty())
            .collect();

        // Map from extraction name -> resolved node ID
        let mut name_to_id: HashMap<String, NodeId> = HashMap::new();

        // Process entities
        for entity in &valid_entities {
            let existing_nodes: Vec<&Node> = self.nodes.values().collect();

            match resolver.resolve(&entity.name, &existing_nodes) {
                Some(existing_id) => {
                    // Merge: keep definition with higher confidence, add aliases
                    if let Some(node) = self.nodes.get_mut(&existing_id) {
                        // Pick definition: prefer higher confidence, then longer
                        if entity.confidence > node.confidence
                            || (entity.confidence == node.confidence
                                && entity.definition.len() > node.definition.len())
                        {
                            node.definition = entity.definition.clone();
                        }
                        // Update confidence to max
                        if entity.confidence > node.confidence {
                            node.confidence = entity.confidence;
                        }
                        for alias in &entity.aliases {
                            let norm = alias.trim().to_lowercase();
                            if !norm.is_empty()
                                && !node.all_names_normalized().contains(&norm)
                            {
                                node.aliases.push(alias.clone());
                                self.index_by_name
                                    .entry(norm)
                                    .or_default()
                                    .push(existing_id);
                            }
                        }
                        node.evidence.extend(entity.evidence.clone());
                    }
                    name_to_id.insert(entity.name.clone(), existing_id);
                    report.merged += 1;
                }
                None => {
                    // New entity
                    let mut new_node = Node::new(
                        0, // will be assigned in add_node
                        entity.name.clone(),
                        entity.entity_type.clone(),
                        entity.definition.clone(),
                        entity.confidence,
                        entity.source.clone(),
                    );
                    new_node.aliases = entity.aliases.clone();
                    new_node.evidence = entity.evidence.clone();

                    match self.add_node(new_node) {
                        Ok(id) => {
                            name_to_id.insert(entity.name.clone(), id);
                            report.added += 1;
                        }
                        Err(e) => {
                            report.errors.push(format!("entity '{}': {}", entity.name, e));
                            report.rejected += 1;
                        }
                    }
                }
            }
        }

        // Process relations
        for relation in &extraction.relations {
            // Skip self-referencing relations by name
            if relation.source.trim().to_lowercase() == relation.target.trim().to_lowercase() {
                report.edges_deduped += 1;
                continue;
            }

            let from_id = match self.resolve_entity_name(&relation.source, &name_to_id) {
                Some(id) => id,
                None => {
                    report.errors.push(format!(
                        "relation source '{}' not found",
                        relation.source
                    ));
                    continue;
                }
            };
            let to_id = match self.resolve_entity_name(&relation.target, &name_to_id) {
                Some(id) => id,
                None => {
                    report.errors.push(format!(
                        "relation target '{}' not found",
                        relation.target
                    ));
                    continue;
                }
            };

            let mut edge = Edge::new(
                0,
                from_id,
                to_id,
                relation.relation_type.clone(),
                relation.confidence,
                relation.source_ref.clone(),
            );
            edge.evidence = relation.evidence.clone();

            let edges_before = self.edges.len();
            match self.add_edge(edge) {
                Ok(_) => {
                    if self.edges.len() > edges_before {
                        report.edges_added += 1;
                    } else {
                        report.edges_deduped += 1;
                    }
                }
                Err(e) => {
                    report
                        .errors
                        .push(format!("edge '{}'->'{}': {}", relation.source, relation.target, e));
                }
            }
        }

        report
    }

    // ── Graph Maintenance ────────────────────────────────────────

    /// Connect orphan nodes bidirectionally:
    /// 1. Scan orphan definitions for mentions of other entities
    /// 2. Scan other entities' definitions for mentions of orphan names
    /// Creates "mentions" edges. Returns the number of new connections.
    pub fn connect_orphans(&mut self) -> usize {
        let mut connected: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        for edge in &self.edges {
            connected.insert(edge.from);
            connected.insert(edge.to);
        }
        let orphan_ids: Vec<NodeId> = self.nodes.keys()
            .filter(|id| !connected.contains(id))
            .copied()
            .collect();

        if orphan_ids.is_empty() {
            return 0;
        }

        // Collect all node info
        let all_nodes: Vec<(NodeId, String, String)> = self.nodes.iter()
            .map(|(id, n)| (*id, n.name.to_lowercase(), n.definition.to_lowercase()))
            .collect();

        let mut new_edges: Vec<(NodeId, NodeId)> = Vec::new();

        for &orphan_id in &orphan_ids {
            let orphan = match self.nodes.get(&orphan_id) {
                Some(n) => n,
                None => continue,
            };
            let orphan_name_lower = orphan.name.to_lowercase();
            let orphan_def_lower = orphan.definition.to_lowercase();

            for (other_id, other_name, other_def) in &all_nodes {
                if *other_id == orphan_id {
                    continue;
                }

                // Direction 1: orphan definition mentions other entity
                if other_name.len() >= 4 && !orphan_def_lower.is_empty()
                    && orphan_def_lower.contains(other_name.as_str())
                {
                    new_edges.push((orphan_id, *other_id));
                    continue; // one connection per pair is enough
                }

                // Direction 2: other entity's definition mentions orphan name
                if orphan_name_lower.len() >= 4 && !other_def.is_empty()
                    && other_def.contains(orphan_name_lower.as_str())
                {
                    new_edges.push((*other_id, orphan_id));
                }
            }
        }

        let mut count = 0;
        for (from, to) in new_edges {
            let source = self.nodes.get(&from)
                .map(|n| n.source.clone())
                .unwrap_or(Source::Inferred);
            let edge = Edge::new(0, from, to, "mentions".to_string(), 0.5, source);
            if self.add_edge(edge).is_ok() {
                count += 1;
            }
        }
        count
    }

    /// Discover implicit connections by scanning ALL node definitions
    /// for mentions of other entities. Creates "mentions" edges with
    /// confidence 0.5. Only creates edges that don't already exist.
    /// Returns the number of new connections.
    pub fn discover_connections(&mut self) -> usize {
        // Collect all entity names with their IDs (min 4 chars to avoid noise)
        let name_entries: Vec<(String, NodeId)> = self.nodes.iter()
            .filter(|(_, n)| n.name.len() >= 4)
            .map(|(id, n)| (n.name.to_lowercase(), *id))
            .collect();

        let mut new_edges: Vec<(NodeId, NodeId)> = Vec::new();

        for (node_id, node) in &self.nodes {
            let def_lower = node.definition.to_lowercase();
            if def_lower.len() < 10 {
                continue;
            }

            for (name_lower, target_id) in &name_entries {
                if target_id == node_id {
                    continue;
                }
                // Check if name appears as a whole word in the definition
                if def_lower.contains(name_lower.as_str()) {
                    // Verify it's not already connected
                    let already = self.edges.iter().any(|e| {
                        (e.from == *node_id && e.to == *target_id)
                            || (e.from == *target_id && e.to == *node_id)
                    });
                    if !already {
                        new_edges.push((*node_id, *target_id));
                    }
                }
            }
        }

        let mut count = 0;
        for (from, to) in new_edges {
            let source = self.nodes.get(&from)
                .map(|n| n.source.clone())
                .unwrap_or(Source::Inferred);
            let edge = Edge::new(0, from, to, "mentions".to_string(), 0.5, source);
            if self.add_edge(edge).is_ok() {
                count += 1;
            }
        }
        count
    }

    /// Get graph quality metrics.
    pub fn quality_metrics(&self) -> QualityMetrics {
        let total_nodes = self.nodes.len();
        let mut connected: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        for edge in &self.edges {
            connected.insert(edge.from);
            connected.insert(edge.to);
        }
        let orphan_count = total_nodes - connected.len();

        let related_to_count = self.edges.iter()
            .filter(|e| e.relation_type.to_lowercase() == "related_to")
            .count();
        let total_edges = self.edges.len();

        let avg_degree = if total_nodes > 0 {
            (total_edges as f64 * 2.0) / total_nodes as f64
        } else {
            0.0
        };

        QualityMetrics {
            total_nodes,
            total_edges,
            orphan_count,
            orphan_ratio: if total_nodes > 0 { orphan_count as f64 / total_nodes as f64 } else { 0.0 },
            related_to_ratio: if total_edges > 0 { related_to_count as f64 / total_edges as f64 } else { 0.0 },
            avg_degree,
            documents: self.documents.len(),
        }
    }

    // ── Stats ───────────────────────────────────────────────────

    pub fn stats(&self) -> GraphStats {
        let mut node_types: HashMap<String, usize> = HashMap::new();
        for node in self.nodes.values() {
            *node_types.entry(node.node_type.clone()).or_default() += 1;
        }

        let mut edge_types: HashMap<String, usize> = HashMap::new();
        for edge in &self.edges {
            *edge_types.entry(edge.relation_type.clone()).or_default() += 1;
        }

        let memory_count = self
            .nodes
            .values()
            .filter(|n| matches!(n.source, Source::Memory))
            .count();

        GraphStats {
            node_count: self.nodes.len(),
            edge_count: self.edges.len(),
            document_count: self.documents.len(),
            memory_count,
            node_types,
            edge_types,
        }
    }

    /// Get the most recently added nodes.
    pub fn recent(&self, limit: usize) -> Vec<&Node> {
        let mut nodes: Vec<&Node> = self.nodes.values().collect();
        nodes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        nodes.truncate(limit);
        nodes
    }

    /// Get main topics: node types with their top entities.
    pub fn topics(&self) -> HashMap<String, Vec<String>> {
        let mut topics: HashMap<String, Vec<String>> = HashMap::new();
        for node in self.nodes.values() {
            topics
                .entry(node.node_type.clone())
                .or_default()
                .push(node.name.clone());
        }
        topics
    }

    /// Register a document as indexed.
    pub fn add_document(&mut self, name: &str) {
        if !self.documents.contains(&name.to_string()) {
            self.documents.push(name.to_string());
        }
    }
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ── Extraction Result (from agent) ──────────────────────────────

/// What the agent returns after processing an extraction prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub name: String,
    pub entity_type: String,
    pub definition: String,
    pub aliases: Vec<String>,
    pub confidence: f32,
    pub source: Source,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelation {
    pub source: String,
    pub target: String,
    pub relation_type: String,
    pub confidence: f32,
    pub source_ref: Source,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_graph() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();

        let n1 = Node::new(0, "Marco Bianchi".into(), "person".into(), "Project manager".into(), 0.9, Source::Document { name: "report.pdf".into(), page: Some(1) });
        let n2 = Node::new(0, "Project Alpha".into(), "project".into(), "Main project".into(), 0.85, Source::Document { name: "report.pdf".into(), page: Some(2) });
        let n3 = Node::new(0, "Budget Q3".into(), "metric".into(), "Q3 budget allocation".into(), 0.8, Source::Document { name: "finance.pdf".into(), page: Some(5) });

        let id1 = kg.add_node(n1).unwrap();
        let id2 = kg.add_node(n2).unwrap();
        let id3 = kg.add_node(n3).unwrap();

        kg.add_edge(Edge::new(0, id1, id2, "works_on".into(), 0.9, Source::Document { name: "report.pdf".into(), page: Some(1) })).unwrap();
        kg.add_edge(Edge::new(0, id2, id3, "has_budget".into(), 0.85, Source::Document { name: "finance.pdf".into(), page: Some(5) })).unwrap();

        kg
    }

    #[test]
    fn test_lookup() {
        let kg = sample_graph();
        assert!(kg.lookup("Marco Bianchi").is_some());
        assert!(kg.lookup("marco bianchi").is_some());
        assert!(kg.lookup("Unknown Person").is_none());
    }

    #[test]
    fn test_lookup_plural_singular() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(Node::new(0, "Neural Networks".into(), "concept".into(),
            "test".into(), 0.9, Source::Memory)).unwrap();

        // Singular matches plural
        assert!(kg.lookup("neural network").is_some());
        // Plural matches directly
        assert!(kg.lookup("neural networks").is_some());
    }

    #[test]
    fn test_lookup_substring() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(Node::new(0, "Cell Division Process".into(), "concept".into(),
            "test".into(), 0.9, Source::Memory)).unwrap();

        // Substring match
        assert!(kg.lookup("cell division").is_some());
    }

    #[test]
    fn test_neighbors() {
        let kg = sample_graph();
        let marco = kg.lookup("Marco Bianchi").unwrap();
        let neighbors = kg.neighbors(marco.id);
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].node.name, "Project Alpha");
    }

    #[test]
    fn test_follow() {
        let kg = sample_graph();
        let marco = kg.lookup("Marco Bianchi").unwrap();
        let projects = kg.follow(marco.id, "works_on");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].node.name, "Project Alpha");
    }

    #[test]
    fn test_path() {
        let kg = sample_graph();
        let path = kg.path("Marco Bianchi", "Budget Q3");
        assert!(path.found);
        assert_eq!(path.length, 2);
        assert_eq!(path.nodes.len(), 3);
        assert_eq!(path.nodes[0].name, "Marco Bianchi");
        assert_eq!(path.nodes[1].name, "Project Alpha");
        assert_eq!(path.nodes[2].name, "Budget Q3");
    }

    #[test]
    fn test_path_not_found() {
        let mut kg = sample_graph();
        let n4 = Node::new(0, "Isolated Node".into(), "concept".into(), "Not connected".into(), 0.5, Source::Memory);
        kg.add_node(n4).unwrap();
        let path = kg.path("Marco Bianchi", "Isolated Node");
        assert!(!path.found);
    }

    #[test]
    fn test_explore() {
        let kg = sample_graph();
        let result = kg.explore("Project Alpha").unwrap();
        assert_eq!(result.entity.name, "Project Alpha");
        assert_eq!(result.relations.len(), 2); // Marco + Budget
    }

    #[test]
    fn test_ingest_with_dedup() {
        let mut kg = sample_graph();

        let extraction = ExtractionResult {
            entities: vec![
                ExtractedEntity {
                    name: "Marco Bianchi".into(), // should merge
                    entity_type: "person".into(),
                    definition: "Senior project manager at ACME Corp with 10 years experience".into(),
                    aliases: vec!["M. Bianchi".into()],
                    confidence: 0.95,
                    source: Source::Document { name: "org.pdf".into(), page: Some(1) },
                    evidence: vec![],
                },
                ExtractedEntity {
                    name: "Sara Verdi".into(), // should add
                    entity_type: "person".into(),
                    definition: "Lead developer".into(),
                    aliases: vec![],
                    confidence: 0.9,
                    source: Source::Document { name: "org.pdf".into(), page: Some(2) },
                    evidence: vec![],
                },
            ],
            relations: vec![
                ExtractedRelation {
                    source: "Sara Verdi".into(),
                    target: "Project Alpha".into(),
                    relation_type: "works_on".into(),
                    confidence: 0.85,
                    source_ref: Source::Document { name: "org.pdf".into(), page: Some(2) },
                    evidence: vec![],
                },
            ],
        };

        let report = kg.ingest(&extraction);
        assert_eq!(report.merged, 1); // Marco merged
        assert_eq!(report.added, 1); // Sara added
        assert_eq!(report.edges_added, 1); // Sara->Project Alpha

        // Check Marco's definition was updated (longer one wins)
        let marco = kg.lookup("Marco Bianchi").unwrap();
        assert!(marco.definition.contains("Senior project manager"));

        // Check alias was added
        assert!(kg.lookup("M. Bianchi").is_some());
    }

    #[test]
    fn test_stats() {
        let kg = sample_graph();
        let stats = kg.stats();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 2);
    }

    #[test]
    fn test_edge_dedup() {
        let mut kg = sample_graph();
        let marco = kg.lookup("Marco Bianchi").unwrap().id;
        let alpha = kg.lookup("Project Alpha").unwrap().id;

        // Try adding duplicate edge (same from, to, relation_type)
        let dup = Edge::new(0, marco, alpha, "works_on".into(), 0.95,
            Source::Document { name: "new.pdf".into(), page: Some(3) });
        let result = kg.add_edge(dup);
        assert!(result.is_ok());
        // Edge count should NOT increase (deduped)
        assert_eq!(kg.edges.len(), 2);
    }

    #[test]
    fn test_self_loop_rejected() {
        let mut kg = sample_graph();
        let marco = kg.lookup("Marco Bianchi").unwrap().id;

        let self_edge = Edge::new(0, marco, marco, "related_to".into(), 0.5, Source::Memory);
        assert!(kg.add_edge(self_edge).is_err());
    }

    #[test]
    fn test_ingest_edge_dedup_report() {
        let mut kg = sample_graph();

        let extraction = ExtractionResult {
            entities: vec![],
            relations: vec![
                ExtractedRelation {
                    source: "Marco Bianchi".into(),
                    target: "Project Alpha".into(),
                    relation_type: "works_on".into(),
                    confidence: 0.9,
                    source_ref: Source::Memory,
                    evidence: vec![],
                },
                ExtractedRelation {
                    source: "Marco Bianchi".into(),
                    target: "Project Alpha".into(),
                    relation_type: "works_on".into(),
                    confidence: 0.8,
                    source_ref: Source::Memory,
                    evidence: vec![],
                },
            ],
        };

        let report = kg.ingest(&extraction);
        assert_eq!(report.edges_added, 0); // both deduped (original already exists)
        assert_eq!(report.edges_deduped, 2);
    }

    #[test]
    fn test_definition_merge_by_confidence() {
        let mut kg = KnowledgeGraph::new();

        let n1 = Node::new(0, "AI".into(), "concept".into(),
            "Short def".into(), 0.7, Source::Memory);
        kg.add_node(n1).unwrap();

        let extraction = ExtractionResult {
            entities: vec![
                ExtractedEntity {
                    name: "AI".into(),
                    entity_type: "concept".into(),
                    definition: "Better definition from higher confidence source".into(),
                    aliases: vec![],
                    confidence: 0.95,
                    source: Source::Memory,
                    evidence: vec![],
                },
            ],
            relations: vec![],
        };

        let report = kg.ingest(&extraction);
        assert_eq!(report.merged, 1);

        let ai = kg.lookup("AI").unwrap();
        assert!(ai.definition.contains("Better definition"));
        assert_eq!(ai.confidence, 0.95);
    }

    #[test]
    fn test_reindex_file() {
        let mut kg = KnowledgeGraph::new();

        // Initial index
        kg.reindex_file("src/test.rs", "fn old_function() -> bool { true }");
        assert!(kg.lookup("old_function").is_some());

        // Reindex with new code — old entities should be gone
        kg.reindex_file("src/test.rs", "fn new_function() -> String { String::new() }");
        assert!(kg.lookup("old_function").is_none());
        assert!(kg.lookup("new_function").is_some());
    }

    #[test]
    fn test_reindex_doesnt_affect_other_files() {
        let mut kg = KnowledgeGraph::new();

        kg.reindex_file("src/a.rs", "fn alpha() {}");
        kg.reindex_file("src/b.rs", "fn bravo() {}");
        assert!(kg.lookup("alpha").is_some());
        assert!(kg.lookup("bravo").is_some());

        // Reindex only a.rs with completely different function
        kg.reindex_file("src/a.rs", "fn charlie() {}");
        assert!(kg.lookup("charlie").is_some());
        assert!(kg.lookup("bravo").is_some()); // untouched

        // alpha should be gone from nodes (even if stale index entry remains)
        let alpha_exists = kg.all_nodes().any(|n| n.name == "alpha");
        assert!(!alpha_exists);
    }

    // ── v2 reference edge tests ─────────────────────────

    #[test]
    fn test_reindex_v2_creates_reference_edges() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/model.rs", "pub struct Node { pub confidence: f32 }");
        kg.reindex_file_v2(
            "src/graph.rs",
            r#"
fn relevance(node: &Node) {
    let c = node.confidence;
}
"#,
        );
        // Should have reference edges
        assert!(kg.stats().edge_count > 0, "Should have reference edges");
    }

    #[test]
    fn test_inbound_reference_count() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/model.rs", "pub struct Node { pub id: u64 }");
        kg.reindex_file_v2("src/a.rs", "fn a(n: Node) {}");
        kg.reindex_file_v2("src/b.rs", "fn b(n: Node) {}");
        kg.reindex_file_v2("src/c.rs", "fn c(n: Node) {}");

        let count = kg.inbound_reference_count("Node");
        assert!(count >= 3, "Expected >=3 refs to Node, got {}", count);
    }

    #[test]
    fn test_references_to() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/chunker.rs", "pub fn chunk_text(text: &str) {}");
        kg.reindex_file_v2("src/boot.rs", "fn boot() { chunk_text(\"x\"); }");
        kg.reindex_file_v2("src/graph.rs", "fn proc() { chunk_text(\"y\"); }");

        let refs = kg.references_to("chunk_text");
        assert!(
            refs.len() >= 2,
            "chunk_text should have >=2 refs, got {}",
            refs.len()
        );
    }

    #[test]
    fn test_reindex_v2_cleans_old_edges() {
        let mut kg = KnowledgeGraph::new();
        kg.reindex_file_v2("src/model.rs", "pub fn target() {}");
        kg.reindex_file_v2("src/a.rs", "fn a() { target(); }");

        let before = kg.inbound_reference_count("target");
        assert!(before >= 1);

        // Reindex a.rs without the call
        kg.reindex_file_v2("src/a.rs", "fn a() { }");
        let after = kg.inbound_reference_count("target");
        assert!(after < before, "Old edge should be removed after reindex");
    }
}
