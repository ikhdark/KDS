use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::storage::{self, Paths, RepeatStatus};
use crate::summarize::ExtractedSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailureState {
    digest: String,
    command: String,
    cwd: String,
    first_seen: String,
    last_seen: String,
    previous_log_path: String,
    last_run_id: String,
    repeat_count: u64,
    resolved: bool,
    resolved_at: Option<String>,
}

pub fn make_exact_digest(
    command_kind: &str,
    command: &str,
    cwd: &str,
    exit_code: i32,
    summary: &ExtractedSummary,
) -> String {
    let mut hasher = Sha256::new();
    let primary_signal = summary
        .primary_failure
        .as_deref()
        .or_else(|| summary.top_errors.first().map(String::as_str))
        .or_else(|| summary.tail.last().map(String::as_str))
        .unwrap_or("");
    hasher.update(command_kind.as_bytes());
    hasher.update(b"\0");
    hasher.update(command.as_bytes());
    hasher.update(b"\0");
    hasher.update(cwd.as_bytes());
    hasher.update(b"\0");
    hasher.update(exit_code.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(primary_signal.as_bytes());
    for line in summary.digest_error_lines.iter().take(3) {
        hasher.update(b"\0err:");
        hasher.update(line.as_bytes());
    }
    for hit in summary.digest_file_hits.iter().take(3) {
        hasher.update(b"\0file:");
        hasher.update(hit.as_bytes());
    }
    if let Some(hint) = &summary.test_or_package_hint {
        hasher.update(b"\0hint:");
        hasher.update(hint.as_bytes());
    }
    crate::hash::sha256_finalize_hex(hasher)
}

pub fn make_normalized_digest(
    command_kind: &str,
    command: &str,
    cwd: &str,
    exit_code: i32,
    summary: &ExtractedSummary,
) -> String {
    let mut normalized = summary.clone();
    normalized.primary_failure = normalized
        .primary_failure
        .as_deref()
        .map(normalize_failure_signal);
    normalized.top_errors = normalized
        .top_errors
        .iter()
        .map(|line| normalize_failure_signal(line))
        .collect();
    normalized.file_hits = normalized
        .file_hits
        .iter()
        .map(|hit| normalize_failure_signal(hit))
        .collect();
    normalized.digest_error_lines = normalized
        .digest_error_lines
        .iter()
        .map(|line| normalize_failure_signal(line))
        .collect();
    normalized.digest_file_hits = normalized
        .digest_file_hits
        .iter()
        .map(|hit| normalize_failure_signal(hit))
        .collect();
    make_exact_digest(command_kind, command, cwd, exit_code, &normalized)
}

fn normalize_failure_signal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            while chars.peek().is_some_and(|next| next.is_ascii_digit()) {
                let _ = chars.next();
            }
            out.push('#');
            continue;
        }
        out.push(ch);
    }
    out = normalize_windows_paths(&out);
    out = normalize_slash_paths(&out);
    out = normalize_hexish(&out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_windows_paths(text: &str) -> String {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        if token.len() > 3 && token.as_bytes().get(1) == Some(&b':') && token.contains('\\') {
            out.push(normalized_path_suffix(token, '\\'));
        } else {
            out.push(token.to_string());
        }
    }
    out.join(" ")
}

