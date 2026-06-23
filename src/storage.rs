use anyhow::{Context, Result};
use chrono::{DateTime, Local, SecondsFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const SUMMARY_SCHEMA_VERSION: u32 = 1;
pub const INDEX_SCHEMA_VERSION: u32 = 1;
pub const METRICS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub logs_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runs_index: PathBuf,
    pub digest_index: PathBuf,
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
    pub repeat_status: RepeatStatus,
    pub raw_stdout_lines: usize,
    pub raw_stderr_lines: usize,
    pub raw_total_lines: usize,
    pub shown_lines: usize,
    pub estimated_saved_lines: usize,
    pub estimated_output_reduction_percent: f64,
    pub error_count: usize,
    pub warning_count: usize,
    pub primary_failure: Option<String>,
    pub delta: Option<String>,
    pub top_errors: Vec<String>,
    pub file_hits: Vec<String>,
    pub tail: Vec<String>,
    pub suggested_next_reads: Vec<String>,
    pub log_path: String,
    pub previous_exact_match_run: Option<PreviousExactMatchRun>,
    pub started_at: String,
    pub command_kind: String,
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
    pub command_count: u64,
    pub raw_line_count: u64,
    pub shown_line_count: u64,
    pub estimated_saved_lines: u64,
    pub failure_count: u64,
    pub repeated_failure_count: u64,
    pub last_command_time: Option<String>,
    pub per_command_kind: BTreeMap<String, u64>,
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
            metrics: state_dir.join("metrics.json"),
            root,
            logs_dir,
            state_dir,
        })
    }

    pub fn ensure_runtime_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.logs_dir)
            .with_context(|| format!("create {}", self.logs_dir.display()))?;
        fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("create {}", self.state_dir.display()))?;
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
        fs::create_dir_all(&date_dir).with_context(|| format!("create {}", date_dir.display()))?;
        let run_id = make_run_id(argv, cwd, started);
        let log_path = date_dir.join(format!("{run_id}.log"));
        let summary_path = date_dir.join(format!("{run_id}.summary.json"));
        Ok(RunPaths {
            run_id,
            log_path,
            summary_path,
        })
    }
}

pub fn command_string(argv: &[String]) -> String {
    argv.join(" ")
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

pub struct RawLog<'a> {
    pub path: &'a Path,
    pub sidecar_hint: &'a str,
    pub command: &'a str,
    pub cwd: &'a Path,
    pub stdout: &'a str,
    pub stderr: &'a str,
    pub exit_code: i32,
    pub elapsed: &'a str,
}

pub fn write_raw_log(record: RawLog<'_>) -> Result<()> {
    let mut body = String::new();
    body.push_str("KDS raw command log\n");
    body.push_str(&format!("Command: {}\n", record.command));
    body.push_str(&format!("CWD: {}\n", record.cwd.display()));
    body.push_str(&format!("Exit code: {}\n", record.exit_code));
    body.push_str(&format!("Elapsed: {}\n", record.elapsed));
    body.push_str(&format!("Summary: {}\n", record.sidecar_hint));
    body.push_str("\n--- stdout ---\n");
    body.push_str(record.stdout);
    if !record.stdout.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("\n--- stderr ---\n");
    body.push_str(record.stderr);
    if !record.stderr.ends_with('\n') {
        body.push('\n');
    }
    fs::write(record.path, body).with_context(|| format!("write raw log {}", record.path.display()))
}

pub fn write_sidecar(path: &Path, sidecar: &SummarySidecar) -> Result<()> {
    write_json_atomic(path, sidecar)
}

pub fn read_sidecar(path: &Path) -> Result<SummarySidecar> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

pub fn append_index(paths: &Paths, entry: &IndexEntry) -> Result<()> {
    if let Some(parent) = paths.runs_index.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.runs_index)
        .with_context(|| format!("open {}", paths.runs_index.display()))?;
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub fn read_index(paths: &Paths) -> Vec<IndexEntry> {
    let file = match fs::File::open(&paths.runs_index) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<IndexEntry>(&line) {
            Ok(entry) => entries.push(entry),
            Err(err) => eprintln!("kds: skipping malformed index line: {err}"),
        }
    }
    entries
}

pub fn resolve_run_id(paths: &Paths, query: &str) -> Result<IndexEntry> {
    let matches: Vec<IndexEntry> = read_index(paths)
        .into_iter()
        .filter(|entry| {
            entry.run_id.starts_with(query)
                || entry.run_id.ends_with(query)
                || entry.run_id.contains(query)
        })
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

pub fn last_run(paths: &Paths) -> Result<IndexEntry> {
    read_index(paths)
        .into_iter()
        .last()
        .context("no KDS runs found")
}

pub fn previous_exact_match(
    paths: &Paths,
    argv: &[String],
    cwd: &str,
) -> Option<PreviousExactMatchRun> {
    read_index(paths)
        .into_iter()
        .rev()
        .find(|entry| entry.argv == argv && entry.cwd == cwd)
        .map(|entry| PreviousExactMatchRun {
            run_id: entry.run_id,
            exit_code: entry.exit_code,
            digest: read_sidecar(Path::new(&entry.summary_path))
                .map(|sidecar| sidecar.digest)
                .unwrap_or_default(),
            summary_path: entry.summary_path,
        })
}

pub fn load_metrics(paths: &Paths) -> Metrics {
    fs::read_to_string(&paths.metrics)
        .ok()
        .and_then(|text| serde_json::from_str::<Metrics>(&text).ok())
        .unwrap_or_else(|| Metrics {
            metrics_schema_version: METRICS_SCHEMA_VERSION,
            ..Metrics::default()
        })
}

pub fn write_metrics(paths: &Paths, metrics: &Metrics) -> Result<()> {
    if let Some(parent) = paths.metrics.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    write_json_atomic(&paths.metrics, metrics)
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("json")
    ));
    let text = serde_json::to_string_pretty(value)?;
    fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("txt")
    ));
    fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

fn make_run_id(argv: &[String], cwd: &Path, started: DateTime<Local>) -> String {
    let stamp = started.format("%Y-%m-%d-%H%M%S").to_string();
    let slug = make_slug(argv);
    let mut hasher = Sha256::new();
    hasher.update(command_string(argv).as_bytes());
    hasher.update(cwd.to_string_lossy().as_bytes());
    hasher.update(
        started
            .to_rfc3339_opts(SecondsFormat::Nanos, true)
            .as_bytes(),
    );
    let hash = format!("{:x}", hasher.finalize());
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
        assert!(resolve_run_id(&paths, "node-version").is_err());
    }
}
