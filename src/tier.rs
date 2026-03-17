use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportanceTier {
    Critical,
    Significant,
    Minor,
}

impl ImportanceTier {
    pub fn weight(&self) -> f64 {
        match self {
            ImportanceTier::Critical => 1.0,
            ImportanceTier::Significant => 0.6,
            ImportanceTier::Minor => 0.3,
        }
    }
}

impl Default for ImportanceTier {
    fn default() -> Self {
        ImportanceTier::Minor
    }
}

/// Calculate relevance score for a node.
/// `created_at` and `now` are Unix timestamps in seconds.
/// `superseded` — if true, relevance is 0.
/// `is_code_entity` — if true, no decay (code entities are refreshed on file change).
pub fn relevance(
    tier: ImportanceTier,
    created_at: u64,
    now: u64,
    superseded: bool,
    is_code_entity: bool,
) -> f64 {
    if superseded {
        return 0.0;
    }

    // Code entities never decay — they are refreshed by tree-sitter on every file change
    if is_code_entity {
        return tier.weight();
    }

    let age_days = (now.saturating_sub(created_at)) as f64 / 86400.0;

    match tier {
        ImportanceTier::Critical => 1.0,
        ImportanceTier::Significant => 0.6 * (-0.01 * age_days).exp(),
        ImportanceTier::Minor => 0.3 * (-0.05 * age_days).exp(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_weights() {
        assert_eq!(ImportanceTier::Critical.weight(), 1.0);
        assert_eq!(ImportanceTier::Significant.weight(), 0.6);
        assert_eq!(ImportanceTier::Minor.weight(), 0.3);
    }

    #[test]
    fn test_relevance_critical_no_decay() {
        let now = 1000 * 86400;
        let created = 0;
        let r = relevance(ImportanceTier::Critical, created, now, false, false);
        assert_eq!(r, 1.0);
    }

    #[test]
    fn test_relevance_significant_decays() {
        let now = 70 * 86400;
        let created = 0;
        let r = relevance(ImportanceTier::Significant, created, now, false, false);
        // 0.6 * e^(-0.01 * 70) ≈ 0.298
        assert!(r > 0.28 && r < 0.31, "got {}", r);
    }

    #[test]
    fn test_relevance_minor_decays_fast() {
        let now = 14 * 86400;
        let created = 0;
        let r = relevance(ImportanceTier::Minor, created, now, false, false);
        // 0.3 * e^(-0.05 * 14) ≈ 0.149
        assert!(r > 0.14 && r < 0.16, "got {}", r);
    }

    #[test]
    fn test_relevance_superseded_is_zero() {
        let r = relevance(ImportanceTier::Critical, 0, 100 * 86400, true, false);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn test_relevance_code_entity_no_decay() {
        let now = 365 * 86400;
        let r = relevance(ImportanceTier::Minor, 0, now, false, true);
        assert_eq!(r, 0.3); // weight only, no decay
    }

    #[test]
    fn test_tier_serialization() {
        let tier = ImportanceTier::Significant;
        let json = serde_json::to_string(&tier).unwrap();
        let back: ImportanceTier = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tier);
    }

    #[test]
    fn test_default_is_minor() {
        assert_eq!(ImportanceTier::default(), ImportanceTier::Minor);
    }
}
