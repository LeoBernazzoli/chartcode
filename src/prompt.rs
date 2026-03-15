use crate::graph::KnowledgeGraph;
use crate::model::Ontology;

/// A structured prompt task for the agent to process.
#[derive(Debug, Clone)]
pub struct PromptTask {
    pub prompt: String,
    pub task_type: TaskType,
}

#[derive(Debug, Clone)]
pub enum TaskType {
    AnalyzeContent,
    ExtractEntities,
    StoreMemory,
}

/// Build an ontology analysis prompt for the agent.
pub fn analyze_content(text: &str, existing_ontology: &Ontology) -> PromptTask {
    let ontology_context = if existing_ontology.node_types.is_empty() {
        "No existing ontology. Suggest entity and relation types from scratch.".to_string()
    } else {
        let node_types: Vec<String> = existing_ontology
            .node_types
            .iter()
            .map(|t| format!("  - {} ({})", t.name, t.description))
            .collect();
        let edge_types: Vec<String> = existing_ontology
            .edge_types
            .iter()
            .map(|t| {
                format!(
                    "  - {} ({}) [{} -> {}]",
                    t.name,
                    t.description,
                    t.from_types.join(", "),
                    t.to_types.join(", ")
                )
            })
            .collect();
        format!(
            "Current entity types:\n{}\n\nCurrent relation types:\n{}\n\nExtend this ontology if needed. Do not remove existing types.",
            node_types.join("\n"),
            edge_types.join("\n")
        )
    };

    let prompt = format!(
        r#"Analyze this text and suggest an ontology (entity types and relation types) appropriate for the domain.

IMPORTANT: Always include a "Person" entity type for any named individuals, researchers, authors, or historical figures mentioned in the text. This is mandatory.

{ontology_context}

Text:
{text}

Return ONLY a valid JSON object:
{{
  "domain": "detected domain name",
  "suggested_entity_types": [
    {{"name": "TypeName", "description": "what this type represents"}}
  ],
  "suggested_relation_types": [
    {{"name": "relation_name", "description": "what this relation means", "from_types": ["TypeA"], "to_types": ["TypeB"]}}
  ]
}}"#,
        ontology_context = ontology_context,
        text = truncate_text(text, 6000),
    );

    PromptTask {
        prompt,
        task_type: TaskType::AnalyzeContent,
    }
}

/// Build an entity/relation extraction prompt for the agent.
pub fn prepare_extraction(
    text: &str,
    ontology: &Ontology,
    existing_entities: &[String],
) -> PromptTask {
    let entity_types: String = ontology
        .node_types
        .iter()
        .take(15)
        .map(|t| format!("- {}: {}", t.name, t.description))
        .collect::<Vec<_>>()
        .join("\n");

    let relation_types: String = ontology
        .edge_types
        .iter()
        .take(15)
        .map(|t| {
            format!(
                "- {}: {} [{} -> {}]",
                t.name,
                t.description,
                t.from_types.join("/"),
                t.to_types.join("/")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let existing = if existing_entities.is_empty() {
        "None yet.".to_string()
    } else {
        let shown = existing_entities.iter().take(30);
        let list: Vec<String> = shown.map(|e| format!("- {}", e)).collect();
        let remainder = existing_entities.len().saturating_sub(30);
        let mut result = list.join("\n");
        if remainder > 0 {
            result.push_str(&format!("\n... and {} more entities in the graph", remainder));
        }
        result
    };

    let prompt = format!(
        r#"Extract entities and relations from this text.

Entity types (use these):
{entity_types}

Relation types (use these):
{relation_types}

Existing entities (reuse these names when referring to the same concept, do NOT create duplicates):
{existing}

Rules:
- Use only evidence from the provided text
- Every relation must have evidence_text (exact quote from the text)
- Confidence: 0.9+ explicitly defined, 0.7-0.89 discussed in detail, 0.5-0.69 mentioned
- Reuse existing entity names when referring to the same concept
- canonical_name should be a concise noun phrase (1-7 words)
- Do NOT extract generic words that are not domain-specific
- Each entity must have a definition based ONLY on what the text says about it. Do NOT confuse definitions between similar concepts
- Extract ALL named people, researchers, scientists, and historical figures as Person entities
- Relation direction matters: "A subfield_of B" means A is a subfield of B. "A uses B" means A uses B. Do NOT invert
- Do NOT create duplicate relations (same source, target, and type)
- Do NOT create self-referencing relations (source = target)

Text:
{text}

Return ONLY a valid JSON object:
{{
  "entities": [
    {{
      "name": "entity name",
      "type": "EntityType",
      "definition": "brief definition from context",
      "aliases": ["alias1"],
      "confidence": 0.85
    }}
  ],
  "relations": [
    {{
      "source": "source entity name",
      "target": "target entity name",
      "type": "relation_type",
      "confidence": 0.8,
      "evidence_text": "exact quote from text"
    }}
  ]
}}"#,
        entity_types = if entity_types.is_empty() {
            "No types defined yet. Use your best judgment.".to_string()
        } else {
            entity_types
        },
        relation_types = if relation_types.is_empty() {
            "No types defined yet. Use your best judgment.".to_string()
        } else {
            relation_types
        },
        existing = existing,
        text = truncate_text(text, 8000),
    );

    PromptTask {
        prompt,
        task_type: TaskType::ExtractEntities,
    }
}

/// Build a memory ingestion prompt for the agent.
pub fn prepare_memory(text: &str, kg: &KnowledgeGraph) -> PromptTask {
    let existing: Vec<String> = kg
        .nodes
        .values()
        .map(|n| format!("{} ({})", n.name, n.node_type))
        .take(50)
        .collect();

    let existing_str = if existing.is_empty() {
        "None yet.".to_string()
    } else {
        existing.join("\n- ")
    };

    let prompt = format!(
        r#"Extract entities and relations from this memory/fact.

Existing entities in the knowledge graph (link to these when possible):
- {existing_str}

Memory text:
{text}

Rules:
- Link to existing entities when the memory refers to them
- Create new entities only if they don't exist yet
- Keep it concise - memories are short facts

Return ONLY a valid JSON object:
{{
  "entities": [
    {{
      "name": "entity name",
      "type": "EntityType",
      "definition": "brief description",
      "aliases": [],
      "confidence": 0.9
    }}
  ],
  "relations": [
    {{
      "source": "source entity name",
      "target": "target entity name",
      "type": "relation_type",
      "confidence": 0.85,
      "evidence_text": "{text}"
    }}
  ]
}}"#,
        existing_str = existing_str,
        text = text,
    );

    PromptTask {
        prompt,
        task_type: TaskType::StoreMemory,
    }
}

fn truncate_text(text: &str, max_chars: usize) -> &str {
    if text.len() <= max_chars {
        text
    } else {
        // Find a safe char boundary
        let mut end = max_chars;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    }
}
