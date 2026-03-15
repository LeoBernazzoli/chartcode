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
    pub fn lookup(&self, name: &str) -> Option<&Node> {
        let norm = name.trim().to_lowercase();
        self.index_by_name
            .get(&norm)
            .and_then(|ids| ids.first())
            .and_then(|id| self.nodes.get(id))
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
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
            let from_id = match name_to_id.get(&relation.source) {
                Some(id) => *id,
                None => {
                    // Try lookup by name in graph
                    match self.lookup(&relation.source).map(|n| n.id) {
                        Some(id) => id,
                        None => {
                            report.errors.push(format!(
                                "relation source '{}' not found",
                                relation.source
                            ));
                            continue;
                        }
                    }
                }
            };
            let to_id = match name_to_id.get(&relation.target) {
                Some(id) => *id,
                None => {
                    match self.lookup(&relation.target).map(|n| n.id) {
                        Some(id) => id,
                        None => {
                            report.errors.push(format!(
                                "relation target '{}' not found",
                                relation.target
                            ));
                            continue;
                        }
                    }
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
}