fn normalize_slash_paths(text: &str) -> String {
    text.split_whitespace()
        .map(|token| {
            if (token.starts_with('/') || token.starts_with("./") || token.starts_with("../"))
                && token.contains('/')
            {
                normalized_path_suffix(token, '/')
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_path_suffix(token: &str, separator: char) -> String {
    let components = token
        .split(separator)
        .filter(|component| !component.is_empty() && *component != "." && *component != "..")
        .collect::<Vec<_>>();
    let suffix_start = components.len().saturating_sub(2);
    let suffix = components[suffix_start..].join(&separator.to_string());
    format!("<path>{separator}{suffix}")
}

fn normalize_hexish(text: &str) -> String {
    text.split_whitespace()
        .map(|token| {
            let trimmed = token.trim_start_matches("0x");
            if token.starts_with("0x")
                && trimmed.len() >= 6
                && trimmed.chars().all(|ch| ch.is_ascii_hexdigit())
            {
                "<hex>"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn update_repeat_state_unlocked(
    paths: &Paths,
    digest: &str,
    command: &str,
    cwd: &str,
    exit_code: i32,
    log_path: &Path,
    run_id: &str,
) -> Result<RepeatStatus> {
    let now = storage::iso_now();
    let current_log_path = log_path.display().to_string();
    migrate_legacy_digest_index_once(paths)?;
    ensure_unresolved_index(paths)?;

    if exit_code == 0 {
        resolve_matching_failures(paths, command, cwd, &now)?;
        return Ok(RepeatStatus {
            is_repeat: false,
            message: "success".to_string(),
            first_seen: None,
            previous_log_path: None,
            current_log_path,
            repeat_count: 0,
        });
    }

    let mut state = read_failure_state(paths, digest);
    let repeat_status = if let Some(state) = state.as_mut() {
        let previous_log_path = state.previous_log_path.clone();
        state.repeat_count += 1;
        state.last_seen = now.clone();
        state.previous_log_path = current_log_path.clone();
        state.last_run_id = run_id.to_string();
        state.resolved = false;
        RepeatStatus {
            is_repeat: true,
            message: "same failure signal as previous run".to_string(),
            first_seen: Some(state.first_seen.clone()),
            previous_log_path: Some(previous_log_path),
            current_log_path,
            repeat_count: state.repeat_count,
        }
    } else {
        state = Some(FailureState {
            digest: digest.to_string(),
            command: command.to_string(),
            cwd: cwd.to_string(),
            first_seen: now.clone(),
            last_seen: now,
            previous_log_path: current_log_path.clone(),
            last_run_id: run_id.to_string(),
            repeat_count: 0,
            resolved: false,
            resolved_at: None,
        });
        RepeatStatus {
            is_repeat: false,
            message: "new failure signal".to_string(),
            first_seen: None,
            previous_log_path: None,
            current_log_path,
            repeat_count: 0,
        }
    };
    if let Some(state) = &state {
        write_failure_state(paths, state)?;
        if !state.resolved {
            insert_unresolved_failure(
                paths,
                command,
                cwd,
                &digest_state_path(paths, &state.digest),
            )?;
        }
    }
    Ok(repeat_status)
}

fn read_failure_state(paths: &Paths, digest: &str) -> Option<FailureState> {
    fs::read_to_string(digest_state_path(paths, digest))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn write_failure_state(paths: &Paths, state: &FailureState) -> Result<()> {
    storage::write_json_atomic(&digest_state_path(paths, &state.digest), state)
}

fn resolve_matching_failures(paths: &Paths, command: &str, cwd: &str, now: &str) -> Result<()> {
    let index_path = unresolved_by_command_path(paths, command, cwd);
    let candidate_paths = read_unresolved_failure_paths(&index_path);
    for path_text in candidate_paths {
        let path = PathBuf::from(path_text);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut state) = serde_json::from_str::<FailureState>(&text) else {
            continue;
        };
        if state.command == command && state.cwd == cwd && !state.resolved {
            state.resolved = true;
            state.resolved_at = Some(now.to_string());
            storage::write_json_atomic(&path, &state)?;
        }
    }
    storage::write_json_atomic(&index_path, &Vec::<String>::new())?;
    Ok(())
}

fn digest_state_path(paths: &Paths, digest: &str) -> std::path::PathBuf {
    let prefix = digest.get(0..2).unwrap_or("xx");
    paths.digest_dir.join(prefix).join(format!("{digest}.json"))
}

fn digest_state_paths(paths: &Paths) -> Vec<std::path::PathBuf> {
    if !paths.digest_dir.exists() {
        return Vec::new();
    }
    let Ok(prefixes) = fs::read_dir(&paths.digest_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for prefix in prefixes.flatten() {
        let path = prefix.path();
        if !path.is_dir() {
            continue;
        }
        if let Ok(files) = fs::read_dir(path) {
            out.extend(
                files
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json")),
            );
        }
    }
    out
}

fn read_legacy_digest_index(paths: &Paths) -> std::collections::BTreeMap<String, FailureState> {
    fs::read_to_string(&paths.digest_index)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn migrate_legacy_digest_index_once(paths: &Paths) -> Result<()> {
    let marker = digest_migration_marker_path(paths);
    if marker.exists() {
        return Ok(());
    }
    for state in read_legacy_digest_index(paths).values() {
        write_failure_state(paths, state)?;
    }
    storage::write_text_atomic(&marker, &storage::iso_now())
}

fn digest_migration_marker_path(paths: &Paths) -> PathBuf {
    paths.state_dir.join("digest-migration-complete")
}

fn ensure_unresolved_index(paths: &Paths) -> Result<()> {
    let marker = unresolved_by_command_ready_path(paths);
    if marker.exists() {
        return Ok(());
    }
    for path in digest_state_paths(paths) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(state) = serde_json::from_str::<FailureState>(&text) else {
            continue;
        };
        if !state.resolved {
            insert_unresolved_failure(paths, &state.command, &state.cwd, &path)?;
        }
    }
    storage::write_text_atomic(&marker, &storage::iso_now())
}

fn insert_unresolved_failure(
    paths: &Paths,
    command: &str,
    cwd: &str,
    digest_path: &Path,
) -> Result<()> {
    let index_path = unresolved_by_command_path(paths, command, cwd);
    let mut paths = read_unresolved_failure_paths(&index_path)
        .into_iter()
        .collect::<BTreeSet<_>>();
    paths.insert(digest_path.display().to_string());
    storage::write_json_atomic(&index_path, &paths.into_iter().collect::<Vec<_>>())
}

fn read_unresolved_failure_paths(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn unresolved_by_command_path(paths: &Paths, command: &str, cwd: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(command.as_bytes());
    hasher.update(b"\0");
    hasher.update(cwd.as_bytes());
    let key = crate::hash::sha256_finalize_hex(hasher);
    unresolved_by_command_dir(paths).join(format!("{key}.json"))
}

fn unresolved_by_command_dir(paths: &Paths) -> PathBuf {
    paths.state_dir.join("unresolved-by-command")
}

fn unresolved_by_command_ready_path(paths: &Paths) -> PathBuf {
    unresolved_by_command_dir(paths).join(".ready")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_paths_keep_file_suffix_identity() {
        let first = normalize_failure_signal("error: /home/me/repo/src/parser.rs:123:9");
        let second = normalize_failure_signal("error: /tmp/other/repo/src/parser.rs:456:1");
        let different = normalize_failure_signal("error: /tmp/other/repo/src/lexer.rs:456:1");
        let windows = normalize_failure_signal("error: C:\\repo\\src\\parser.rs:123:9");

        assert!(first.contains("<path>/src/parser.rs:#:#"), "{first}");
        assert_eq!(first, second);
        assert_ne!(first, different);
        assert!(windows.contains("<path>\\src\\parser.rs:#:#"), "{windows}");
    }

    #[test]
    fn legacy_digest_index_migrates_once_to_shards() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(dir.path());
        fs::create_dir_all(&paths.state_dir).unwrap();
        let state = FailureState {
            digest: "abcdef".into(),
            command: "cargo test".into(),
            cwd: "C:/repo".into(),
            first_seen: "first".into(),
            last_seen: "last".into(),
            previous_log_path: "old.log".into(),
            last_run_id: "old-run".into(),
            repeat_count: 1,
            resolved: false,
            resolved_at: None,
        };
        let mut legacy = std::collections::BTreeMap::new();
        legacy.insert(state.digest.clone(), state.clone());
        storage::write_json_atomic(&paths.digest_index, &legacy).unwrap();

        migrate_legacy_digest_index_once(&paths).unwrap();
        assert!(read_failure_state(&paths, "abcdef").is_some());
        assert!(digest_migration_marker_path(&paths).exists());

        fs::remove_file(digest_state_path(&paths, "abcdef")).unwrap();
        assert!(read_failure_state(&paths, "abcdef").is_none());
        migrate_legacy_digest_index_once(&paths).unwrap();
        assert!(read_failure_state(&paths, "abcdef").is_none());
    }

    #[test]
    fn success_resolves_only_indexed_failures_for_command() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(dir.path());
        paths.ensure_runtime_dirs().unwrap();
        update_repeat_state_unlocked(
            &paths,
            "aa1111",
            "cargo test",
            "C:/repo",
            1,
            Path::new("first.log"),
            "first",
        )
        .unwrap();
        update_repeat_state_unlocked(
            &paths,
            "bb2222",
            "cargo check",
            "C:/repo",
            1,
            Path::new("second.log"),
            "second",
        )
        .unwrap();

        update_repeat_state_unlocked(
            &paths,
            "success",
            "cargo test",
            "C:/repo",
            0,
            Path::new("success.log"),
            "success",
        )
        .unwrap();

        let resolved = read_failure_state(&paths, "aa1111").unwrap();
        let unresolved = read_failure_state(&paths, "bb2222").unwrap();
        assert!(resolved.resolved);
        assert!(resolved.resolved_at.is_some());
        assert!(!unresolved.resolved);
        assert!(read_unresolved_failure_paths(&unresolved_by_command_path(
            &paths,
            "cargo test",
            "C:/repo"
        ))
        .is_empty());
        assert!(unresolved_by_command_path(&paths, "cargo check", "C:/repo").exists());
    }

    fn test_paths(root: &Path) -> Paths {
        let logs_dir = root.join("logs");
        let state_dir = root.join("state");
        Paths {
            root: root.to_path_buf(),
            logs_dir,
            runs_index: state_dir.join("runs.jsonl"),
            digest_index: state_dir.join("digest-index.json"),
            digest_dir: state_dir.join("digest"),
            latest_by_command: state_dir.join("latest-by-command.json"),
            temp_cleanup_marker: state_dir.join("last-temp-cleanup"),
            metrics: state_dir.join("metrics.json"),
            state_dir,
        }
    }
}
