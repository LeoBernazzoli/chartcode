pub mod chunker;
pub mod context;
pub mod tier;
pub mod claude_parser;
pub mod graph;
pub mod model;
pub mod prompt;
#[cfg(feature = "python")]
pub mod python;
pub mod resolver;
pub mod storage;

// Re-export main types for convenience
pub use chunker::{chunk_text, Chunk};
pub use graph::{ExploreResult, ExtractionResult, IngestReport, KnowledgeGraph, PathResult};
pub use model::{Edge, Evidence, Node, NodeId, Ontology, Source};
pub use prompt::PromptTask;
pub use storage::{load, load_or_create, save};
