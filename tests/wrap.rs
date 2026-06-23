use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

#[test]
fn wraps_real_command_and_writes_local_run_artifacts() {
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
    assert!(stdout.contains("KDS\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("Exit code: 0"), "stdout:\n{stdout}");
    assert!(stdout.contains("Summary: success"), "stdout:\n{stdout}");

    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let log = String::from_utf8_lossy(&fs::read(&logs[0]).unwrap()).to_string();
    assert!(log.contains("Command: rustc --version"), "log:\n{log}");
    assert!(log.contains("rustc "), "log:\n{log}");

    let summaries = collect_files(&kds_home.path().join("logs"), "json");
    assert_eq!(summaries.len(), 1, "summaries: {summaries:?}");

    let index = fs::read_to_string(kds_home.path().join("state").join("runs.jsonl")).unwrap();
    assert!(index.contains("rustc --version"), "index:\n{index}");
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
        .arg("--")
        .arg("foo")
        .arg("ok")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("KDS\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("Exit code: 0"), "stdout:\n{stdout}");

    let logs = collect_files(&kds_home.path().join("logs"), "log");
    assert_eq!(logs.len(), 1, "logs: {logs:?}");
    let log = String::from_utf8_lossy(&fs::read(&logs[0]).unwrap()).to_string();
    assert!(log.contains("Command: foo ok"), "log:\n{log}");
    assert!(log.contains("shim:ok"), "log:\n{log}");
}
