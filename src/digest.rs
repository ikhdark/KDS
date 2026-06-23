use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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

pub fn make_digest(
    command_kind: &str,
    command: &str,
    cwd: &str,
    exit_code: i32,
    summary: &ExtractedSummary,
) -> String {
    let signal = summary
        .primary_failure
        .as_deref()
        .or_else(|| summary.top_errors.first().map(String::as_str))
        .or_else(|| summary.tail.last().map(String::as_str))
        .unwrap_or("");
    let first_file = summary.file_hits.first().map(String::as_str).unwrap_or("");
    let mut hasher = Sha256::new();
    hasher.update(command_kind.as_bytes());
    hasher.update(command.as_bytes());
    hasher.update(cwd.as_bytes());
    hasher.update(exit_code.to_string().as_bytes());
    hasher.update(signal.as_bytes());
    hasher.update(first_file.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn update_repeat_state(
    paths: &Paths,
    digest: &str,
    command: &str,
    cwd: &str,
    exit_code: i32,
    log_path: &Path,
    run_id: &str,
) -> Result<RepeatStatus> {
    let mut index = read_digest_index(paths);
    let now = storage::iso_now();
    let current_log_path = log_path.display().to_string();

    if exit_code == 0 {
        for state in index
            .values_mut()
            .filter(|state| state.command == command && state.cwd == cwd && !state.resolved)
        {
            state.resolved = true;
            state.resolved_at = Some(now.clone());
        }
        write_digest_index(paths, &index)?;
        return Ok(RepeatStatus {
            is_repeat: false,
            message: "success".to_string(),
            first_seen: None,
            previous_log_path: None,
            current_log_path,
            repeat_count: 0,
        });
    }

    let repeat_status = if let Some(state) = index.get_mut(digest) {
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
        index.insert(
            digest.to_string(),
            FailureState {
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
            },
        );
        RepeatStatus {
            is_repeat: false,
            message: "new failure signal".to_string(),
            first_seen: None,
            previous_log_path: None,
            current_log_path,
            repeat_count: 0,
        }
    };
    write_digest_index(paths, &index)?;
    Ok(repeat_status)
}

fn read_digest_index(paths: &Paths) -> BTreeMap<String, FailureState> {
    fs::read_to_string(&paths.digest_index)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_digest_index(paths: &Paths, index: &BTreeMap<String, FailureState>) -> Result<()> {
    storage::write_json_atomic(&paths.digest_index, index)
}
