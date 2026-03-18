use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct GraphocodeConfig {
    #[serde(default)]
    pub sources: SourcesConfig,
    #[serde(default)]
    pub bootstrap: BootstrapConfig,
    #[serde(default)]
    pub extraction: ExtractionConfig,
    #[serde(default)]
    pub impact: ImpactConfig,
}

#[derive(Debug, Deserialize)]
pub struct SourcesConfig {
    #[serde(default = "default_code_patterns")]
    pub code: Vec<String>,
    #[serde(default = "default_true")]
    pub conversations: bool,
    #[serde(default)]
    pub documents: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct BootstrapConfig {
    #[serde(default = "default_true")]
    pub on_first_session: bool,
    #[serde(default = "default_snapshot_every")]
    pub snapshot_every: u64,
}

#[derive(Debug, Deserialize)]
pub struct ExtractionConfig {
    #[serde(default = "default_threshold")]
    pub threshold: u64,
    #[serde(default = "default_budget")]
    pub budget: usize,
    #[serde(default = "default_model")]
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct ImpactConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_depth")]
    pub depth: usize,
}

fn default_code_patterns() -> Vec<String> {
    vec!["**/*.rs".into()]
}
fn default_true() -> bool {
    true
}
fn default_threshold() -> u64 {
    85
}
fn default_budget() -> usize {
    2000
}
fn default_model() -> String {
    "haiku".into()
}
fn default_depth() -> usize {
    2
}
fn default_snapshot_every() -> u64 {
    20
}

impl Default for GraphocodeConfig {
    fn default() -> Self {
        Self {
            sources: SourcesConfig::default(),
            bootstrap: BootstrapConfig::default(),
            extraction: ExtractionConfig::default(),
            impact: ImpactConfig::default(),
        }
    }
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            code: default_code_patterns(),
            conversations: true,
            documents: vec![],
        }
    }
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            on_first_session: true,
            snapshot_every: default_snapshot_every(),
        }
    }
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            threshold: default_threshold(),
            budget: default_budget(),
            model: default_model(),
        }
    }
}

impl Default for ImpactConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            depth: default_depth(),
        }
    }
}

/// Load config from file, or return defaults if file doesn't exist.
pub fn load_config(path: &Path) -> GraphocodeConfig {
    if path.exists() {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        toml::from_str(&content).unwrap_or_default()
    } else {
        GraphocodeConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let toml_str = r#"
[sources]
code = ["src/**/*.rs", "lib/**/*.rs"]
conversations = true
documents = ["docs/spec.md"]

[bootstrap]
on_first_session = true
snapshot_every = 30

[extraction]
threshold = 90
budget = 3000
model = "sonnet"

[impact]
enabled = false
depth = 3
"#;
        let config: GraphocodeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.sources.code, vec!["src/**/*.rs", "lib/**/*.rs"]);
        assert!(config.sources.conversations);
        assert_eq!(config.sources.documents, vec!["docs/spec.md"]);
        assert!(config.bootstrap.on_first_session);
        assert_eq!(config.bootstrap.snapshot_every, 30);
        assert_eq!(config.extraction.threshold, 90);
        assert_eq!(config.extraction.budget, 3000);
        assert_eq!(config.extraction.model, "sonnet");
        assert!(!config.impact.enabled);
        assert_eq!(config.impact.depth, 3);
    }

    #[test]
    fn test_default_config() {
        let config = GraphocodeConfig::default();
        assert_eq!(config.sources.code, vec!["**/*.rs"]);
        assert!(config.sources.conversations);
        assert!(config.sources.documents.is_empty());
        assert_eq!(config.extraction.threshold, 85);
        assert_eq!(config.extraction.budget, 2000);
        assert_eq!(config.extraction.model, "haiku");
        assert!(config.impact.enabled);
        assert_eq!(config.impact.depth, 2);
    }

    #[test]
    fn test_partial_config() {
        let toml_str = r#"
[sources]
code = ["**/*.py"]
"#;
        let config: GraphocodeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.sources.code, vec!["**/*.py"]);
        // Everything else should be defaults
        assert!(config.sources.conversations);
        assert_eq!(config.extraction.threshold, 85);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let config = load_config(Path::new("/nonexistent/graphocode.toml"));
        assert_eq!(config.extraction.threshold, 85); // defaults
    }

    #[test]
    fn test_empty_config_file() {
        let config: GraphocodeConfig = toml::from_str("").unwrap();
        assert_eq!(config.sources.code, vec!["**/*.rs"]); // defaults
    }
}
