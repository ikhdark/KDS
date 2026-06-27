use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

fn kds_bin() -> &'static str {
    env!("CARGO_BIN_EXE_kds")
}

fn collect_files(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !root.exists() {
        return files;
    }

    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(collect_files(&path, extension));
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            files.push(path);
        }
    }
    files
}

fn run_id_from_stdout(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("Run ID: "))
        .expect("missing Run ID")
        .to_string()
}

fn ignored_args_command(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let path = dir.join("ignore-args.cmd");
        fs::write(&path, "@echo off\r\nexit /b 0\r\n").unwrap();
        path
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("ignore-args.sh");
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&path, perms).unwrap();
        path
    }
}

fn fake_git_command(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let path = dir.join("git.cmd");
        fs::write(
            &path,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho native-git:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();
        path
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("git");
        fs::write(
            &path,
            "#!/bin/sh\nfor arg in \"$@\"; do printf 'native-git:[%s]\\n' \"$arg\"; done\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&path, perms).unwrap();
        path
    }
}

#[test]
fn wraps_real_command_and_writes_local_run_artifacts() {
    let kds_home = tempfile::tempdir().unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env("KDS_SAVE_ARTIFACTS", "1")
        .arg("--")
        .arg("rustc")
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("Result: passed"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Saved logs: yes; inspect with `kds logs "),
        "stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains(&kds_home.path().display().to_string()),
        "stdout:\n{stdout}"
    );

    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let log = String::from_utf8_lossy(&fs::read(&logs[0]).unwrap()).to_string();
    assert!(log.contains("Command: rustc --version"), "log:\n{log}");
    assert!(log.contains("rustc "), "log:\n{log}");

    let summaries = collect_files(&kds_home.path().join("logs"), "json");
    assert_eq!(summaries.len(), 1, "summaries: {summaries:?}");
    let sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(&summaries[0]).unwrap()).unwrap();
    assert!(sidecar["raw_total_chars"].as_u64().unwrap() > 0);
    assert!(sidecar["shown_chars"].as_u64().unwrap() > 0);
    assert!(sidecar["approx_raw_tokens"].as_u64().unwrap() > 0);
    assert!(sidecar["exact_digest"].as_str().unwrap().len() >= 64);
    assert!(sidecar["normalized_digest"].as_str().unwrap().len() >= 64);

    let index = fs::read_to_string(kds_home.path().join("state").join("runs.jsonl")).unwrap();
    assert!(index.contains("rustc --version"), "index:\n{index}");
    assert!(kds_home
        .path()
        .join("state")
        .join("latest-by-command.json")
        .is_file());
    let digest_shards = collect_files(&kds_home.path().join("state").join("digest"), "json");
    assert!(
        digest_shards.is_empty(),
        "successful run should not create failure digest shards"
    );
    let metrics_path = kds_home.path().join("state").join("metrics.json");
    let metrics: serde_json::Value =
        serde_json::from_slice(&fs::read(&metrics_path).unwrap()).unwrap();
    assert_eq!(metrics["saved_artifact_count"].as_u64(), Some(1));
    assert_eq!(metrics["memory_only_count"].as_u64(), Some(0));
    assert!(
        metrics["per_command"]
            .as_object()
            .unwrap()
            .contains_key("rustc --version"),
        "saved metrics should retain command-level stats: {metrics}"
    );

    let temp_files = collect_files(&kds_home.path().join("logs"), "tmp");
    assert!(temp_files.is_empty(), "temp files: {temp_files:?}");
}

#[test]
fn default_run_does_not_write_local_artifacts() {
    let kds_home = tempfile::tempdir().unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("--")
        .arg("rustc")
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("Saved logs: no"), "stdout:\n{stdout}");
    assert!(!stdout.contains("--save-artifacts"), "stdout:\n{stdout}");
    assert!(!stdout.contains("logs/evidence"), "stdout:\n{stdout}");
    assert!(
        !kds_home.path().join("logs").exists(),
        "logs dir should not be created"
    );
    let metrics_path = kds_home.path().join("state").join("metrics.json");
    assert!(metrics_path.is_file(), "metrics should be aggregate-only");
    let metrics: serde_json::Value =
        serde_json::from_slice(&fs::read(&metrics_path).unwrap()).unwrap();
    assert_eq!(metrics["memory_only_count"].as_u64(), Some(1));
    assert_eq!(metrics["saved_artifact_count"].as_u64(), Some(0));
    assert!(metrics["approx_raw_tokens"].as_u64().unwrap() > 0);
    assert!(
        metrics["per_command"].as_object().unwrap().is_empty(),
        "memory-only metrics should not keep command strings: {metrics}"
    );
    assert!(
        !kds_home.path().join("state").join("runs.jsonl").exists(),
        "default run should not create saved-run index"
    );
    assert!(
        !kds_home.path().join("state").join("digest").exists(),
        "default run should not create digest shards"
    );
}

#[test]
fn default_summarize_does_not_write_local_artifacts() {
    let kds_home = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let log_path = input_dir.path().join("ci.log");
    fs::write(
        &log_path,
        "src/app.ts(12,7): error TS2304: Cannot find name 'x'.\n",
    )
    .unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("summarize")
        .arg("--file")
        .arg(&log_path)
        .arg("--name")
        .arg("ci-log")
        .arg("--exit-code")
        .arg("1")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("Saved logs: no"), "stdout:\n{stdout}");
    assert!(!stdout.contains("--save-artifacts"), "stdout:\n{stdout}");
    assert!(!stdout.contains("logs/evidence"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Result: failed (exit code 1)"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("src/app.ts:12:7"), "stdout:\n{stdout}");
    assert!(
        !kds_home.path().join("logs").exists(),
        "logs dir should not be created"
    );
    let metrics_path = kds_home.path().join("state").join("metrics.json");
    assert!(metrics_path.is_file(), "metrics should be aggregate-only");
    let metrics: serde_json::Value =
        serde_json::from_slice(&fs::read(&metrics_path).unwrap()).unwrap();
    assert_eq!(metrics["memory_only_count"].as_u64(), Some(1));
    assert_eq!(metrics["saved_artifact_count"].as_u64(), Some(0));
    assert!(
        metrics["per_command"].as_object().unwrap().is_empty(),
        "memory-only metrics should not keep command strings: {metrics}"
    );
    assert!(
        !kds_home.path().join("state").join("runs.jsonl").exists(),
        "default summarize should not create saved-run index"
    );
}

#[test]
fn successful_warning_summary_uses_compact_warning_layout() {
    let kds_home = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let log_path = input_dir.path().join("warnings.log");
    fs::write(
        &log_path,
        "warning: unused variable `count`\nfinal successful line\n",
    )
    .unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("summarize")
        .arg("--file")
        .arg(&log_path)
        .arg("--name")
        .arg("warning-log")
        .arg("--exit-code")
        .arg("0")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Result: passed with warnings"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("Top warnings:"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("warning: unused variable"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("Details: unavailable"), "stdout:\n{stdout}");
    assert!(!stdout.contains("Last output shown:"), "stdout:\n{stdout}");
}

#[test]
fn gain_reports_token_first_and_artifact_scope() {
    let kds_home = tempfile::tempdir().unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("--")
        .arg("rustc")
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("gain")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Estimated token savings:"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("Char savings:"), "stdout:\n{stdout}");
    assert!(stdout.contains("Line savings:"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Artifacts counted: 0 saved, 1 memory-only"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("Run-level drilldown: unavailable for memory-only runs"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn summarizes_existing_log_file_into_safe_artifacts() {
    let kds_home = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let log_path = input_dir.path().join("ci.log");
    fs::write(
        &log_path,
        "C:\\repo\\src\\app.ts\n  12:7  error  token=SECRET_CANARY_VALUE failed  no-undef\nfinal tail\n",
    )
    .unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("summarize")
        .arg("--file")
        .arg(&log_path)
        .arg("--name")
        .arg("ci-log")
        .arg("--exit-code")
        .arg("1")
        .arg("--save-artifacts")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Command: kds-summarize ci-log"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("Result: failed (exit code 1)"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("src\\app.ts:12:7"), "stdout:\n{stdout}");
    assert!(!stdout.contains("SECRET_CANARY_VALUE"), "stdout:\n{stdout}");
    assert!(
        !stdout.contains(&kds_home.path().display().to_string()),
        "stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains(&log_path.display().to_string()),
        "stdout:\n{stdout}"
    );

    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let log = String::from_utf8_lossy(&fs::read(&logs[0]).unwrap()).to_string();
    assert!(log.contains("Command: kds-summarize ci-log"), "log:\n{log}");
    assert!(log.contains("token=[redacted]"), "log:\n{log}");
    assert!(!log.contains("SECRET_CANARY_VALUE"), "log:\n{log}");

    let summaries = collect_files(&kds_home.path().join("logs"), "json");
    assert_eq!(summaries.len(), 1, "summaries: {summaries:?}");
    let sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(&summaries[0]).unwrap()).unwrap();
    assert_eq!(sidecar["mode"].as_str(), Some("import"));
    assert_eq!(
        sidecar["capture_mode"].as_str(),
        Some("file import; redacted before local artifact write")
    );
    assert_eq!(sidecar["exit_code"].as_i64(), Some(1));
    assert!(sidecar["top_errors"][0]
        .as_str()
        .unwrap()
        .contains("src\\app.ts:12:7"));

    let evidence = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("evidence")
        .arg("last")
        .output()
        .unwrap();
    assert!(evidence.status.success(), "{evidence:?}");
    let stdout = String::from_utf8_lossy(&evidence.stdout);
    assert!(stdout.contains("KDS evidence"), "stdout:\n{stdout}");
    assert!(!stdout.contains("SECRET_CANARY_VALUE"), "stdout:\n{stdout}");
}

#[test]
fn show_paths_explicitly_prints_log_path() {
    let kds_home = tempfile::tempdir().unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("run")
        .arg("--show-paths")
        .arg("--save-artifacts")
        .arg("--")
        .arg("rustc")
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&kds_home.path().display().to_string()),
        "stdout:\n{stdout}"
    );
}

#[test]
fn proof_style_git_commands_pass_through_without_kds_artifacts() {
    let kds_home = tempfile::tempdir().unwrap();
    let shim_dir = tempfile::tempdir().unwrap();
    let _git = fake_git_command(shim_dir.path());
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![shim_dir.path().to_path_buf()];
    paths.extend(std::env::split_paths(&old_path));
    let path = std::env::join_paths(paths).unwrap();

    for args in [
        vec!["diff", "--check"],
        vec!["status", "--short"],
        vec!["rev-parse", "HEAD"],
        vec!["hash-object", "Cargo.toml"],
        vec!["log", "--oneline", "-1"],
        vec!["show", "--stat", "HEAD"],
        vec!["ls-files"],
        vec!["describe", "--tags"],
        vec!["tag", "--list"],
    ] {
        let output = Command::new(kds_bin())
            .env("KDS_HOME", kds_home.path())
            .env("PATH", &path)
            .arg("--")
            .arg("git")
            .args(&args)
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("native-git:[{}]", args[0])),
            "stdout:\n{stdout}"
        );
        assert!(!stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    }
    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert!(logs.is_empty(), "logs: {logs:?}");
}

#[test]
fn raw_log_command_header_redacts_sensitive_argv() {
    let kds_home = tempfile::tempdir().unwrap();
    let shim_dir = tempfile::tempdir().unwrap();
    let shim = ignored_args_command(shim_dir.path());

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env("KDS_SAVE_ARTIFACTS", "1")
        .arg("--")
        .arg(shim)
        .arg("--token")
        .arg("SECRET_CANARY_VALUE")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let log = String::from_utf8_lossy(&fs::read(&logs[0]).unwrap()).to_string();
    assert!(log.contains("--token [redacted]"), "log:\n{log}");
    assert!(!log.contains("SECRET_CANARY_VALUE"), "log:\n{log}");
}

#[test]
fn raw_log_capture_can_be_capped_by_env() {
    let kds_home = tempfile::tempdir().unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env("KDS_SAVE_ARTIFACTS", "1")
        .env("KDS_MAX_RAW_BYTES", "5")
        .arg("--")
        .arg("rustc")
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let log = String::from_utf8_lossy(&fs::read(&logs[0]).unwrap()).to_string();
    assert!(
        log.contains("stdout raw log capture reached 5 bytes"),
        "log:\n{log}"
    );
    let summaries = collect_files(&kds_home.path().join("logs"), "json");
    assert_eq!(summaries.len(), 1, "summaries: {summaries:?}");
    let sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(&summaries[0]).unwrap()).unwrap();
    assert_eq!(sidecar["raw_byte_limit"].as_u64(), Some(5));
    assert_eq!(sidecar["raw_stdout_truncated"].as_bool(), Some(true));
    assert!(sidecar["raw_stdout_discarded_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn doctor_reports_malformed_state_without_creating_logs() {
    let kds_home = tempfile::tempdir().unwrap();
    let profile_dir = tempfile::tempdir().unwrap();
    let state_dir = kds_home.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("runs.jsonl"), "{not valid json\n").unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env(
            "KDS_POWERSHELL_PROFILE",
            profile_dir.path().join("profile.ps1"),
        )
        .arg("doctor")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "Runs index health: 0 valid run(s), 1 malformed line(s), 0 unreadable line(s)"
        ),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("Update check: run `kds update check`"),
        "stdout:\n{stdout}"
    );
    assert!(
        !kds_home.path().join("logs").exists(),
        "doctor should not create logs dir"
    );
    assert!(stdout.contains("Codex Desktop hook:"), "stdout:\n{stdout}");
    assert!(stdout.contains("Codex hooks.json:"), "stdout:\n{stdout}");
}

#[test]
fn update_help_exposes_explicit_check_command_without_network() {
    let output = Command::new(kds_bin())
        .arg("update")
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("check"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Check for KDS updates") || stdout.contains("Usage:"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn logs_show_missing_sidecar_uses_path_safe_error() {
    let kds_home = tempfile::tempdir().unwrap();
    let state_dir = kds_home.path().join("state");
    let logs_dir = kds_home.path().join("logs").join("2026-01-01");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&logs_dir).unwrap();
    let summary_path = logs_dir.join("missing.summary.json");
    let entry = serde_json::json!({
        "index_schema_version": 1,
        "run_id": "2026-01-01-010101-cargo-test-a1b2c3",
        "summary_path": summary_path.display().to_string(),
        "exit_code": 1,
        "command_kind": "cargo",
        "command": "cargo test",
        "argv": ["cargo", "test"],
        "cwd": "C:/repo",
        "started_at": "2026-01-01T01:01:01Z",
        "log_path": logs_dir.join("missing.log").display().to_string()
    });
    fs::write(state_dir.join("runs.jsonl"), format!("{entry}\n")).unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("logs")
        .arg("a1b2c3")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("KDS summary sidecar is missing or unreadable"),
        "stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains(&kds_home.path().display().to_string()),
        "stderr:\n{stderr}"
    );
}

#[test]
fn logs_stats_reports_safe_artifact_counts() {
    let kds_home = tempfile::tempdir().unwrap();

    let run = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env("KDS_SAVE_ARTIFACTS", "1")
        .arg("--")
        .arg("rustc")
        .arg("--version")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("logs")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("KDS logs stats"), "stdout:\n{stdout}");
    assert!(stdout.contains("Runs indexed: 1"), "stdout:\n{stdout}");
    assert!(stdout.contains("Raw logs: 1"), "stdout:\n{stdout}");
    assert!(stdout.contains("Summary sidecars: 1"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Logs directory: use `kds logs --show-paths`"),
        "stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains(&kds_home.path().display().to_string()),
        "stdout:\n{stdout}"
    );

    let dir = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("logs")
        .arg("--show-paths")
        .output()
        .unwrap();
    assert!(dir.status.success(), "{dir:?}");
    let stdout = String::from_utf8_lossy(&dir.stdout);
    assert!(
        stdout.contains(&kds_home.path().join("logs").display().to_string()),
        "stdout:\n{stdout}"
    );
}

#[test]
fn logs_rejects_removed_show_alias() {
    let output = Command::new(kds_bin())
        .arg("logs")
        .arg("show")
        .arg("last")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("logs show alias was removed"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn clean_removes_old_artifacts_under_logs_only() {
    let kds_home = tempfile::tempdir().unwrap();
    let day = kds_home.path().join("logs").join("2026-01-01");
    fs::create_dir_all(&day).unwrap();
    let log = day.join("old.log");
    let summary = day.join("old.summary.json");
    let keep = day.join("keep.txt");
    fs::write(&log, "old log").unwrap();
    fs::write(&summary, "{}").unwrap();
    fs::write(&keep, "not a kds artifact").unwrap();
    thread::sleep(Duration::from_millis(1500));

    let delete = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("clean")
        .arg("--older-than")
        .arg("1s")
        .output()
        .unwrap();
    assert!(delete.status.success(), "{delete:?}");
    let stdout = String::from_utf8_lossy(&delete.stdout);
    assert!(stdout.contains("KDS clean"), "stdout:\n{stdout}");
    assert!(stdout.contains("Deleted: 2"), "stdout:\n{stdout}");
    assert!(!log.exists());
    assert!(!summary.exists());
    assert!(keep.exists());
}

#[test]
fn logs_show_error_window_prints_bounded_context() {
    let kds_home = tempfile::tempdir().unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env("KDS_SAVE_ARTIFACTS", "1")
        .arg("--")
        .arg("pwsh")
        .arg("-NoProfile")
        .arg("-Command")
        .arg("Write-Output 'before'; Write-Error 'window boom'; Write-Output 'after'; exit 7")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(7), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let run_id = run_id_from_stdout(&stdout);
    let show = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("logs")
        .arg(run_id)
        .arg("--error-window")
        .output()
        .unwrap();
    assert!(show.status.success(), "{show:?}");
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(stdout.contains("Error windows"), "stdout:\n{stdout}");
    assert!(stdout.contains("window boom"), "stdout:\n{stdout}");

    let show_last = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .arg("logs")
        .arg("--error-window")
        .output()
        .unwrap();
    assert!(show_last.status.success(), "{show_last:?}");
    let stdout = String::from_utf8_lossy(&show_last.stdout);
    assert!(stdout.contains("Error windows"), "stdout:\n{stdout}");
    assert!(stdout.contains("window boom"), "stdout:\n{stdout}");
}

#[test]
fn repeat_failures_use_short_compact_output_and_digest_shards() {
    let kds_home = tempfile::tempdir().unwrap();
    let mut second_stdout = String::new();
    for _ in 0..2 {
        let output = Command::new(kds_bin())
            .env("KDS_HOME", kds_home.path())
            .env("KDS_SAVE_ARTIFACTS", "1")
            .arg("--")
            .arg("pwsh")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("Write-Error 'repeat boom'; exit 9")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(9), "{output:?}");
        second_stdout = String::from_utf8_lossy(&output.stdout).to_string();
    }
    assert!(
        second_stdout.contains("Seen before: yes (same failure signal as previous run)"),
        "stdout:\n{second_stdout}"
    );
    assert!(
        !second_stdout.contains("Last output shown:"),
        "stdout:\n{second_stdout}"
    );
    let digest_shards = collect_files(&kds_home.path().join("state").join("digest"), "json");
    assert!(!digest_shards.is_empty(), "digest shards missing");
}

#[test]
fn spawn_failure_writes_run_artifacts() {
    let kds_home = tempfile::tempdir().unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env("KDS_SAVE_ARTIFACTS", "1")
        .arg("--")
        .arg("definitely-not-a-real-kds-test-command")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Result: failed (exit code 1)"),
        "stdout:\n{stdout}"
    );
    assert!(
        stderr.contains("kds: failed to run `definitely-not-a-real-kds-test-command`"),
        "stderr:\n{stderr}"
    );

    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let summaries = collect_files(&kds_home.path().join("logs"), "json");
    assert_eq!(summaries.len(), 1, "summaries: {summaries:?}");
    let index = fs::read_to_string(kds_home.path().join("state").join("runs.jsonl")).unwrap();
    assert!(
        index.contains("definitely-not-a-real-kds-test-command"),
        "index:\n{index}"
    );
}

#[test]
fn parallel_runs_keep_index_and_artifacts_consistent() {
    let kds_home = tempfile::tempdir().unwrap();
    let mut handles = Vec::new();
    for _ in 0..6 {
        let kds_home = kds_home.path().to_path_buf();
        handles.push(thread::spawn(move || {
            Command::new(kds_bin())
                .env("KDS_HOME", kds_home)
                .env("KDS_SAVE_ARTIFACTS", "1")
                .arg("--")
                .arg("rustc")
                .arg("--version")
                .output()
                .unwrap()
        }));
    }

    for handle in handles {
        let output = handle.join().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    }

    let logs = collect_files(&kds_home.path().join("logs"), "log");
    let summaries = collect_files(&kds_home.path().join("logs"), "json");
    assert_eq!(logs.len(), 6, "logs: {logs:?}");
    assert_eq!(summaries.len(), 6, "summaries: {summaries:?}");
    let index = fs::read_to_string(kds_home.path().join("state").join("runs.jsonl")).unwrap();
    assert_eq!(index.lines().count(), 6, "index:\n{index}");
    assert_eq!(
        index.matches("rustc --version").count(),
        6,
        "index:\n{index}"
    );
}

#[cfg(windows)]
#[test]
fn wraps_windows_pathext_cmd_shim_end_to_end() {
    let kds_home = tempfile::tempdir().unwrap();
    let shim_dir = tempfile::tempdir().unwrap();
    fs::write(
        shim_dir.path().join("foo.cmd"),
        "@echo off\r\necho shim:%1\r\nexit /b 0\r\n",
    )
    .unwrap();

    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![shim_dir.path().to_path_buf()];
    paths.extend(std::env::split_paths(&old_path));
    let path = std::env::join_paths(paths).unwrap();

    let output = Command::new(kds_bin())
        .env("KDS_HOME", kds_home.path())
        .env("PATH", path)
        .env("KDS_SAVE_ARTIFACTS", "1")
        .arg("--")
        .arg("foo")
        .arg("ok")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("KDS summary\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("Result: passed"), "stdout:\n{stdout}");

    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let log = String::from_utf8_lossy(&fs::read(&logs[0]).unwrap()).to_string();
    assert!(log.contains("Command: foo ok"), "log:\n{log}");
    assert!(log.contains("shim:ok"), "log:\n{log}");
}
