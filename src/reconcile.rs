use serde::Deserialize;

use crate::graph::KnowledgeGraph;
use crate::model::*;
use crate::tier::{relevance, ImportanceTier};

// ── Input types (from Haiku extraction JSON) ─────────────────────

#[derive(Debug, Deserialize)]
pub struct ReconcileInput {
    #[serde(default)]
    pub new_facts: Vec<NewFact>,
    #[serde(default)]
    pub superseded: Vec<SupersededEntry>,
    #[serde(default)]
    pub promotions: Vec<PromotionEntry>,
    #[serde(default)]
    pub relations: Vec<RelationEntry>,
}

#[derive(Debug, Deserialize)]
pub struct NewFact {
    pub name: String,
    #[serde(rename = "type", alias = "fact_type")]
    pub fact_type: String,
    pub tier: String,
    pub definition: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub relations: Vec<FactRelation>,
    #[serde(default)]
    pub evidence_text: String,
}

#[derive(Debug, Deserialize)]
pub struct FactRelation {
    pub target: String,
    #[serde(rename = "type")]
    pub relation_type: String,
}

#[derive(Debug, Deserialize)]
pub struct SupersededEntry {
    pub old: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct PromotionEntry {
    pub name: String,
    pub new_tier: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct RelationEntry {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub relation_type: String,
    #[serde(default)]
    pub evidence: String,
}

// ── Output ───────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct ReconcileReport {
    pub added: usize,
    pub superseded: usize,
    pub promoted: usize,
    pub gc_removed: usize,
    pub edges_added: usize,
    pub errors: Vec<String>,
}

// ── Logic ────────────────────────────────────────────────────────

fn parse_tier(s: &str) -> ImportanceTier {
    match s.to_lowercase().as_str() {
        "critical" => ImportanceTier::Critical,
        "significant" => ImportanceTier::Significant,
        _ => ImportanceTier::Minor,
    }
}

/// Reconcile extraction results with the existing KG.
/// Adds new facts, supersedes old ones, promotes confirmed ones.
pub fn reconcile(kg: &mut KnowledgeGraph, input: &ReconcileInput) -> ReconcileReport {
    let mut report = ReconcileReport {
        added: 0,
        superseded: 0,
        promoted: 0,
        gc_removed: 0,
        edges_added: 0,
        errors: vec![],
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 1. Add new facts
    for fact in &input.new_facts {
        // Check if already exists (avoid duplicates)
        if kg.lookup(&fact.name).is_some() {
            continue;
        }

        let mut node = Node::new(
            0, // will be assigned by add_node
            fact.name.clone(),
            fact.fact_type.clone(),
            fact.definition.clone(),
            0.9,
            Source::Conversation,
        );
        node.tier = parse_tier(&fact.tier);
        node.created_at = now;
        node.last_referenced = now;

        // Add reason to definition if present
        if !fact.reason.is_empty() && !fact.definition.contains(&fact.reason) {
            node.definition = format!("{} (reason: {})", fact.definition, fact.reason);
        }

        // Add evidence if present
        if !fact.evidence_text.is_empty() {
            node.evidence.push(Evidence {
                document: "conversation".to_string(),
                page: None,
                text_snippet: fact.evidence_text.clone(),
                offset_start: 0,
                offset_end: 0,
            });
        }

        match kg.add_node(node) {
            Ok(_) => report.added += 1,
            Err(e) => report.errors.push(format!("Add '{}': {:?}", fact.name, e)),
        }
    }

    // 2. Process supersessions
    for sup in &input.superseded {
        if let Some(old_node) = kg.lookup(&sup.old) {
            let old_id = old_node.id;
            // Find the new node that supersedes this one
            let new_id = input
                .new_facts
                .iter()
                .find(|f| f.supersedes.as_deref() == Some(&sup.old))
                .and_then(|f| kg.lookup(&f.name))
                .map(|n| n.id);

            if let Some(node) = kg.get_node_mut(old_id) {
                node.superseded_by = new_id.or(Some(0)); // mark as superseded
                report.superseded += 1;
            }
        }
    }

    // 3. Process promotions
    for prom in &input.promotions {
        if let Some(node) = kg.lookup(&prom.name) {
            let id = node.id;
            if let Some(node) = kg.get_node_mut(id) {
                let new_tier = parse_tier(&prom.new_tier);
                // Only promote, never demote
                if new_tier.weight() > node.tier.weight() {
                    node.tier = new_tier;
                    report.promoted += 1;
                }
            }
        }
    }

    // 4. Add relations
    for rel in &input.relations {
        let from_id = kg.lookup(&rel.from).map(|n| n.id);
        let to_id = kg.lookup(&rel.to).map(|n| n.id);

        if let (Some(from), Some(to)) = (from_id, to_id) {
            let edge = Edge::new(0, from, to, rel.relation_type.clone(), 0.8, Source::Conversation);
            match kg.add_edge(edge) {
                Ok(_) => report.edges_added += 1,
                Err(e) => report
                    .errors
                    .push(format!("Edge '{}'->'{}': {:?}", rel.from, rel.to, e)),
            }
        }
    }

    report
}

/// Remove nodes with relevance below threshold.
/// Code entities (Source::CodeAnalysis) are never removed.
/// Returns count of removed nodes.
pub fn garbage_collect(kg: &mut KnowledgeGraph, threshold: f64, now: u64) -> usize {
    let to_remove: Vec<NodeId> = kg
        .all_nodes()
        .filter(|n| {
            // Never GC code entities
            if matches!(n.source, Source::CodeAnalysis { .. }) {
                return false;
            }
            let r = relevance(
                n.tier,
                n.created_at,
                now,
                n.superseded_by.is_some(),
                false,
            );
            r < threshold
        })
        .map(|n| n.id)
        .collect();

    let count = to_remove.len();
    for id in to_remove {
        kg.remove_node(id);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconcile_adds_new_facts() {
        let mut kg = KnowledgeGraph::new();
        let input = ReconcileInput {
            new_facts: vec![NewFact {
                name: "Use LCS matching".into(),
                fact_type: "Decision".into(),
                tier: "critical".into(),
                definition: "Use LCS distance for fuzzy matching".into(),
                reason: "Best balance of precision and recall".into(),
                supersedes: None,
                relations: vec![],
                evidence_text: "we decided to use LCS".into(),
            }],
            superseded: vec![],
            promotions: vec![],
            relations: vec![],
        };

        let report = reconcile(&mut kg, &input);
        assert_eq!(report.added, 1);
        assert!(report.errors.is_empty());
        let node = kg.lookup("Use LCS matching").unwrap();
        assert_eq!(node.tier, ImportanceTier::Critical);
        assert!(node.definition.contains("reason:"));
    }

    #[test]
    fn test_reconcile_skips_duplicates() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(Node::new(
            1,
            "existing fact".into(),
            "Decision".into(),
            "Already here".into(),
            0.9,
            Source::Conversation,
        ))
        .unwrap();

        let input = ReconcileInput {
            new_facts: vec![NewFact {
                name: "existing fact".into(),
                fact_type: "Decision".into(),
                tier: "critical".into(),
                definition: "Duplicate".into(),
                reason: String::new(),
                supersedes: None,
                relations: vec![],
                evidence_text: String::new(),
            }],
            superseded: vec![],
            promotions: vec![],
            relations: vec![],
        };

        let report = reconcile(&mut kg, &input);
        assert_eq!(report.added, 0); // skipped
    }

    #[test]
    fn test_reconcile_supersedes_old_fact() {
        let mut kg = KnowledgeGraph::new();
        let mut old = Node::new(
            1,
            "Use exact match".into(),
            "Decision".into(),
            "Use exact matching only".into(),
            0.9,
            Source::Conversation,
        );
        old.tier = ImportanceTier::Significant;
        kg.add_node(old).unwrap();

        let input = ReconcileInput {
            new_facts: vec![NewFact {
                name: "Use fuzzy match".into(),
                fact_type: "Decision".into(),
                tier: "critical".into(),
                definition: "Use fuzzy matching".into(),
                reason: "Exact match missed too many".into(),
                supersedes: Some("Use exact match".into()),
                relations: vec![],
                evidence_text: String::new(),
            }],
            superseded: vec![SupersededEntry {
                old: "Use exact match".into(),
                reason: "replaced by fuzzy matching".into(),
            }],
            promotions: vec![],
            relations: vec![],
        };

        let report = reconcile(&mut kg, &input);
        assert_eq!(report.added, 1);
        assert_eq!(report.superseded, 1);
        let old_node = kg.lookup("Use exact match").unwrap();
        assert!(old_node.superseded_by.is_some());
    }

    #[test]
    fn test_reconcile_promotes_tier() {
        let mut kg = KnowledgeGraph::new();
        let mut n = Node::new(
            1,
            "overlap config".into(),
            "TechnicalFact".into(),
            "Overlap is 500 chars".into(),
            0.8,
            Source::Conversation,
        );
        n.tier = ImportanceTier::Minor;
        kg.add_node(n).unwrap();

        let input = ReconcileInput {
            new_facts: vec![],
            superseded: vec![],
            promotions: vec![PromotionEntry {
                name: "overlap config".into(),
                new_tier: "significant".into(),
                reason: "referenced 3 times".into(),
            }],
            relations: vec![],
        };

        let report = reconcile(&mut kg, &input);
        assert_eq!(report.promoted, 1);
        let node = kg.lookup("overlap config").unwrap();
        assert_eq!(node.tier, ImportanceTier::Significant);
    }

    #[test]
    fn test_reconcile_no_demotion() {
        let mut kg = KnowledgeGraph::new();
        let mut n = Node::new(
            1,
            "arch decision".into(),
            "Decision".into(),
            "Important".into(),
            0.9,
            Source::Conversation,
        );
        n.tier = ImportanceTier::Critical;
        kg.add_node(n).unwrap();

        let input = ReconcileInput {
            new_facts: vec![],
            superseded: vec![],
            promotions: vec![PromotionEntry {
                name: "arch decision".into(),
                new_tier: "minor".into(), // attempt to demote
                reason: String::new(),
            }],
            relations: vec![],
        };

        let report = reconcile(&mut kg, &input);
        assert_eq!(report.promoted, 0); // no demotion
        let node = kg.lookup("arch decision").unwrap();
        assert_eq!(node.tier, ImportanceTier::Critical); // unchanged
    }

    #[test]
    fn test_reconcile_adds_relations() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(Node::new(
            1,
            "resolver".into(),
            "Module".into(),
            "Entity resolver".into(),
            1.0,
            Source::Conversation,
        ))
        .unwrap();
        kg.add_node(Node::new(
            2,
            "graph".into(),
            "Module".into(),
            "Knowledge graph".into(),
            1.0,
            Source::Conversation,
        ))
        .unwrap();

        let input = ReconcileInput {
            new_facts: vec![],
            superseded: vec![],
            promotions: vec![],
            relations: vec![RelationEntry {
                from: "resolver".into(),
                to: "graph".into(),
                relation_type: "calls".into(),
                evidence: "merge operation".into(),
            }],
        };

        let report = reconcile(&mut kg, &input);
        assert_eq!(report.edges_added, 1);
    }

    #[test]
    fn test_gc_removes_stale_facts() {
        let mut kg = KnowledgeGraph::new();
        let mut n = Node::new(
            1,
            "old minor fact".into(),
            "TechnicalFact".into(),
            "Something trivial".into(),
            0.3,
            Source::Conversation,
        );
        n.tier = ImportanceTier::Minor;
        n.created_at = 0; // very old
        kg.add_node(n).unwrap();

        let now = 365 * 86400; // 1 year later
        let removed = garbage_collect(&mut kg, 0.05, now);
        assert_eq!(removed, 1);
        assert!(kg.lookup("old minor fact").is_none());
    }

    #[test]
    fn test_gc_preserves_critical() {
        let mut kg = KnowledgeGraph::new();
        let mut n = Node::new(
            1,
            "old critical decision".into(),
            "Decision".into(),
            "Architecture choice".into(),
            0.95,
            Source::Conversation,
        );
        n.tier = ImportanceTier::Critical;
        n.created_at = 0; // very old
        kg.add_node(n).unwrap();

        let now = 365 * 86400;
        let removed = garbage_collect(&mut kg, 0.05, now);
        assert_eq!(removed, 0); // critical never GC'd
        assert!(kg.lookup("old critical decision").is_some());
    }

    #[test]
    fn test_gc_preserves_code_entities() {
        let mut kg = KnowledgeGraph::new();
        let mut n = Node::new(
            1,
            "chunk_text".into(),
            "Function".into(),
            "Main chunking function".into(),
            1.0,
            Source::CodeAnalysis {
                file: "src/chunker.rs".into(),
            },
        );
        n.tier = ImportanceTier::Minor;
        n.created_at = 0;
        kg.add_node(n).unwrap();

        let now = 365 * 86400;
        let removed = garbage_collect(&mut kg, 0.05, now);
        assert_eq!(removed, 0); // code entities never GC'd
    }

    #[test]
    fn test_gc_removes_superseded() {
        let mut kg = KnowledgeGraph::new();
        let mut n = Node::new(
            1,
            "old approach".into(),
            "Decision".into(),
            "Use approach A".into(),
            0.9,
            Source::Conversation,
        );
        n.tier = ImportanceTier::Critical;
        n.superseded_by = Some(99);
        kg.add_node(n).unwrap();

        let now = 100 * 86400;
        let removed = garbage_collect(&mut kg, 0.05, now);
        assert_eq!(removed, 1); // superseded → relevance 0 → GC'd
    }

    #[test]
    fn test_full_reconcile_json_roundtrip() {
        let json = r#"{
            "new_facts": [
                {
                    "name": "Use MessagePack",
                    "type": "Decision",
                    "tier": "critical",
                    "definition": "Use MessagePack for storage",
                    "reason": "Compact binary format",
                    "supersedes": null,
                    "relations": [],
                    "evidence_text": "we chose messagepack"
                }
            ],
            "superseded": [],
            "promotions": [],
            "relations": []
        }"#;

        let input: ReconcileInput = serde_json::from_str(json).unwrap();
        let mut kg = KnowledgeGraph::new();
        let report = reconcile(&mut kg, &input);
        assert_eq!(report.added, 1);
    }
}
