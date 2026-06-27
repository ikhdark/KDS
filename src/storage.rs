use anyhow::{Context, Result};
use chrono::{DateTime, Local, SecondsFormat};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SUMMARY_SCHEMA_VERSION: u32 = 6;
pub const INDEX_SCHEMA_VERSION: u32 = 1;
pub const METRICS_SCHEMA_VERSION: u32 = 4;

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub logs_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runs_index: PathBuf,
    pub digest_index: PathBuf,
    pub digest_dir: PathBuf,
    pub latest_by_command: PathBuf,
    pub temp_cleanup_marker: PathBuf,
    pub metrics: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunPaths {
    pub run_id: String,
    pub log_path: PathBuf,
    pub summary_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousExactMatchRun {
    pub run_id: String,
    pub exit_code: i32,
    pub digest: String,
    pub summary_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatStatus {
    pub is_repeat: bool,
    pub message: String,
    pub first_seen: Option<String>,
    pub previous_log_path: Option<String>,
    pub current_log_path: String,
    pub repeat_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorWindow {
    pub stream: String,
    pub line: usize,
    pub before: Vec<String>,
    pub matched: String,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarySidecar {
    pub summary_schema_version: u32,
    pub kds_version: String,
    pub run_id: String,
    pub summary_path: String,
    pub command: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub mode: String,
    pub exit_code: i32,
    pub elapsed: String,
    pub elapsed_ms: u128,
    pub digest: String,
    #[serde(default)]
    pub exact_digest: String,
    #[serde(default)]
    pub normalized_digest: String,
    pub repeat_status: RepeatStatus,
    pub raw_stdout_lines: usize,
    pub raw_stderr_lines: usize,
    pub raw_total_lines: usize,
    #[serde(default)]
    pub raw_stdout_chars: usize,
    #[serde(default)]
    pub raw_stderr_chars: usize,
    #[serde(default)]
    pub raw_total_chars: usize,
    #[serde(default)]
    pub raw_byte_limit: Option<u64>,
    #[serde(default)]
    pub raw_stdout_truncated: bool,
    #[serde(default)]
    pub raw_stderr_truncated: bool,
    #[serde(default)]
    pub raw_stdout_discarded_bytes: u64,
    #[serde(default)]
    pub raw_stderr_discarded_bytes: u64,
    pub shown_lines: usize,
    #[serde(default)]
    pub shown_chars: usize,
    pub estimated_saved_lines: usize,
    #[serde(default)]
    pub estimated_saved_chars: usize,
    pub estimated_output_reduction_percent: f64,
    #[serde(default)]
    pub estimated_char_reduction_percent: f64,
    #[serde(default)]
    pub approx_raw_tokens: usize,
    #[serde(default)]
    pub approx_shown_tokens: usize,
    #[serde(default)]
    pub approx_saved_tokens: usize,
    pub error_count: usize,
    pub warning_count: usize,
    pub primary_failure: Option<String>,
    pub delta: Option<String>,
    pub top_errors: Vec<String>,
    #[serde(default)]
    pub top_warnings: Vec<String>,
    pub file_hits: Vec<String>,
    pub tail: Vec<String>,
    pub suggested_next_reads: Vec<String>,
    #[serde(default)]
    pub error_windows: Vec<ErrorWindow>,
    #[serde(default)]
    pub digest_error_lines: Vec<String>,
    #[serde(default)]
    pub digest_file_hits: Vec<String>,
    #[serde(default)]
    pub test_or_package_hint: Option<String>,
    pub log_path: String,
    pub previous_exact_match_run: Option<PreviousExactMatchRun>,
    pub started_at: String,
    pub command_kind: String,
    #[serde(default)]
    pub summary_budget: String,
    #[serde(default = "default_capture_mode")]
    pub capture_mode: String,
    #[serde(default)]
    pub spawn_error: Option<String>,
    #[serde(default)]
    pub runtime_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub index_schema_version: u32,
    pub run_id: String,
    pub summary_path: String,
    pub exit_code: i32,
    pub command_kind: String,
    pub command: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub started_at: String,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metrics {
    pub metrics_schema_version: u32,
    #[serde(default = "default_metrics_scope")]
    pub metrics_scope: String,
    #[serde(default)]
    pub memory_only_count: u64,
    #[serde(default)]
    pub saved_artifact_count: u64,
    pub command_count: u64,
    pub raw_line_count: u64,
    pub shown_line_count: u64,
    pub estimated_saved_lines: u64,
    #[serde(default)]
    pub raw_char_count: u64,
    #[serde(default)]
    pub shown_char_count: u64,
    #[serde(default)]
    pub estimated_saved_chars: u64,
    #[serde(default)]
    pub approx_raw_tokens: u64,
    #[serde(default)]
    pub approx_shown_tokens: u64,
    #[serde(default)]
    pub approx_saved_tokens: u64,
    pub failure_count: u64,
    pub repeated_failure_count: u64,
    #[serde(default)]
    pub repeated_failure_saved_lines: u64,
    #[serde(default)]
    pub repeated_failure_saved_chars: u64,
    pub last_command_time: Option<String>,
    pub per_command_kind: BTreeMap<String, u64>,
    #[serde(default)]
    pub per_command_kind_stats: BTreeMap<String, CommandMetrics>,
    #[serde(default)]
    pub per_command: BTreeMap<String, CommandMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandMetrics {
    pub count: u64,
    pub raw_lines: u64,
    pub shown_lines: u64,
    pub saved_lines: u64,
    pub raw_chars: u64,
    pub shown_chars: u64,
    pub saved_chars: u64,
    pub failures: u64,
    pub repeated_failures: u64,
    #[serde(default)]
    pub raw_line_samples: Vec<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct StateDiagnostics {
    pub runs_index_present: bool,
    pub runs_index_entries: usize,
    pub runs_index_malformed_lines: usize,
    pub runs_index_read_errors: usize,
    pub metrics_present: bool,
    pub metrics_valid: bool,
    pub digest_present: bool,
    pub digest_valid_json: bool,
    pub digest_shards: usize,
    pub latest_by_command_present: bool,
    pub latest_by_command_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestByCommandEntry {
    pub run_id: String,
    pub summary_path: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Default)]
pub struct LogStats {
    pub indexed_runs: usize,
    pub raw_logs: usize,
    pub summary_sidecars: usize,
    pub temp_files: usize,
    pub artifact_bytes: u64,
    pub oldest_run: Option<String>,
    pub newest_run: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GcReport {
    pub matched_artifacts: usize,
    pub matched_bytes: u64,
    pub deleted_artifacts: usize,
    pub deleted_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct StateReconciliationReport {
    pub index_entries_removed: usize,
    pub latest_entries_rebuilt: usize,
    pub digest_shards_removed: usize,
    pub unresolved_digest_refs_removed: usize,
}

#[derive(Debug)]
struct ArtifactInfo {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
}

#[derive(Debug)]
struct ArtifactGroup {
    artifacts: Vec<ArtifactInfo>,
    modified: SystemTime,
    bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct IndexDiagnostics {
    present: bool,
    entries: usize,
    malformed_lines: usize,
    read_errors: usize,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let root = if let Ok(path) = std::env::var("KDS_HOME") {
            PathBuf::from(path)
        } else {
            dirs::data_local_dir()
                .context("could not resolve local data directory")?
                .join("CodexKD")
                .join("kds")
        };
        let logs_dir = root.join("logs");
        let state_dir = root.join("state");
        Ok(Self {
            runs_index: state_dir.join("runs.jsonl"),
            digest_index: state_dir.join("digest-index.json"),
            digest_dir: state_dir.join("digest"),
            latest_by_command: state_dir.join("latest-by-command.json"),
            temp_cleanup_marker: state_dir.join("last-temp-cleanup"),
            metrics: state_dir.join("metrics.json"),
            root,
            logs_dir,
            state_dir,
        })
    }

    pub fn ensure_runtime_dirs(&self) -> Result<()> {
        create_private_dir_all(&self.root)?;
        create_private_dir_all(&self.logs_dir)?;
        create_private_dir_all(&self.state_dir)?;
        create_private_dir_all(&self.digest_dir)?;
        Ok(())
    }

    pub fn prepare_run_paths(
        &self,
        argv: &[String],
        cwd: &Path,
        started: DateTime<Local>,
    ) -> Result<RunPaths> {
        self.ensure_runtime_dirs()?;
        let date = started.format("%Y-%m-%d").to_string();
        let date_dir = self.logs_dir.join(date);
        create_private_dir_all(&date_dir)?;
        let run_id = make_run_id(argv, cwd, started);
        let log_path = date_dir.join(format!("{run_id}.log"));
        let summary_path = date_dir.join(format!("{run_id}.summary.json"));
        Ok(RunPaths {
            run_id,
            log_path,
            summary_path,
        })
    }

    pub fn cleanup_stale_temp_files(&self, stale_after: Duration) -> Result<usize> {
        if !self.logs_dir.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        cleanup_stale_temp_files_in(&self.logs_dir, stale_after, &mut removed)?;
        Ok(removed)
    }

    pub fn cleanup_stale_temp_files_amortized(
        &self,
        stale_after: Duration,
        interval: Duration,
    ) -> Result<Option<usize>> {
        if !self.logs_dir.exists() || !cleanup_due(&self.temp_cleanup_marker, interval) {
            return Ok(None);
        }
        let removed = self.cleanup_stale_temp_files(stale_after)?;
        if let Some(parent) = self.temp_cleanup_marker.parent() {
            create_private_dir_all(parent)?;
        }
        write_text_atomic(&self.temp_cleanup_marker, &iso_now())?;
        Ok(Some(removed))
    }
}

fn cleanup_due(marker: &Path, interval: Duration) -> bool {
    marker
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .map(|age| age >= interval)
        .unwrap_or(true)
}

fn default_capture_mode() -> String {
    "stdout/stderr piped".to_string()
}

fn default_metrics_scope() -> String {
    "aggregate local summaries".to_string()
}

pub fn command_string(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| render_command_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn command_identity(argv: &[String]) -> String {
    serde_json::to_string(argv).unwrap_or_else(|_| command_string(argv))
}

fn render_command_arg(arg: &str) -> String {
    if !needs_command_quote(arg) {
        return arg.to_string();
    }
    if cfg!(windows) {
        format!("'{}'", arg.replace('\'', "''"))
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn needs_command_quote(arg: &str) -> bool {
    arg.is_empty()
        || arg.chars().any(|ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '\'' | '"'
                        | '`'
                        | '$'
                        | '&'
                        | '|'
                        | ';'
                        | '<'
                        | '>'
                        | '('
                        | ')'
                        | '{'
                        | '}'
                        | '['
                        | ']'
                )
        })
}

pub fn command_kind(argv: &[String]) -> String {
    argv.first()
        .map(|first| {
            Path::new(first)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(first)
                .to_ascii_lowercase()
        })
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn resolve_command(command: &str) -> PathBuf {
    resolve_command_from(
        command,
        std::env::var_os("PATH"),
        std::env::var("PATHEXT").ok(),
    )
}

fn resolve_command_from(command: &str, path: Option<OsString>, pathext: Option<String>) -> PathBuf {
    let command_path = PathBuf::from(command);
    if is_path_like(command) {
        return resolve_path_like_command(&command_path, pathext.as_deref())
            .unwrap_or(command_path);
    }

    let Some(path) = path else {
        return command_path;
    };
    for dir in std::env::split_paths(&path) {
        for candidate_name in command_candidates(command, pathext.as_deref()) {
            let candidate = dir.join(candidate_name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    command_path
}

fn resolve_path_like_command(command: &Path, pathext: Option<&str>) -> Option<PathBuf> {
    if command.is_file() {
        return Some(command.to_path_buf());
    }
    if cfg!(windows) && command.extension().is_none() {
        for ext in pathext_extensions(pathext) {
            let candidate = command.with_extension(ext.trim_start_matches('.'));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn command_candidates(command: &str, pathext: Option<&str>) -> Vec<String> {
    if !cfg!(windows) || Path::new(command).extension().is_some() {
        return vec![command.to_string()];
    }
    pathext_extensions(pathext)
        .into_iter()
        .map(|ext| format!("{command}{ext}"))
        .collect()
}

fn pathext_extensions(pathext: Option<&str>) -> Vec<String> {
    pathext
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .filter_map(|ext| {
            let ext = ext.trim();
            if ext.is_empty() {
                None
            } else if ext.starts_with('.') {
                Some(ext.to_string())
            } else {
                Some(format!(".{ext}"))
            }
        })
        .collect()
}

fn is_path_like(command: &str) -> bool {
    command.contains('/') || command.contains('\\') || Path::new(command).is_absolute()
}

pub struct RawLog<'a> {
    pub path: &'a Path,
    pub sidecar_hint: &'a str,
    pub command: &'a str,
    pub cwd: &'a Path,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    pub exit_code: i32,
    pub elapsed: &'a str,
}

pub struct RawLogPaths<'a> {
    pub path: &'a Path,
    pub sidecar_hint: &'a str,
    pub command: &'a str,
    pub cwd: &'a Path,
    pub stdout_path: &'a Path,
    pub stderr_path: &'a Path,
    pub stdout_discarded_bytes: u64,
    pub stderr_discarded_bytes: u64,
    pub raw_byte_limit: Option<u64>,
    pub exit_code: i32,
    pub elapsed: &'a str,
}

pub fn write_raw_log(record: RawLog<'_>) -> Result<()> {
    let mut file = create_private_file_new(record.path)
        .with_context(|| format!("write raw log {}", record.path.display()))?;
    write_raw_log_header(
        &mut file,
        record.command,
        record.cwd,
        record.exit_code,
        record.elapsed,
        record.sidecar_hint,
    )?;
    file.write_all(record.stdout)?;
    if !record.stdout.ends_with(b"\n") {
        file.write_all(b"\n")?;
    }
    file.write_all(b"\n--- stderr ---\n")?;
    file.write_all(record.stderr)?;
    if !record.stderr.ends_with(b"\n") {
        file.write_all(b"\n")?;
    }
    Ok(())
}

fn cleanup_stale_temp_files_in(
    path: &Path,
    stale_after: Duration,
    removed: &mut usize,
) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry.with_context(|| format!("read {}", path.display()))?;
        let child = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", child.display()))?;
        if file_type.is_dir() {
            cleanup_stale_temp_files_in(&child, stale_after, removed)?;
            continue;
        }
        if !file_type.is_file() || child.extension().and_then(|ext| ext.to_str()) != Some("tmp") {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok());
        if age.is_some_and(|age| age >= stale_after) {
            fs::remove_file(&child).with_context(|| format!("remove {}", child.display()))?;
            *removed += 1;
        }
    }
    Ok(())
}

pub fn write_raw_log_from_paths(record: RawLogPaths<'_>) -> Result<()> {
    let mut file = create_private_file_new(record.path)
        .with_context(|| format!("write raw log {}", record.path.display()))?;
    write_raw_log_header(
        &mut file,
        record.command,
        record.cwd,
        record.exit_code,
        record.elapsed,
        record.sidecar_hint,
    )?;
    copy_file_with_trailing_newline(record.stdout_path, &mut file)?;
    write_truncation_note(
        &mut file,
        "stdout",
        record.raw_byte_limit,
        record.stdout_discarded_bytes,
    )?;
    file.write_all(b"\n--- stderr ---\n")?;
    copy_file_with_trailing_newline(record.stderr_path, &mut file)?;
    write_truncation_note(
        &mut file,
        "stderr",
        record.raw_byte_limit,
        record.stderr_discarded_bytes,
    )?;
    Ok(())
}

fn write_raw_log_header(
    file: &mut fs::File,
    command: &str,
    cwd: &Path,
    exit_code: i32,
    elapsed: &str,
    sidecar_hint: &str,
) -> Result<()> {
    write!(
        file,
        "KDS raw command log\nCommand: {}\nCWD: {}\nExit code: {}\nElapsed: {}\nSummary: {}\n\n--- stdout ---\n",
        command,
        cwd.display(),
        exit_code,
        elapsed,
        sidecar_hint
    )?;
    Ok(())
}

fn copy_file_with_trailing_newline(path: &Path, out: &mut fs::File) -> Result<()> {
    let mut input = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut last_byte = None;
    loop {
        let read = input
            .read(&mut buffer)
            .with_context(|| format!("read {}", path.display()))?;
        if read == 0 {
            break;
        }
        last_byte = Some(buffer[read - 1]);
        out.write_all(&buffer[..read])?;
    }
    if last_byte != Some(b'\n') {
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn write_truncation_note(
    file: &mut fs::File,
    stream: &str,
    raw_byte_limit: Option<u64>,
    discarded_bytes: u64,
) -> Result<()> {
    if discarded_bytes == 0 {
        return Ok(());
    }
    let limit = raw_byte_limit
        .map(|limit| limit.to_string())
        .unwrap_or_else(|| "configured limit".to_string());
    writeln!(
        file,
        "[kds: {stream} raw log capture reached {limit} bytes; discarded {discarded_bytes} additional byte(s)]"
    )?;
    Ok(())
}

pub fn write_sidecar(path: &Path, sidecar: &SummarySidecar) -> Result<()> {
    write_json_atomic(path, sidecar)
}

pub fn read_sidecar(path: &Path) -> Result<SummarySidecar> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

pub fn read_sidecar_for_display(path: &Path) -> Result<SummarySidecar> {
    let text = fs::read_to_string(path)
        .context("KDS summary sidecar is missing or unreadable; the run may have been pruned")?;
    serde_json::from_str(&text)
        .context("KDS summary sidecar is invalid; the run metadata cannot be displayed safely")
}

#[cfg(test)]
pub fn append_index(paths: &Paths, entry: &IndexEntry) -> Result<()> {
    with_state_lock(paths, || append_index_unlocked(paths, entry))
}

fn append_index_unlocked(paths: &Paths, entry: &IndexEntry) -> Result<()> {
    if let Some(parent) = paths.runs_index.parent() {
        create_private_dir_all(parent)?;
    }
    let mut file = append_private_file(&paths.runs_index)
        .with_context(|| format!("open {}", paths.runs_index.display()))?;
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub fn read_index(paths: &Paths) -> Vec<IndexEntry> {
    let (entries, diagnostics) = read_index_with_diagnostics(paths);
    if diagnostics.malformed_lines > 0 || diagnostics.read_errors > 0 {
        eprintln!(
            "kds: skipped {} malformed run index line(s) and {} unreadable line(s)",
            diagnostics.malformed_lines, diagnostics.read_errors
        );
    }
    entries
}

fn read_index_with_diagnostics(paths: &Paths) -> (Vec<IndexEntry>, IndexDiagnostics) {
    let file = match fs::File::open(&paths.runs_index) {
        Ok(file) => file,
        Err(_) => return (Vec::new(), IndexDiagnostics::default()),
    };
    let mut entries = Vec::new();
    let mut diagnostics = IndexDiagnostics {
        present: true,
        ..IndexDiagnostics::default()
    };
    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                diagnostics.read_errors += 1;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<IndexEntry>(&line) {
            Ok(entry) => {
                diagnostics.entries += 1;
                entries.push(entry);
            }
            Err(_) => diagnostics.malformed_lines += 1,
        }
    }
    (entries, diagnostics)
}

pub fn state_diagnostics(paths: &Paths) -> StateDiagnostics {
    let (_, index) = read_index_with_diagnostics(paths);
    let (metrics_present, metrics_valid) = json_file_valid::<Metrics>(&paths.metrics);
    let (digest_present, digest_valid_json) = json_value_file_valid(&paths.digest_index);
    let digest_shards = count_digest_shards(&paths.digest_dir);
    let (latest_by_command_present, latest_by_command_valid) =
        json_file_valid::<BTreeMap<String, LatestByCommandEntry>>(&paths.latest_by_command);
    StateDiagnostics {
        runs_index_present: index.present,
        runs_index_entries: index.entries,
        runs_index_malformed_lines: index.malformed_lines,
        runs_index_read_errors: index.read_errors,
        metrics_present,
        metrics_valid,
        digest_present: digest_present || digest_shards > 0,
        digest_valid_json: digest_valid_json || !digest_present,
        digest_shards,
        latest_by_command_present,
        latest_by_command_valid,
    }
}

fn count_digest_shards(path: &Path) -> usize {
    if !path.exists() {
        return 0;
    }
    let mut count = 0;
    let Ok(first_level) = fs::read_dir(path) else {
        return 0;
    };
    for entry in first_level.flatten() {
        let child = entry.path();
        if child.is_dir() {
            if let Ok(files) = fs::read_dir(&child) {
                count += files
                    .flatten()
                    .filter(|entry| {
                        entry.path().extension().and_then(|ext| ext.to_str()) == Some("json")
                    })
                    .count();
            }
        }
    }
    count
}

fn digest_state_paths(paths: &Paths) -> Vec<PathBuf> {
    if !paths.digest_dir.exists() {
        return Vec::new();
    }
    let Ok(first_level) = fs::read_dir(&paths.digest_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in first_level.flatten() {
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }
        if let Ok(files) = fs::read_dir(&child) {
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

pub fn log_stats(paths: &Paths) -> Result<LogStats> {
    let entries = read_index(paths);
    let mut stats = LogStats {
        indexed_runs: entries.len(),
        oldest_run: entries.first().map(|entry| entry.started_at.clone()),
        newest_run: entries.last().map(|entry| entry.started_at.clone()),
        ..LogStats::default()
    };
    if paths.logs_dir.exists() {
        scan_log_stats(&paths.logs_dir, &mut stats)?;
    }
    Ok(stats)
}

fn scan_log_stats(path: &Path, stats: &mut LogStats) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry.with_context(|| format!("read {}", path.display()))?;
        let child = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", child.display()))?;
        if file_type.is_dir() {
            scan_log_stats(&child, stats)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(kind) = kds_artifact_kind(&child) else {
            continue;
        };
        stats.artifact_bytes += entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        match kind {
            ArtifactKind::RawLog => stats.raw_logs += 1,
            ArtifactKind::SummarySidecar => stats.summary_sidecars += 1,
            ArtifactKind::Temp => stats.temp_files += 1,
        }
    }
    Ok(())
}

pub fn gc_artifacts(paths: &Paths, older_than: Duration, dry_run: bool) -> Result<GcReport> {
    let mut report = GcReport::default();
    if !paths.logs_dir.exists() {
        return Ok(report);
    }
    let root = paths
        .logs_dir
        .canonicalize()
        .with_context(|| format!("resolve {}", paths.logs_dir.display()))?;
    gc_artifacts_in(&root, &root, older_than, dry_run, &mut report)?;
    if !dry_run {
        remove_empty_log_dirs(&root, &root)?;
    }
    Ok(report)
}

pub fn prune_to_max_artifact_bytes(
    paths: &Paths,
    max_bytes: u64,
    dry_run: bool,
) -> Result<GcReport> {
    let mut report = GcReport::default();
    if !paths.logs_dir.exists() {
        return Ok(report);
    }
    let root = paths
        .logs_dir
        .canonicalize()
        .with_context(|| format!("resolve {}", paths.logs_dir.display()))?;
    let mut artifacts = Vec::new();
    collect_artifacts(&root, &root, &mut artifacts)?;
    let total: u64 = artifacts.iter().map(|artifact| artifact.bytes).sum();
    if total <= max_bytes {
        return Ok(report);
    }
    let mut groups = artifact_groups(artifacts);
    groups.sort_by_key(|group| group.modified);
    let mut remaining = total;
    for group in groups {
        if remaining <= max_bytes {
            break;
        }
        report.matched_artifacts += group.artifacts.len();
        report.matched_bytes += group.bytes;
        remaining = remaining.saturating_sub(group.bytes);
        if !dry_run {
            for artifact in group.artifacts {
                fs::remove_file(&artifact.path)
                    .with_context(|| format!("remove {}", artifact.path.display()))?;
                report.deleted_artifacts += 1;
                report.deleted_bytes += artifact.bytes;
            }
        }
    }
    if !dry_run {
        remove_empty_log_dirs(&root, &root)?;
    }
    Ok(report)
}

pub fn reconcile_state_after_artifact_cleanup(paths: &Paths) -> Result<StateReconciliationReport> {
    with_state_lock(paths, || {
        reconcile_state_after_artifact_cleanup_unlocked(paths)
    })
}

fn reconcile_state_after_artifact_cleanup_unlocked(
    paths: &Paths,
) -> Result<StateReconciliationReport> {
    let (entries, diagnostics) = read_index_with_diagnostics(paths);
    let original_entries = entries.len();
    let mut retained = Vec::new();
    let mut latest = BTreeMap::new();

    for entry in entries {
        if read_sidecar(Path::new(&entry.summary_path)).is_err() {
            continue;
        }
        latest.insert(
            command_cache_key(&entry.argv, &entry.cwd),
            LatestByCommandEntry {
                run_id: entry.run_id.clone(),
                summary_path: entry.summary_path.clone(),
                exit_code: entry.exit_code,
            },
        );
        retained.push(entry);
    }

    if diagnostics.present
        && (retained.len() != original_entries
            || diagnostics.malformed_lines > 0
            || diagnostics.read_errors > 0)
    {
        write_index_unlocked(paths, &retained)?;
    }
    if paths.latest_by_command.exists() || !retained.is_empty() {
        write_latest_by_command(paths, &latest)?;
    }

    let digest_shards_removed = retire_digest_shards_with_missing_logs(paths)?;
    let unresolved_digest_refs_removed = reconcile_unresolved_digest_refs(paths)?;

    Ok(StateReconciliationReport {
        index_entries_removed: original_entries.saturating_sub(retained.len()),
        latest_entries_rebuilt: latest.len(),
        digest_shards_removed,
        unresolved_digest_refs_removed,
    })
}

fn write_index_unlocked(paths: &Paths, entries: &[IndexEntry]) -> Result<()> {
    if let Some(parent) = paths.runs_index.parent() {
        create_private_dir_all(parent)?;
    }
    let mut text = String::new();
    for entry in entries {
        text.push_str(&serde_json::to_string(entry)?);
        text.push('\n');
    }
    write_text_atomic(&paths.runs_index, &text)
}

fn retire_digest_shards_with_missing_logs(paths: &Paths) -> Result<usize> {
    let mut removed = 0;
    for path in digest_state_paths(paths) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(log_path) = value
            .get("previous_log_path")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if log_path.trim().is_empty() || log_artifact_retained(Path::new(log_path)) {
            continue;
        }
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        removed += 1;
    }
    if removed > 0 && paths.digest_dir.exists() {
        remove_empty_log_dirs(&paths.digest_dir, &paths.digest_dir)?;
    }
    Ok(removed)
}

fn log_artifact_retained(path: &Path) -> bool {
    path.is_file() || compressed_log_path(path).is_some_and(|path| path.is_file())
}

fn compressed_log_path(path: &Path) -> Option<PathBuf> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("log") {
        Some(path.with_extension("log.gz"))
    } else {
        None
    }
}

fn reconcile_unresolved_digest_refs(paths: &Paths) -> Result<usize> {
    let dir = paths.state_dir.join("unresolved-by-command");
    if !dir.exists() {
        return Ok(0);
    }

    let mut removed = 0;
    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read {}", dir.display()))?;
        let path = entry.path();
        if !entry
            .file_type()
            .with_context(|| format!("stat {}", path.display()))?
            .is_file()
            || path.extension().and_then(|ext| ext.to_str()) != Some("json")
        {
            continue;
        }
        let Some(values) = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        else {
            continue;
        };
        let original_len = values.len();
        let retained = values
            .into_iter()
            .filter(|digest_path| Path::new(digest_path).is_file())
            .collect::<Vec<_>>();
        if retained.len() == original_len {
            continue;
        }
        removed += original_len - retained.len();
        write_json_atomic(&path, &retained)?;
    }
    Ok(removed)
}

pub fn compress_artifacts_older_than(paths: &Paths, older_than: Duration) -> Result<GcReport> {
    let mut report = GcReport::default();
    if !paths.logs_dir.exists() {
        return Ok(report);
    }
    let root = paths
        .logs_dir
        .canonicalize()
        .with_context(|| format!("resolve {}", paths.logs_dir.display()))?;
    compress_artifacts_in(&root, &root, older_than, &mut report)?;
    Ok(report)
}

fn compress_artifacts_in(
    root: &Path,
    path: &Path,
    older_than: Duration,
    report: &mut GcReport,
) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry.with_context(|| format!("read {}", path.display()))?;
        let child = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", child.display()))?;
        if file_type.is_dir() {
            compress_artifacts_in(root, &child, older_than, report)?;
            continue;
        }
        if !file_type.is_file() || child.extension().and_then(|ext| ext.to_str()) != Some("log") {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("stat {}", child.display()))?;
        let age = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok());
        if age.is_none_or(|age| age < older_than) {
            continue;
        }
        let canonical = child
            .canonicalize()
            .with_context(|| format!("resolve {}", child.display()))?;
        if !canonical.starts_with(root) {
            anyhow::bail!(
                "refusing to compress artifact outside logs dir: {}",
                child.display()
            );
        }
        let gz_path = canonical.with_extension("log.gz");
        if gz_path.exists() {
            continue;
        }
        compress_file(&canonical, &gz_path)?;
        fs::remove_file(&canonical).with_context(|| format!("remove {}", canonical.display()))?;
        update_sidecar_log_path_after_compress(&canonical, &gz_path);
        report.matched_artifacts += 1;
        report.deleted_artifacts += 1;
        report.matched_bytes += metadata.len();
        report.deleted_bytes += metadata.len().saturating_sub(
            gz_path
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or_default(),
        );
    }
    Ok(())
}

fn compress_file(path: &Path, gz_path: &Path) -> Result<()> {
    let mut input = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    let output = create_private_file_new(gz_path)
        .with_context(|| format!("create {}", gz_path.display()))?;
    let mut encoder = GzEncoder::new(output, Compression::default());
    std::io::copy(&mut input, &mut encoder)
        .with_context(|| format!("compress {}", path.display()))?;
    let output = encoder.finish()?;
    if durable_state_writes_enabled() {
        output
            .sync_all()
            .with_context(|| format!("sync {}", gz_path.display()))?;
    }
    Ok(())
}

fn update_sidecar_log_path_after_compress(log_path: &Path, gz_path: &Path) {
    let Some(file_name) = log_path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let Some(stem) = file_name.strip_suffix(".log") else {
        return;
    };
    let sidecar_path = log_path.with_file_name(format!("{stem}.summary.json"));
    let Ok(mut sidecar) = read_sidecar(&sidecar_path) else {
        return;
    };
    sidecar.log_path = gz_path.display().to_string();
    let _ = write_sidecar(&sidecar_path, &sidecar);
}

fn durable_state_writes_enabled() -> bool {
    matches!(
        std::env::var("KDS_DURABLE_LOGS")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn collect_artifacts(root: &Path, path: &Path, artifacts: &mut Vec<ArtifactInfo>) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry.with_context(|| format!("read {}", path.display()))?;
        let child = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", child.display()))?;
        if file_type.is_dir() {
            collect_artifacts(root, &child, artifacts)?;
            continue;
        }
        if !file_type.is_file() || kds_artifact_kind(&child).is_none() {
            continue;
        }
        let canonical = child
            .canonicalize()
            .with_context(|| format!("resolve {}", child.display()))?;
        if !canonical.starts_with(root) {
            anyhow::bail!(
                "refusing to inspect artifact outside logs dir: {}",
                child.display()
            );
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("stat {}", child.display()))?;
        artifacts.push(ArtifactInfo {
            path: canonical,
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            bytes: metadata.len(),
        });
    }
    Ok(())
}

fn artifact_groups(artifacts: Vec<ArtifactInfo>) -> Vec<ArtifactGroup> {
    let mut grouped: BTreeMap<PathBuf, Vec<ArtifactInfo>> = BTreeMap::new();
    for artifact in artifacts {
        grouped
            .entry(artifact_group_key(&artifact.path))
            .or_default()
            .push(artifact);
    }
    grouped
        .into_values()
        .map(|artifacts| {
            let modified = artifacts
                .iter()
                .map(|artifact| artifact.modified)
                .min()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let bytes = artifacts.iter().map(|artifact| artifact.bytes).sum();
            ArtifactGroup {
                artifacts,
                modified,
                bytes,
            }
        })
        .collect()
}

fn artifact_group_key(path: &Path) -> PathBuf {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.to_path_buf();
    };
    if let Some(base) = artifact_temp_base(file_name) {
        return artifact_group_key(&path.with_file_name(base));
    }
    let stem = file_name
        .strip_suffix(".summary.json")
        .or_else(|| file_name.strip_suffix(".log.gz"))
        .or_else(|| file_name.strip_suffix(".log"))
        .or_else(|| file_name.strip_suffix(".tmp"))
        .unwrap_or(file_name);
    path.with_file_name(stem)
}

fn artifact_temp_base(file_name: &str) -> Option<&str> {
    let without_tmp = file_name.strip_suffix(".tmp")?;
    let (before_unique, unique) = without_tmp.rsplit_once('.')?;
    let (base, pid) = before_unique.rsplit_once('.')?;
    if pid.chars().all(|ch| ch.is_ascii_digit())
        && unique.chars().all(|ch| ch.is_ascii_digit())
        && !base.is_empty()
    {
        Some(base)
    } else {
        None
    }
}

fn gc_artifacts_in(
    root: &Path,
    path: &Path,
    older_than: Duration,
    dry_run: bool,
    report: &mut GcReport,
) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry.with_context(|| format!("read {}", path.display()))?;
        let child = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", child.display()))?;
        if file_type.is_dir() {
            gc_artifacts_in(root, &child, older_than, dry_run, report)?;
            continue;
        }
        if !file_type.is_file() || kds_artifact_kind(&child).is_none() {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("stat {}", child.display()))?;
        let age = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok());
        if age.is_none_or(|age| age < older_than) {
            continue;
        }
        let canonical = child
            .canonicalize()
            .with_context(|| format!("resolve {}", child.display()))?;
        if !canonical.starts_with(root) {
            anyhow::bail!(
                "refusing to remove artifact outside logs dir: {}",
                child.display()
            );
        }
        report.matched_artifacts += 1;
        report.matched_bytes += metadata.len();
        if !dry_run {
            fs::remove_file(&canonical).with_context(|| format!("remove {}", child.display()))?;
            report.deleted_artifacts += 1;
            report.deleted_bytes += metadata.len();
        }
    }
    Ok(())
}

fn remove_empty_log_dirs(root: &Path, path: &Path) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry.with_context(|| format!("read {}", path.display()))?;
        let child = entry.path();
        if entry
            .file_type()
            .with_context(|| format!("stat {}", child.display()))?
            .is_dir()
        {
            remove_empty_log_dirs(root, &child)?;
        }
    }
    if path != root && fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ArtifactKind {
    RawLog,
    SummarySidecar,
    Temp,
}

fn kds_artifact_kind(path: &Path) -> Option<ArtifactKind> {
    let file_name = path.file_name()?.to_str()?;
    if file_name.ends_with(".summary.json") {
        return Some(ArtifactKind::SummarySidecar);
    }
    if file_name.ends_with(".log.gz") {
        return Some(ArtifactKind::RawLog);
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("log") => Some(ArtifactKind::RawLog),
        Some("tmp") => Some(ArtifactKind::Temp),
        _ => None,
    }
}

fn json_file_valid<T: for<'de> Deserialize<'de>>(path: &Path) -> (bool, bool) {
    match fs::read_to_string(path) {
        Ok(text) => (true, serde_json::from_str::<T>(&text).is_ok()),
        Err(_) => (false, false),
    }
}

fn json_value_file_valid(path: &Path) -> (bool, bool) {
    match fs::read_to_string(path) {
        Ok(text) => (
            true,
            serde_json::from_str::<serde_json::Value>(&text).is_ok(),
        ),
        Err(_) => (false, false),
    }
}

pub fn resolve_run_id(paths: &Paths, query: &str) -> Result<IndexEntry> {
    let matches: Vec<IndexEntry> = read_index(paths)
        .into_iter()
        .filter(|entry| entry.run_id.starts_with(query) || run_hash(&entry.run_id) == Some(query))
        .collect();
    match matches.len() {
        0 => anyhow::bail!("run id `{query}` not found"),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => {
            eprintln!("kds: run id prefix `{query}` is ambiguous; use a longer prefix");
            for entry in matches.iter().take(20) {
                eprintln!("  {}", entry.run_id);
            }
            anyhow::bail!("ambiguous run id")
        }
    }
}

fn run_hash(run_id: &str) -> Option<&str> {
    run_id.rsplit_once('-').map(|(_, hash)| hash)
}

pub fn last_run(paths: &Paths) -> Result<IndexEntry> {
    read_index(paths)
        .into_iter()
        .rev()
        .find(|entry| read_sidecar(Path::new(&entry.summary_path)).is_ok())
        .context("no readable KDS runs found")
}

pub fn previous_exact_match_with_sidecar(
    paths: &Paths,
    argv: &[String],
    cwd: &str,
) -> Option<(PreviousExactMatchRun, Option<SummarySidecar>)> {
    let mut fallback_without_sidecar = None;
    let key = command_cache_key(argv, cwd);
    if let Some(entry) = load_latest_by_command(paths).remove(&key) {
        let sidecar = read_sidecar(Path::new(&entry.summary_path)).ok();
        let previous = PreviousExactMatchRun {
            run_id: entry.run_id,
            exit_code: entry.exit_code,
            digest: sidecar
                .as_ref()
                .map(|sidecar| sidecar.digest.clone())
                .unwrap_or_default(),
            summary_path: entry.summary_path,
        };
        if sidecar.is_some() {
            return Some((previous, sidecar));
        }
        fallback_without_sidecar = Some((previous, None));
    }

    for entry in read_index(paths)
        .into_iter()
        .rev()
        .filter(|entry| entry.argv == argv && entry.cwd == cwd)
    {
        let sidecar = read_sidecar(Path::new(&entry.summary_path)).ok();
        let previous = previous_match_from_entry(entry, sidecar.as_ref());
        if sidecar.is_some() {
            return Some((previous, sidecar));
        }
        if fallback_without_sidecar.is_none() {
            fallback_without_sidecar = Some((previous, None));
        }
    }
    fallback_without_sidecar
}

fn previous_match_from_entry(
    entry: IndexEntry,
    sidecar: Option<&SummarySidecar>,
) -> PreviousExactMatchRun {
    PreviousExactMatchRun {
        run_id: entry.run_id,
        exit_code: entry.exit_code,
        digest: sidecar
            .map(|sidecar| sidecar.digest.clone())
            .unwrap_or_default(),
        summary_path: entry.summary_path,
    }
}

pub fn command_cache_key(argv: &[String], cwd: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command_identity(argv).as_bytes());
    hasher.update(b"\0");
    hasher.update(cwd.as_bytes());
    crate::hash::sha256_finalize_hex(hasher)
}

fn load_latest_by_command(paths: &Paths) -> BTreeMap<String, LatestByCommandEntry> {
    fs::read_to_string(&paths.latest_by_command)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_latest_by_command(
    paths: &Paths,
    latest: &BTreeMap<String, LatestByCommandEntry>,
) -> Result<()> {
    write_json_atomic(&paths.latest_by_command, latest)
}

pub fn load_metrics(paths: &Paths) -> Metrics {
    fs::read_to_string(&paths.metrics)
        .ok()
        .and_then(|text| serde_json::from_str::<Metrics>(&text).ok())
        .unwrap_or_else(|| Metrics {
            metrics_schema_version: METRICS_SCHEMA_VERSION,
            metrics_scope: default_metrics_scope(),
            ..Metrics::default()
        })
}

pub fn write_metrics(paths: &Paths, metrics: &Metrics) -> Result<()> {
    if let Some(parent) = paths.metrics.parent() {
        create_private_dir_all(parent)?;
    }
    write_json_atomic(&paths.metrics, metrics)
}

pub fn record_run_state_unlocked(
    paths: &Paths,
    entry: &IndexEntry,
    sidecar: &SummarySidecar,
) -> Result<()> {
    append_index_unlocked(paths, entry)?;
    let mut latest = load_latest_by_command(paths);
    latest.insert(
        command_cache_key(&entry.argv, &entry.cwd),
        LatestByCommandEntry {
            run_id: entry.run_id.clone(),
            summary_path: entry.summary_path.clone(),
            exit_code: entry.exit_code,
        },
    );
    write_latest_by_command(paths, &latest)?;

    let mut metrics = load_metrics(paths);
    update_metrics_for_sidecar(&mut metrics, sidecar, MetricRecordKind::SavedArtifact);
    write_metrics(paths, &metrics)
}

pub fn record_metric_only(paths: &Paths, sidecar: &SummarySidecar) -> Result<()> {
    let mut metrics = load_metrics(paths);
    update_metrics_for_sidecar(&mut metrics, sidecar, MetricRecordKind::MemoryOnly);
    write_metrics(paths, &metrics)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricRecordKind {
    MemoryOnly,
    SavedArtifact,
}

fn update_metrics_for_sidecar(
    metrics: &mut Metrics,
    sidecar: &SummarySidecar,
    record_kind: MetricRecordKind,
) {
    metrics.metrics_schema_version = METRICS_SCHEMA_VERSION;
    metrics.metrics_scope = default_metrics_scope();
    metrics.command_count += 1;
    match record_kind {
        MetricRecordKind::MemoryOnly => metrics.memory_only_count += 1,
        MetricRecordKind::SavedArtifact => metrics.saved_artifact_count += 1,
    }
    metrics.raw_line_count += sidecar.raw_total_lines as u64;
    metrics.shown_line_count += sidecar.shown_lines as u64;
    metrics.estimated_saved_lines += sidecar.estimated_saved_lines as u64;
    metrics.raw_char_count += sidecar.raw_total_chars as u64;
    metrics.shown_char_count += sidecar.shown_chars as u64;
    metrics.estimated_saved_chars += sidecar.estimated_saved_chars as u64;
    metrics.approx_raw_tokens += sidecar.approx_raw_tokens as u64;
    metrics.approx_shown_tokens += sidecar.approx_shown_tokens as u64;
    metrics.approx_saved_tokens += sidecar.approx_saved_tokens as u64;
    if sidecar.exit_code != 0 {
        metrics.failure_count += 1;
    }
    if sidecar.repeat_status.is_repeat {
        metrics.repeated_failure_count += 1;
        metrics.repeated_failure_saved_lines += sidecar.estimated_saved_lines as u64;
        metrics.repeated_failure_saved_chars += sidecar.estimated_saved_chars as u64;
    }
    metrics.last_command_time = Some(sidecar.started_at.clone());
    *metrics
        .per_command_kind
        .entry(sidecar.command_kind.clone())
        .or_insert(0) += 1;
    update_command_metrics(
        metrics
            .per_command_kind_stats
            .entry(sidecar.command_kind.clone())
            .or_default(),
        sidecar,
    );
    if record_kind == MetricRecordKind::SavedArtifact {
        update_command_metrics(
            metrics
                .per_command
                .entry(sidecar.command.clone())
                .or_default(),
            sidecar,
        );
    }
}

fn update_command_metrics(metric: &mut CommandMetrics, sidecar: &SummarySidecar) {
    metric.count += 1;
    metric.raw_lines += sidecar.raw_total_lines as u64;
    metric.shown_lines += sidecar.shown_lines as u64;
    metric.saved_lines += sidecar.estimated_saved_lines as u64;
    metric.raw_chars += sidecar.raw_total_chars as u64;
    metric.shown_chars += sidecar.shown_chars as u64;
    metric.saved_chars += sidecar.estimated_saved_chars as u64;
    if sidecar.exit_code != 0 {
        metric.failures += 1;
    }
    if sidecar.repeat_status.is_repeat {
        metric.repeated_failures += 1;
    }
    metric.raw_line_samples.push(sidecar.raw_total_lines as u64);
    if metric.raw_line_samples.len() > 200 {
        metric.raw_line_samples.remove(0);
    }
}

pub fn with_state_lock<T>(paths: &Paths, action: impl FnOnce() -> Result<T>) -> Result<T> {
    create_private_dir_all(&paths.state_dir)?;
    let _guard = StateLock::acquire(paths.state_dir.join("state.lock"))?;
    action()
}

struct StateLock {
    path: PathBuf,
}

impl StateLock {
    fn acquire(path: PathBuf) -> Result<Self> {
        let start = Instant::now();
        loop {
            match create_private_file_new(&path) {
                Ok(mut file) => {
                    writeln!(file, "pid={}", std::process::id())
                        .with_context(|| format!("write {}", path.display()))?;
                    return Ok(Self { path });
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    if is_stale_lock(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if start.elapsed() > Duration::from_secs(10) {
                        anyhow::bail!("timed out waiting for state lock {}", path.display());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(err) => {
                    return Err(err).with_context(|| format!("create {}", path.display()));
                }
            }
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn is_stale_lock(path: &Path) -> bool {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > Duration::from_secs(600))
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir_all(parent)?;
    }
    let tmp = tmp_path(
        path,
        path.extension().and_then(|e| e.to_str()).unwrap_or("json"),
    );
    let text = serde_json::to_string_pretty(value)?;
    write_private_atomic_bytes(path, &tmp, text.as_bytes())?;
    Ok(())
}

pub fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir_all(parent)?;
    }
    let tmp = tmp_path(
        path,
        path.extension().and_then(|e| e.to_str()).unwrap_or("txt"),
    );
    write_private_atomic_bytes(path, &tmp, text.as_bytes())?;
    Ok(())
}

pub fn create_temp_file_near(path: &Path, label: &str) -> Result<(PathBuf, fs::File)> {
    let tmp = tmp_path(path, label);
    if let Some(parent) = tmp.parent() {
        create_private_dir_all(parent)?;
    }
    let file =
        create_private_file_new(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    Ok((tmp, file))
}

fn write_private_atomic_bytes(path: &Path, tmp: &Path, bytes: &[u8]) -> Result<()> {
    let mut file =
        create_private_file_new(tmp).with_context(|| format!("write {}", tmp.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write {}", tmp.display()))?;
    file.sync_all()
        .with_context(|| format!("sync {}", tmp.display()))?;
    replace_file(tmp, path)
}

fn replace_file(tmp: &Path, path: &Path) -> Result<()> {
    replace_file_impl(tmp, path)
}

#[cfg(windows)]
fn replace_file_impl(tmp: &Path, path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let tmp_wide = wide(tmp);
    let path_wide = wide(path);
    let ok = unsafe {
        MoveFileExW(
            tmp_wide.as_ptr(),
            path_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    Err(std::io::Error::last_os_error())
        .with_context(|| format!("replace {} with {}", path.display(), tmp.display()))
}

#[cfg(not(windows))]
fn replace_file_impl(tmp: &Path, path: &Path) -> Result<()> {
    fs::rename(tmp, path).with_context(|| format!("rename {} to {}", tmp.display(), path.display()))
}

fn create_private_file_new(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn append_private_file(path: &Path) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn create_private_dir_all(path: &Path) -> Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    harden_private_dir(path, existed)
}

#[cfg(unix)]
fn harden_private_dir(path: &Path, existed: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let mode = metadata.permissions().mode();
    if existed && mode & 0o002 != 0 {
        anyhow::bail!(
            "refusing world-writable KDS storage directory {}",
            path.display()
        );
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("chmod 700 {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_private_dir(_path: &Path, _existed: bool) -> Result<()> {
    Ok(())
}

fn tmp_path(path: &Path, fallback_extension: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_extension(format!(
        "{fallback_extension}.{}.{}.tmp",
        std::process::id(),
        unique
    ))
}

fn make_run_id(argv: &[String], cwd: &Path, started: DateTime<Local>) -> String {
    let stamp = started.format("%Y-%m-%d-%H%M%S").to_string();
    let slug = make_slug(argv);
    let mut hasher = Sha256::new();
    hasher.update(command_identity(argv).as_bytes());
    hasher.update(cwd.to_string_lossy().as_bytes());
    hasher.update(
        started
            .to_rfc3339_opts(SecondsFormat::Nanos, true)
            .as_bytes(),
    );
    let hash = crate::hash::sha256_finalize_hex(hasher);
    format!("{stamp}-{slug}-{}", &hash[..6])
}

fn make_slug(argv: &[String]) -> String {
    let source = argv.iter().take(3).cloned().collect::<Vec<_>>().join("-");
    let mut out = String::new();
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "command".to_string()
    } else {
        trimmed
    }
}

pub fn line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

pub fn display_percent(saved: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (saved as f64 / total as f64) * 100.0
    }
}

pub fn iso_now() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_rendering_quotes_display_and_identity_preserves_argv_boundaries() {
        let argv = vec![
            "tool".to_string(),
            "two words".to_string(),
            "plain".to_string(),
        ];
        let rendered = command_string(&argv);
        assert!(rendered.contains("'two words'"), "rendered: {rendered}");

        let combined = vec!["tool".to_string(), "a b".to_string()];
        let split = vec!["tool".to_string(), "a".to_string(), "b".to_string()];
        assert_ne!(command_identity(&combined), command_identity(&split));
    }

    #[test]
    fn prefix_resolution_handles_zero_one_many() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("KDS_HOME", dir.path());
        let paths = Paths::discover().unwrap();
        paths.ensure_runtime_dirs().unwrap();
        let first = IndexEntry {
            index_schema_version: INDEX_SCHEMA_VERSION,
            run_id: "2026-01-01-010101-node-version-a1b2c3".into(),
            summary_path: "one.summary.json".into(),
            exit_code: 0,
            command_kind: "node".into(),
            command: "node --version".into(),
            argv: vec!["node".into(), "--version".into()],
            cwd: "C:/tmp".into(),
            started_at: iso_now(),
            log_path: "one.log".into(),
        };
        let mut second = first.clone();
        second.run_id = "2026-01-01-010102-node-version-d4e5f6".into();
        append_index(&paths, &first).unwrap();
        append_index(&paths, &second).unwrap();

        assert!(resolve_run_id(&paths, "missing").is_err());
        assert_eq!(
            resolve_run_id(&paths, "a1b2c3").unwrap().run_id,
            first.run_id
        );
        assert!(resolve_run_id(&paths, "1b2c3").is_err());
        assert!(resolve_run_id(&paths, "node-version").is_err());
    }

    #[test]
    fn malformed_index_lines_are_skipped_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("KDS_HOME", dir.path());
        let paths = Paths::discover().unwrap();
        paths.ensure_runtime_dirs().unwrap();
        let entry = IndexEntry {
            index_schema_version: INDEX_SCHEMA_VERSION,
            run_id: "2026-01-01-010101-node-version-a1b2c3".into(),
            summary_path: "one.summary.json".into(),
            exit_code: 0,
            command_kind: "node".into(),
            command: "node --version".into(),
            argv: vec!["node".into(), "--version".into()],
            cwd: "C:/tmp".into(),
            started_at: iso_now(),
            log_path: "one.log".into(),
        };
        append_index(&paths, &entry).unwrap();
        {
            let mut file = OpenOptions::new()
                .append(true)
                .open(&paths.runs_index)
                .unwrap();
            writeln!(file, "{{not valid json").unwrap();
        }

        let entries = read_index(&paths);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].run_id, entry.run_id);
        let diagnostics = state_diagnostics(&paths);
        assert!(diagnostics.runs_index_present);
        assert_eq!(diagnostics.runs_index_entries, 1);
        assert_eq!(diagnostics.runs_index_malformed_lines, 1);
    }

    #[test]
    fn previous_exact_match_skips_stale_latest_cache() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(dir.path());
        paths.ensure_runtime_dirs().unwrap();
        let day = paths.logs_dir.join("2026-01-01");
        fs::create_dir_all(&day).unwrap();
        let argv = vec!["cargo".to_string(), "test".to_string()];
        let cwd = "C:/repo";
        let old_summary = day.join("old.summary.json");
        let missing_summary = day.join("missing.summary.json");
        write_sidecar(&old_summary, &test_sidecar("old-run", "old-digest")).unwrap();

        let old_entry = test_index_entry("old-run", &old_summary, &argv, cwd, 1);
        let stale_entry = test_index_entry("stale-run", &missing_summary, &argv, cwd, 1);
        append_index(&paths, &old_entry).unwrap();
        append_index(&paths, &stale_entry).unwrap();

        let mut latest = BTreeMap::new();
        latest.insert(
            command_cache_key(&argv, cwd),
            LatestByCommandEntry {
                run_id: "stale-run".into(),
                summary_path: missing_summary.display().to_string(),
                exit_code: 1,
            },
        );
        write_latest_by_command(&paths, &latest).unwrap();

        let (previous, sidecar) = previous_exact_match_with_sidecar(&paths, &argv, cwd).unwrap();
        assert_eq!(previous.run_id, "old-run");
        assert_eq!(previous.digest, "old-digest");
        assert_eq!(sidecar.unwrap().digest, "old-digest");
    }

    #[test]
    fn last_run_skips_entries_without_readable_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(dir.path());
        paths.ensure_runtime_dirs().unwrap();
        let day = paths.logs_dir.join("2026-01-01");
        fs::create_dir_all(&day).unwrap();
        let argv = vec!["cargo".to_string(), "test".to_string()];
        let old_summary = day.join("old.summary.json");
        let missing_summary = day.join("missing.summary.json");
        write_sidecar(&old_summary, &test_sidecar("old-run", "old-digest")).unwrap();

        append_index(
            &paths,
            &test_index_entry("old-run", &old_summary, &argv, "C:/repo", 1),
        )
        .unwrap();
        append_index(
            &paths,
            &test_index_entry("stale-run", &missing_summary, &argv, "C:/repo", 1),
        )
        .unwrap();

        assert_eq!(last_run(&paths).unwrap().run_id, "old-run");
    }

    #[test]
    fn reconcile_state_after_cleanup_removes_stale_artifact_refs() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(dir.path());
        paths.ensure_runtime_dirs().unwrap();
        let day = paths.logs_dir.join("2026-01-01");
        fs::create_dir_all(&day).unwrap();
        let argv = vec!["cargo".to_string(), "test".to_string()];
        let retained_summary = day.join("retained.summary.json");
        let missing_summary = day.join("missing.summary.json");
        write_sidecar(
            &retained_summary,
            &test_sidecar("retained-run", "retained-digest"),
        )
        .unwrap();

        let retained_entry =
            test_index_entry("retained-run", &retained_summary, &argv, "C:/repo", 1);
        let stale_entry = test_index_entry("stale-run", &missing_summary, &argv, "C:/repo", 1);
        append_index(&paths, &retained_entry).unwrap();
        append_index(&paths, &stale_entry).unwrap();

        let mut latest = BTreeMap::new();
        latest.insert(
            command_cache_key(&argv, "C:/repo"),
            LatestByCommandEntry {
                run_id: "stale-run".into(),
                summary_path: missing_summary.display().to_string(),
                exit_code: 1,
            },
        );
        write_latest_by_command(&paths, &latest).unwrap();

        let digest_path = paths.digest_dir.join("aa").join("aa1111.json");
        write_json_atomic(
            &digest_path,
            &serde_json::json!({
                "digest": "aa1111",
                "previous_log_path": day.join("missing.log").display().to_string()
            }),
        )
        .unwrap();
        let unresolved = paths
            .state_dir
            .join("unresolved-by-command")
            .join("cmd.json");
        write_json_atomic(
            &unresolved,
            &vec![
                digest_path.display().to_string(),
                paths
                    .digest_dir
                    .join("bb")
                    .join("missing.json")
                    .display()
                    .to_string(),
            ],
        )
        .unwrap();

        let report = reconcile_state_after_artifact_cleanup(&paths).unwrap();

        assert_eq!(report.index_entries_removed, 1);
        assert_eq!(report.latest_entries_rebuilt, 1);
        assert_eq!(report.digest_shards_removed, 1);
        assert_eq!(report.unresolved_digest_refs_removed, 2);
        let entries = read_index(&paths);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].run_id, "retained-run");
        let latest = load_latest_by_command(&paths);
        assert_eq!(
            latest
                .get(&command_cache_key(&argv, "C:/repo"))
                .unwrap()
                .run_id,
            "retained-run"
        );
        assert!(!digest_path.exists());
    }

    #[test]
    fn invalid_metrics_and_digest_state_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("KDS_HOME", dir.path());
        let paths = Paths::discover().unwrap();
        paths.ensure_runtime_dirs().unwrap();
        fs::write(&paths.metrics, "{not valid json").unwrap();
        fs::write(&paths.digest_index, "{not valid json").unwrap();

        let diagnostics = state_diagnostics(&paths);
        assert!(diagnostics.metrics_present);
        assert!(!diagnostics.metrics_valid);
        assert!(diagnostics.digest_present);
        assert!(!diagnostics.digest_valid_json);
    }

    #[test]
    fn cleanup_stale_temp_files_removes_only_tmp_files_under_logs() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("KDS_HOME", dir.path());
        let paths = Paths::discover().unwrap();
        paths.ensure_runtime_dirs().unwrap();
        let day = paths.logs_dir.join("2026-01-01");
        fs::create_dir_all(&day).unwrap();
        let stale = day.join("run.stdout.1.tmp");
        let keep = day.join("run.log");
        fs::write(&stale, "tmp").unwrap();
        fs::write(&keep, "log").unwrap();

        let removed = paths.cleanup_stale_temp_files(Duration::ZERO).unwrap();
        assert_eq!(removed, 1);
        assert!(!stale.exists());
        assert!(keep.exists());
    }

    #[test]
    fn resolves_windows_pathext_command_shims() {
        if !cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("foo.cmd");
        fs::write(&shim, "@echo off\r\necho shim:%1\r\n").unwrap();

        let resolved = resolve_command_from(
            "foo",
            Some(dir.path().as_os_str().to_os_string()),
            Some(".COM;.EXE;.BAT;.CMD".to_string()),
        );
        assert_eq!(
            resolved.canonicalize().unwrap(),
            shim.canonicalize().unwrap()
        );
        let output = std::process::Command::new(&resolved)
            .arg("ok")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "shim:ok");

        let path_like = dir.path().join("foo");
        let resolved_path_like =
            resolve_command_from(path_like.to_str().unwrap(), None, Some(".CMD".to_string()));
        assert_eq!(
            resolved_path_like.canonicalize().unwrap(),
            shim.canonicalize().unwrap()
        );
    }

    #[test]
    fn raw_logs_preserve_non_utf8_output_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("raw.log");
        write_raw_log(RawLog {
            path: &path,
            sidecar_hint: "hint",
            command: "cmd",
            cwd: dir.path(),
            stdout: &[0xff, 0x00, b'a'],
            stderr: &[0xfe, b'b'],
            exit_code: 1,
            elapsed: "1ms",
        })
        .unwrap();
        let bytes = fs::read(path).unwrap();
        assert!(bytes.windows(3).any(|window| window == [0xff, 0x00, b'a']));
        assert!(bytes.windows(2).any(|window| window == [0xfe, b'b']));
    }

    #[test]
    fn compress_artifacts_gzips_old_raw_logs() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(dir.path());
        paths.ensure_runtime_dirs().unwrap();
        let day = paths.logs_dir.join("2026-01-01");
        fs::create_dir_all(&day).unwrap();
        let log = day.join("old.log");
        let gz = day.join("old.log.gz");
        fs::write(&log, "old log\n").unwrap();

        let report = compress_artifacts_older_than(&paths, Duration::ZERO).unwrap();

        assert_eq!(report.deleted_artifacts, 1);
        assert!(!log.exists());
        assert!(gz.exists());
        let bytes = fs::read(gz).unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
    }

    #[test]
    fn prune_to_max_artifact_bytes_deletes_run_groups() {
        let dir = tempfile::tempdir().unwrap();
        let paths = test_paths(dir.path());
        paths.ensure_runtime_dirs().unwrap();
        let day = paths.logs_dir.join("2026-01-01");
        fs::create_dir_all(&day).unwrap();
        let old_log = day.join("aaa-old.log");
        let old_sidecar = day.join("aaa-old.summary.json");
        let old_temp = day.join("aaa-old.summary.json.123.456.tmp");
        let new_log = day.join("zzz-new.log");
        fs::write(&old_log, "old-log").unwrap();
        fs::write(&old_sidecar, "old-sidecar").unwrap();
        fs::write(&old_temp, "old-temp").unwrap();
        fs::write(&new_log, "new-log").unwrap();

        let max_bytes = fs::metadata(&new_log).unwrap().len();
        let report = prune_to_max_artifact_bytes(&paths, max_bytes, false).unwrap();

        assert_eq!(report.deleted_artifacts, 3);
        assert!(!old_log.exists());
        assert!(!old_sidecar.exists());
        assert!(!old_temp.exists());
        assert!(new_log.exists());
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

    fn test_index_entry(
        run_id: &str,
        summary_path: &Path,
        argv: &[String],
        cwd: &str,
        exit_code: i32,
    ) -> IndexEntry {
        IndexEntry {
            index_schema_version: INDEX_SCHEMA_VERSION,
            run_id: run_id.into(),
            summary_path: summary_path.display().to_string(),
            exit_code,
            command_kind: "cargo".into(),
            command: command_string(argv),
            argv: argv.to_vec(),
            cwd: cwd.into(),
            started_at: iso_now(),
            log_path: summary_path.with_extension("log").display().to_string(),
        }
    }

    fn test_sidecar(run_id: &str, digest: &str) -> SummarySidecar {
        SummarySidecar {
            summary_schema_version: SUMMARY_SCHEMA_VERSION,
            kds_version: "test".into(),
            run_id: run_id.into(),
            summary_path: "old.summary.json".into(),
            command: "cargo test".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: "C:/repo".into(),
            mode: "compact".into(),
            exit_code: 1,
            elapsed: "1ms".into(),
            elapsed_ms: 1,
            digest: digest.into(),
            exact_digest: digest.into(),
            normalized_digest: digest.into(),
            repeat_status: RepeatStatus {
                is_repeat: false,
                message: "new failure signal".into(),
                first_seen: None,
                previous_log_path: None,
                current_log_path: "old.log".into(),
                repeat_count: 0,
            },
            raw_stdout_lines: 0,
            raw_stderr_lines: 1,
            raw_total_lines: 1,
            raw_stdout_chars: 0,
            raw_stderr_chars: 12,
            raw_total_chars: 12,
            raw_byte_limit: Some(10 * 1024 * 1024),
            raw_stdout_truncated: false,
            raw_stderr_truncated: false,
            raw_stdout_discarded_bytes: 0,
            raw_stderr_discarded_bytes: 0,
            shown_lines: 1,
            shown_chars: 12,
            estimated_saved_lines: 0,
            estimated_saved_chars: 0,
            estimated_output_reduction_percent: 0.0,
            estimated_char_reduction_percent: 0.0,
            approx_raw_tokens: 3,
            approx_shown_tokens: 3,
            approx_saved_tokens: 0,
            error_count: 1,
            warning_count: 0,
            primary_failure: Some("error: old".into()),
            delta: None,
            top_errors: vec!["error: old".into()],
            top_warnings: Vec::new(),
            file_hits: Vec::new(),
            tail: vec!["error: old".into()],
            suggested_next_reads: Vec::new(),
            error_windows: Vec::new(),
            digest_error_lines: vec!["error: old".into()],
            digest_file_hits: Vec::new(),
            test_or_package_hint: None,
            log_path: "old.log".into(),
            previous_exact_match_run: None,
            started_at: iso_now(),
            command_kind: "cargo".into(),
            summary_budget: "normal".into(),
            capture_mode: "test".into(),
            spawn_error: None,
            runtime_warnings: Vec::new(),
        }
    }
}
