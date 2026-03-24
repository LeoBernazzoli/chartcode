use crate::config::GraphocodeConfig;
use crate::impact::reference_files_for_entity;
use crate::KnowledgeGraph;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BenchmarkRepository {
    pub name: String,
    pub url: String,
    pub commit: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BenchmarkCase {
    pub name: String,
    pub repo: String,
    pub entity: String,
    #[serde(default)]
    pub expected_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkSuite {
    pub repositories: Vec<BenchmarkRepository>,
    pub cases: Vec<BenchmarkCase>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccuracyMetrics {
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub true_positive_files: BTreeSet<String>,
    pub false_positive_files: BTreeSet<String>,
    pub false_negative_files: BTreeSet<String>,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkCaseResult {
    pub name: String,
    pub repo: String,
    pub entity: String,
    pub predicted_files: BTreeSet<String>,
    pub expected_files: BTreeSet<String>,
    pub metrics: AccuracyMetrics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkSummary {
    pub case_count: usize,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkRunResult {
    pub cases: Vec<BenchmarkCaseResult>,
    pub summary: BenchmarkSummary,
}

#[derive(Debug, Deserialize)]
struct RepositoriesFile {
    repositories: Vec<BenchmarkRepository>,
}

pub fn load_benchmark_suite(bench_dir: &Path) -> Result<BenchmarkSuite, String> {
    let repos_path = bench_dir.join("repos.toml");
    let repos_content = std::fs::read_to_string(&repos_path)
        .map_err(|e| format!("failed to read {}: {}", repos_path.display(), e))?;
    let mut repositories: Vec<BenchmarkRepository> = toml::from_str::<RepositoriesFile>(&repos_content)
        .map_err(|e| format!("failed to parse {}: {}", repos_path.display(), e))?
        .repositories;
    repositories.sort_by(|a, b| a.name.cmp(&b.name));

    let cases_dir = bench_dir.join("cases");
    let mut entries: Vec<_> = std::fs::read_dir(&cases_dir)
        .map_err(|e| format!("failed to read {}: {}", cases_dir.display(), e))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect();
    entries.sort_by_key(|entry| entry.path());

    let mut cases = Vec::new();
    for entry in entries {
        let path = entry.path();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let mut case: BenchmarkCase = toml::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {}", path.display(), e))?;
        case.expected_files = case
            .expected_files
            .into_iter()
            .map(|file| normalize_file_path(&file))
            .collect();
        cases.push(case);
    }
    cases.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(BenchmarkSuite { repositories, cases })
}

pub fn compute_accuracy_metrics(
    predicted: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> AccuracyMetrics {
    let true_positive_files: BTreeSet<String> =
        predicted.intersection(expected).cloned().collect();
    let false_positive_files: BTreeSet<String> =
        predicted.difference(expected).cloned().collect();
    let false_negative_files: BTreeSet<String> =
        expected.difference(predicted).cloned().collect();

    let true_positives = true_positive_files.len();
    let false_positives = false_positive_files.len();
    let false_negatives = false_negative_files.len();

    let precision = safe_ratio(true_positives, true_positives + false_positives);
    let recall = safe_ratio(true_positives, true_positives + false_negatives);
    let f1 = if precision == 0.0 || recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };

    AccuracyMetrics {
        true_positives,
        false_positives,
        false_negatives,
        true_positive_files,
        false_positive_files,
        false_negative_files,
        precision,
        recall,
        f1,
    }
}

pub fn normalize_file_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    while normalized.starts_with("./") {
        normalized = normalized.trim_start_matches("./").to_string();
    }
    normalized
}

pub fn run_benchmark_suite(
    bench_dir: &Path,
    cache_dir: &Path,
    case_filter: Option<&str>,
) -> Result<BenchmarkRunResult, String> {
    let suite = load_benchmark_suite(bench_dir)?;
    let repositories: HashMap<String, BenchmarkRepository> = suite
        .repositories
        .into_iter()
        .map(|repo| (repo.name.clone(), repo))
        .collect();

    let selected_cases: Vec<BenchmarkCase> = suite
        .cases
        .into_iter()
        .filter(|case| case_filter.map(|filter| case.name == filter).unwrap_or(true))
        .collect();

    if selected_cases.is_empty() {
        return Err(match case_filter {
            Some(filter) => format!("no benchmark case found: {}", filter),
            None => "no benchmark cases found".to_string(),
        });
    }

    let mut checkout_cache: HashMap<String, PathBuf> = HashMap::new();
    let mut graph_cache: HashMap<String, KnowledgeGraph> = HashMap::new();
    let mut results = Vec::new();

    for case in selected_cases {
        let repo = repositories
            .get(&case.repo)
            .ok_or_else(|| format!("benchmark repo '{}' not found for case '{}'", case.repo, case.name))?;

        let checkout_path = match checkout_cache.get(&repo.name) {
            Some(path) => path.clone(),
            None => {
                let path = ensure_repo_checkout(repo, cache_dir)?;
                checkout_cache.insert(repo.name.clone(), path.clone());
                path
            }
        };

        if !graph_cache.contains_key(&repo.name) {
            let mut config = GraphocodeConfig::default();
            config.sources.conversations = false;
            config.sources.documents.clear();

            let mut kg = KnowledgeGraph::new();
            crate::bootstrap::bootstrap(&mut kg, &config, &checkout_path);
            graph_cache.insert(repo.name.clone(), kg);
        }

        let kg = graph_cache
            .get(&repo.name)
            .ok_or_else(|| format!("failed to bootstrap benchmark repo '{}'", repo.name))?;
        let predicted_files = reference_files_for_entity(kg, &case.entity);
        let expected_files: BTreeSet<String> = case
            .expected_files
            .iter()
            .map(|file| normalize_file_path(file))
            .collect();
        let metrics = compute_accuracy_metrics(&predicted_files, &expected_files);

        results.push(BenchmarkCaseResult {
            name: case.name,
            repo: case.repo,
            entity: case.entity,
            predicted_files,
            expected_files,
            metrics,
        });
    }

    let summary = summarize_results(&results);
    Ok(BenchmarkRunResult { cases: results, summary })
}

pub fn format_benchmark_report(result: &BenchmarkRunResult) -> String {
    let mut lines = Vec::new();
    for case in &result.cases {
        lines.push(format!(
            "{} repo={} entity={} tp={} fp={} fn={} precision={:.3} recall={:.3} f1={:.3}",
            case.name,
            case.repo,
            case.entity,
            case.metrics.true_positives,
            case.metrics.false_positives,
            case.metrics.false_negatives,
            case.metrics.precision,
            case.metrics.recall,
            case.metrics.f1
        ));

        if !case.metrics.false_positive_files.is_empty() {
            lines.push(format!(
                "  fp_files={}",
                case.metrics
                    .false_positive_files
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !case.metrics.false_negative_files.is_empty() {
            lines.push(format!(
                "  fn_files={}",
                case.metrics
                    .false_negative_files
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
    }

    lines.push(format!(
        "TOTAL cases={} tp={} fp={} fn={} precision={:.3} recall={:.3} f1={:.3}",
        result.summary.case_count,
        result.summary.true_positives,
        result.summary.false_positives,
        result.summary.false_negatives,
        result.summary.precision,
        result.summary.recall,
        result.summary.f1
    ));

    lines.join("\n")
}

fn safe_ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn summarize_results(results: &[BenchmarkCaseResult]) -> BenchmarkSummary {
    let true_positives: usize = results.iter().map(|result| result.metrics.true_positives).sum();
    let false_positives: usize = results.iter().map(|result| result.metrics.false_positives).sum();
    let false_negatives: usize = results.iter().map(|result| result.metrics.false_negatives).sum();
    let precision = safe_ratio(true_positives, true_positives + false_positives);
    let recall = safe_ratio(true_positives, true_positives + false_negatives);
    let f1 = if precision == 0.0 || recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };

    BenchmarkSummary {
        case_count: results.len(),
        true_positives,
        false_positives,
        false_negatives,
        precision,
        recall,
        f1,
    }
}

fn ensure_repo_checkout(repo: &BenchmarkRepository, cache_dir: &Path) -> Result<PathBuf, String> {
    let absolute_cache_dir = if cache_dir.is_absolute() {
        cache_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("failed to read current dir: {}", e))?
            .join(cache_dir)
    };

    std::fs::create_dir_all(&absolute_cache_dir).map_err(|e| {
        format!(
            "failed to create cache dir {}: {}",
            absolute_cache_dir.display(),
            e
        )
    })?;

    let checkout_dir = absolute_cache_dir.join(&repo.name);
    if !checkout_dir.join(".git").exists() {
        run_git(
            &absolute_cache_dir,
            &[
                "clone",
                "--quiet",
                "--no-checkout",
                &repo.url,
                checkout_dir.to_string_lossy().as_ref(),
            ],
        )?;
    }

    run_git(
        &checkout_dir,
        &["fetch", "--quiet", "--depth", "1", "origin", &repo.commit],
    )?;
    run_git(
        &checkout_dir,
        &["checkout", "--quiet", "--force", "--detach", &repo.commit],
    )?;

    Ok(checkout_dir)
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git {:?}: {}", args, e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {:?} failed in {}: {}",
            args,
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
