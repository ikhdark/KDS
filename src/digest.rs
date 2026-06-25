use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

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
    format!("{:x}", hasher.finalize())
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
            out.push("<path>");
        } else {
            out.push(token);
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
                "<path>"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    }
    Ok(repeat_status)
}

fn read_failure_state(paths: &Paths, digest: &str) -> Option<FailureState> {
    fs::read_to_string(digest_state_path(paths, digest))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .or_else(|| read_legacy_digest_index(paths).remove(digest))
}

fn write_failure_state(paths: &Paths, state: &FailureState) -> Result<()> {
    storage::write_json_atomic(&digest_state_path(paths, &state.digest), state)
}

fn resolve_matching_failures(paths: &Paths, command: &str, cwd: &str, now: &str) -> Result<()> {
    for path in digest_state_paths(paths) {
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
