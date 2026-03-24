use std::process::Command;

#[test]
fn accuracy_cli_runs_against_a_pinned_local_repository() {
    let temp = tempfile::tempdir().unwrap();
    let bench_dir = write_local_benchmark_fixture(temp.path());
    let cache_dir = temp.path().join("cache");

    let output = Command::new(env!("CARGO_BIN_EXE_chartcode"))
        .arg("accuracy-bench")
        .arg("--bench-dir")
        .arg(&bench_dir)
        .arg("--cache-dir")
        .arg(&cache_dir)
        .arg("--case")
        .arg("local-timeout")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("local-timeout"));
    assert!(stdout.contains("precision=1.000"));
    assert!(stdout.contains("recall=1.000"));
    assert!(stdout.contains("f1=1.000"));
}

#[test]
fn accuracy_cli_accepts_relative_cache_dir() {
    let temp = tempfile::tempdir().unwrap();
    let bench_dir = write_local_benchmark_fixture(temp.path());

    let output = Command::new(env!("CARGO_BIN_EXE_chartcode"))
        .current_dir(temp.path())
        .arg("accuracy-bench")
        .arg("--bench-dir")
        .arg(&bench_dir)
        .arg("--cache-dir")
        .arg("relative-cache")
        .arg("--case")
        .arg("local-timeout")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("local-timeout"));
    assert!(stdout.contains("precision=1.000"));
}

fn write_local_benchmark_fixture(root: &std::path::Path) -> std::path::PathBuf {
    let source_repo = root.join("source-repo");
    let bench_dir = root.join("benchmarks");
    let cases_dir = bench_dir.join("cases");

    std::fs::create_dir_all(&source_repo).unwrap();
    std::fs::create_dir_all(&cases_dir).unwrap();

    std::fs::write(
        source_repo.join("models.py"),
        "class TimeoutException(Exception):\n    pass\n",
    )
    .unwrap();
    std::fs::write(
        source_repo.join("api.py"),
        "from models import TimeoutException\n\n\ndef fetch():\n    raise TimeoutException()\n",
    )
    .unwrap();

    run_git(&source_repo, ["init"]);
    run_git(&source_repo, ["add", "."]);
    run_git(
        &source_repo,
        [
            "-c",
            "user.name=Chartcode Tests",
            "-c",
            "user.email=tests@example.com",
            "commit",
            "-m",
            "seed",
        ],
    );

    let commit = run_git_capture(&source_repo, ["rev-parse", "HEAD"]);

    std::fs::write(
        bench_dir.join("repos.toml"),
        format!(
            "[[repositories]]\nname = \"local\"\nurl = \"{}\"\ncommit = \"{}\"\n",
            source_repo.display(),
            commit.trim()
        ),
    )
    .unwrap();
    std::fs::write(
        cases_dir.join("local-timeout.toml"),
        "name = \"local-timeout\"\nrepo = \"local\"\nentity = \"TimeoutException\"\nexpected_files = [\"api.py\"]\n",
    )
    .unwrap();

    bench_dir
}

fn run_git<const N: usize>(cwd: &std::path::Path, args: [&str; N]) {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed", args);
}

fn run_git_capture<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
