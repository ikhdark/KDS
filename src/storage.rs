use anyhow::{Context, Result};
use chrono::{DateTime, Local, SecondsFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SUMMARY_SCHEMA_VERSION: u32 = 2;
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
    pub command_count: u64,
    pub raw_line_count: u64,
    pub shown_line_count: u64,
    pub estimated_saved_lines: u64,
    pub failure_count: u64,
    pub repeated_failure_count: u64,
    pub last_command_time: Option<String>,
    pub per_command_kind: BTreeMap<String, u64>,
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
}

fn default_capture_mode() -> String {
    "stdout/stderr piped".to_string()
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
    let mut buffer = [0_u8; 8192];
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
    StateDiagnostics {
        runs_index_present: index.present,
        runs_index_entries: index.entries,
        runs_index_malformed_lines: index.malformed_lines,
        runs_index_read_errors: index.read_errors,
        metrics_present,
        metrics_valid,
        digest_present,
        digest_valid_json,
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
        .last()
        .context("no KDS runs found")
}

pub fn previous_exact_match_with_sidecar(
    paths: &Paths,
    argv: &[String],
    cwd: &str,
) -> Option<(PreviousExactMatchRun, Option<SummarySidecar>)> {
    let entry = read_index(paths)
        .into_iter()
        .rev()
        .find(|entry| entry.argv == argv && entry.cwd == cwd)?;
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
    Some((previous, sidecar))
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
        create_private_dir_all(parent)?;
    }
    write_json_atomic(&paths.metrics, metrics)
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
    match fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(_err) if cfg!(windows) && path.exists() => {
            fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
            fs::rename(tmp, path)
                .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))
        }
        Err(err) => {
            Err(err).with_context(|| format!("rename {} to {}", tmp.display(), path.display()))
        }
    }
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
}
