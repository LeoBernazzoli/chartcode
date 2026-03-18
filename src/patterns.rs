use crate::treesitter::{CodeReference, RefType};

#[derive(Debug, Clone)]
pub struct ReferencePattern {
    pub pattern: String,
    pub count: usize,
    pub example_files: Vec<String>,
    pub total_files: usize,
}

/// Group references by type and produce compact patterns.
/// 10K references → 3-5 pattern lines.
pub fn group_by_pattern(refs: &[CodeReference]) -> Vec<ReferencePattern> {
    if refs.is_empty() {
        return Vec::new();
    }

    let mut groups: std::collections::HashMap<String, Vec<&CodeReference>> =
        std::collections::HashMap::new();

    for r in refs {
        let key = match r.ref_type {
            RefType::Calls => format!("calls {}", r.target_name),
            RefType::MethodCall => format!("calls .{}()", r.target_name),
            RefType::ReadsField => format!("reads .{}", r.target_name),
            RefType::WritesField => format!("writes .{}", r.target_name),
            RefType::UsesType => format!("uses type {}", r.target_name),
        };
        groups.entry(key).or_default().push(r);
    }

    let mut patterns: Vec<ReferencePattern> = groups
        .into_iter()
        .map(|(pattern, refs)| {
            let mut files: Vec<String> = refs
                .iter()
                .map(|r| {
                    std::path::Path::new(&r.source_file)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&r.source_file)
                        .to_string()
                })
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            files.sort();
            let total_files = files.len();
            let example_files: Vec<String> = files.into_iter().take(5).collect();

            ReferencePattern {
                pattern,
                count: refs.len(),
                example_files,
                total_files,
            }
        })
        .collect();

    // Sort by count descending
    patterns.sort_by(|a, b| b.count.cmp(&a.count));
    patterns
}

/// Format patterns into a compact impact report.
pub fn format_impact_report(entity: &str, patterns: &[ReferencePattern]) -> String {
    if patterns.is_empty() {
        return String::new();
    }

    let total_refs: usize = patterns.iter().map(|p| p.count).sum();
    let total_files: usize = patterns.iter().map(|p| p.total_files).sum();

    let mut output = format!(
        "⚠️ IMPACT: {}\nREFERENCES: {} in {} files\n",
        entity, total_refs, total_files
    );

    output.push_str("PATTERNS:\n");
    for p in patterns {
        let files_str = if p.total_files <= 5 {
            p.example_files.join(", ")
        } else {
            format!(
                "{} +{}",
                p.example_files[..3].join(", "),
                p.total_files - 3
            )
        };
        output.push_str(&format!("  {}x {} — {}\n", p.count, p.pattern, files_str));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ref(file: &str, target: &str, rt: RefType) -> CodeReference {
        CodeReference {
            source_file: file.into(),
            source_line: 1,
            target_name: target.into(),
            ref_type: rt,
        }
    }

    #[test]
    fn test_group_same_type() {
        let refs = vec![
            make_ref("a.rs", "chunk_text", RefType::Calls),
            make_ref("b.rs", "chunk_text", RefType::Calls),
            make_ref("c.rs", "chunk_text", RefType::Calls),
        ];
        let patterns = group_by_pattern(&refs);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].count, 3);
        assert!(patterns[0].pattern.contains("calls chunk_text"));
    }

    #[test]
    fn test_group_read_write_separately() {
        let refs = vec![
            make_ref("a.rs", "confidence", RefType::ReadsField),
            make_ref("b.rs", "confidence", RefType::ReadsField),
            make_ref("c.rs", "confidence", RefType::WritesField),
        ];
        let patterns = group_by_pattern(&refs);
        assert_eq!(patterns.len(), 2);
        assert!(patterns.iter().any(|p| p.pattern.contains("reads") && p.count == 2));
        assert!(patterns.iter().any(|p| p.pattern.contains("writes") && p.count == 1));
    }

    #[test]
    fn test_group_empty() {
        let patterns = group_by_pattern(&[]);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_format_report() {
        let patterns = vec![ReferencePattern {
            pattern: "calls chunk_text".into(),
            count: 47,
            example_files: vec!["a.rs".into(), "b.rs".into(), "c.rs".into()],
            total_files: 12,
        }];
        let report = format_impact_report("chunk_text", &patterns);
        assert!(report.contains("IMPACT: chunk_text"));
        assert!(report.contains("47"));
        assert!(report.contains("12 files"));
        assert!(report.contains("+9")); // 12 - 3 example files
    }

    #[test]
    fn test_sorted_by_count() {
        let refs = vec![
            make_ref("a.rs", "func", RefType::Calls),
            make_ref("b.rs", "field", RefType::ReadsField),
            make_ref("c.rs", "field", RefType::ReadsField),
            make_ref("d.rs", "field", RefType::ReadsField),
        ];
        let patterns = group_by_pattern(&refs);
        assert!(patterns[0].count >= patterns[1].count, "Should be sorted by count desc");
    }

    #[test]
    fn test_deduplicates_files() {
        let refs = vec![
            make_ref("a.rs", "func", RefType::Calls),
            make_ref("a.rs", "func", RefType::Calls), // same file, 2 calls
        ];
        let patterns = group_by_pattern(&refs);
        assert_eq!(patterns[0].count, 2);
        assert_eq!(patterns[0].total_files, 1); // deduplicated
    }
}
