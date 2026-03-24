use autoclaw::accuracy::load_benchmark_suite;

#[test]
fn accuracy_config_loads_repositories_and_cases() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bench_dir = manifest_dir.join("benchmarks");

    let suite = load_benchmark_suite(&bench_dir).expect("benchmark suite should load");

    let fastapi = suite
        .repositories
        .iter()
        .find(|repo| repo.name == "fastapi")
        .expect("fastapi repo should exist");
    assert_eq!(fastapi.url, "https://github.com/fastapi/fastapi.git");
    assert!(!fastapi.commit.is_empty(), "pinned commit must be set");

    let case = suite
        .cases
        .iter()
        .find(|case| case.name == "httpx-timeoutexception")
        .expect("httpx-timeoutexception case should exist");
    assert_eq!(case.repo, "httpx");
    assert_eq!(case.entity, "TimeoutException");
    assert_eq!(case.expected_files.len(), 4);
    assert!(
        case.expected_files
            .contains(&"httpx/_exceptions.py".to_string())
    );
}
