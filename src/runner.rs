use anyhow::Result;
use chrono::Local;
use std::fs;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::digest;
use crate::storage::{
    self, IndexEntry, Paths, RunPaths, SummarySidecar, INDEX_SCHEMA_VERSION,
    METRICS_SCHEMA_VERSION, SUMMARY_SCHEMA_VERSION,
};
use crate::summarize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Compact,
    Raw,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Compact => "compact",
            Mode::Raw => "raw",
        }
    }
}

pub fn run(argv: Vec<String>, mode: Mode, show_paths: bool) -> Result<i32> {
    if argv.is_empty() {
        eprintln!("kds: no wrapped command provided");
        return Ok(2);
    }

    if should_passthrough(&argv) {
        return passthrough(&argv);
    }

    let cwd = std::env::current_dir()?;
    let started = Local::now();
    let command = storage::command_string(&argv);
    let safe_command = summarize::redact_sensitive_text(&command);
    let safe_argv = summarize::redact_argv(&argv);
    let safe_command_identity = storage::command_identity(&safe_argv);
    let paths = Paths::discover()?;
    let run_paths = paths.prepare_run_paths(&safe_argv, &cwd, started)?;
    cleanup_stale_temps(&paths);
    let command_kind = storage::command_kind(&argv);
    let program = storage::resolve_command(&argv[0]);
    let raw_byte_limit = raw_byte_limit();

    let (stdout_temp_path, stdout_temp_file) =
        storage::create_temp_file_near(&run_paths.log_path, "stdout")?;
    let (stderr_temp_path, stderr_temp_file) =
        storage::create_temp_file_near(&run_paths.log_path, "stderr")?;
    let _temp_cleanup = TempFileCleanup(vec![stdout_temp_path.clone(), stderr_temp_path.clone()]);

    let begin = Instant::now();
    let child = Command::new(&program)
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            let failure = format!("failed to start command: {err}");
            eprintln!("kds: failed to run `{command}`: {err}");
            record_spawn_failure(SpawnFailureRecord {
                paths: &paths,
                run_paths: &run_paths,
                mode,
                command: &safe_command,
                safe_argv: &safe_argv,
                safe_command_identity: &safe_command_identity,
                command_kind: &command_kind,
                cwd: &cwd,
                started,
                elapsed_duration: begin.elapsed(),
                failure: &failure,
            });
            return Ok(1);
        }
    };
    let child_guard = track_child(child.id());

    let stdout = child
        .stdout
        .take()
        .expect("child stdout was piped but unavailable");
    let stderr = child
        .stderr
        .take()
        .expect("child stderr was piped but unavailable");
    let stdout_reader = spawn_pipe_copy(stdout, stdout_temp_file, raw_byte_limit);
    let stderr_reader = spawn_pipe_copy(stderr, stderr_temp_file, raw_byte_limit);

    let status = match child.wait() {
        Ok(status) => status,
        Err(err) => return Err(err.into()),
    };
    let stdout_capture = join_pipe_copy("stdout", stdout_reader).unwrap_or_else(|err| {
        eprintln!("kds: stdout capture failed: {err:#}");
        PipeCapture::default()
    });
    let stderr_capture = join_pipe_copy("stderr", stderr_reader).unwrap_or_else(|err| {
        eprintln!("kds: stderr capture failed: {err:#}");
        PipeCapture::default()
    });

    let elapsed_duration = begin.elapsed();
    let elapsed = format_elapsed(elapsed_duration.as_millis());

    let interrupted = child_guard.was_interrupted();
    drop(child_guard);
    let exit_code = if interrupted {
        eprintln!("kds: interrupt received; terminated wrapped command");
        130
    } else {
        match status.code() {
            Some(code) => code,
            None => {
                eprintln!("kds: wrapped command did not provide a normal exit code; exiting 1");
                1
            }
        }
    };

    if mode == Mode::Raw {
        if let Err(err) = copy_file_to_stdout(&stdout_temp_path) {
            eprintln!("kds: raw stdout replay failed: {err:#}");
        }
        if let Err(err) = copy_file_to_stderr(&stderr_temp_path) {
            eprintln!("kds: raw stderr replay failed: {err:#}");
        }
        if stdout_capture.discarded_bytes > 0 || stderr_capture.discarded_bytes > 0 {
            eprintln!(
                "kds: raw output replay was truncated by KDS_MAX_RAW_BYTES; see raw log note"
            );
        }
    }

    let extracted_output =
        summarize::extract_from_paths(&stdout_temp_path, &stderr_temp_path, exit_code)?;
    let raw_stdout_lines = extracted_output.stdout_lines;
    let raw_stderr_lines = extracted_output.stderr_lines;
    let raw_total_lines = raw_stdout_lines + raw_stderr_lines;
    let extracted = extracted_output.summary;
    let cwd_string = cwd.display().to_string();
    let digest = digest::make_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let (previous_match, previous_sidecar) =
        match storage::previous_exact_match_with_sidecar(&paths, &safe_argv, &cwd_string) {
            Some((previous_match, previous_sidecar)) => (Some(previous_match), previous_sidecar),
            None => (None, None),
        };
    let delta = summarize::delta_line(
        previous_sidecar.as_ref(),
        &extracted,
        exit_code,
        previous_sidecar.as_ref().map(|s| s.digest.as_str()) != Some(digest.as_str()),
    );

    let log_hint = "full stdout/stderr sections preserved below";
    let mut runtime_warnings = Vec::new();
    if let Err(err) = storage::write_raw_log_from_paths(storage::RawLogPaths {
        path: &run_paths.log_path,
        sidecar_hint: log_hint,
        command: &safe_command,
        cwd: &cwd,
        stdout_path: &stdout_temp_path,
        stderr_path: &stderr_temp_path,
        stdout_discarded_bytes: stdout_capture.discarded_bytes,
        stderr_discarded_bytes: stderr_capture.discarded_bytes,
        raw_byte_limit,
        exit_code,
        elapsed: &elapsed,
    }) {
        record_runtime_warning(&mut runtime_warnings, "raw log write failed", &err);
    }

    let repeat_status = match digest::update_repeat_state(
        &paths,
        &digest,
        &safe_command_identity,
        &cwd_string,
        exit_code,
        &run_paths.log_path,
        &run_paths.run_id,
    ) {
        Ok(status) => status,
        Err(err) => {
            eprintln!("kds: digest state write failed: {err:#}");
            storage::RepeatStatus {
                is_repeat: false,
                message: "digest state unavailable".to_string(),
                first_seen: None,
                previous_log_path: None,
                current_log_path: run_paths.log_path.display().to_string(),
                repeat_count: 0,
            }
        }
    };

    let mut sidecar = SummarySidecar {
        summary_schema_version: SUMMARY_SCHEMA_VERSION,
        kds_version: env!("CARGO_PKG_VERSION").to_string(),
        run_id: run_paths.run_id.clone(),
        summary_path: run_paths.summary_path.display().to_string(),
        command: safe_command.clone(),
        argv: safe_argv.clone(),
        cwd: cwd_string.clone(),
        mode: mode.as_str().to_string(),
        exit_code,
        elapsed: elapsed.clone(),
        elapsed_ms: elapsed_duration.as_millis(),
        digest: digest.clone(),
        repeat_status,
        raw_stdout_lines,
        raw_stderr_lines,
        raw_total_lines,
        shown_lines: 0,
        estimated_saved_lines: 0,
        estimated_output_reduction_percent: 0.0,
        error_count: extracted.error_count,
        warning_count: extracted.warning_count,
        primary_failure: extracted.primary_failure,
        delta,
        top_errors: extracted.top_errors,
        file_hits: extracted.file_hits,
        tail: extracted.tail,
        suggested_next_reads: extracted.suggested_next_reads,
        log_path: run_paths.log_path.display().to_string(),
        previous_exact_match_run: previous_match,
        started_at: started.to_rfc3339(),
        command_kind: command_kind.clone(),
        capture_mode: "stdout/stderr piped to local temp files".to_string(),
        spawn_error: None,
        runtime_warnings,
    };

    let display_once = summarize::format_compact_with_paths(&sidecar, show_paths);
    sidecar.shown_lines = storage::line_count(&display_once);
    sidecar.estimated_saved_lines = raw_total_lines.saturating_sub(sidecar.shown_lines);
    sidecar.estimated_output_reduction_percent =
        storage::display_percent(sidecar.estimated_saved_lines, raw_total_lines);
    let display = summarize::format_compact_with_paths(&sidecar, show_paths);

    if mode == Mode::Compact {
        print!("{display}");
    }

    let entry = IndexEntry {
        index_schema_version: INDEX_SCHEMA_VERSION,
        run_id: run_paths.run_id.clone(),
        summary_path: run_paths.summary_path.display().to_string(),
        exit_code,
        command_kind,
        command: safe_command,
        argv: safe_argv,
        cwd: cwd_string,
        started_at: sidecar.started_at.clone(),
        log_path: run_paths.log_path.display().to_string(),
    };
    write_sidecar_index_metrics(&paths, &run_paths, &sidecar, &entry);

    Ok(exit_code)
}

fn should_passthrough(argv: &[String]) -> bool {
    git_subcommand(argv).is_some_and(|command| command == "diff")
}

fn git_subcommand(argv: &[String]) -> Option<&str> {
    let command = argv.first()?;
    let stem = std::path::Path::new(command).file_stem()?.to_str()?;
    if !stem.eq_ignore_ascii_case("git") {
        return None;
    }

    let mut args = argv.iter().skip(1).map(String::as_str).peekable();
    while let Some(arg) = args.next() {
        if arg == "--" {
            return None;
        }
        if arg == "-C"
            || arg == "-c"
            || arg == "--config-env"
            || arg == "--exec-path"
            || arg == "--git-dir"
            || arg == "--work-tree"
            || arg == "--namespace"
            || arg == "--super-prefix"
        {
            let _ = args.next();
            continue;
        }
        if arg == "-p"
            || arg == "--paginate"
            || arg == "-P"
            || arg == "--no-pager"
            || arg == "--bare"
            || arg == "--no-replace-objects"
            || arg == "--literal-pathspecs"
            || arg == "--glob-pathspecs"
            || arg == "--noglob-pathspecs"
            || arg == "--icase-pathspecs"
        {
            continue;
        }
        if arg.starts_with("-C")
            || arg.starts_with("-c")
            || arg.starts_with("--config-env=")
            || arg.starts_with("--exec-path=")
            || arg.starts_with("--git-dir=")
            || arg.starts_with("--work-tree=")
            || arg.starts_with("--namespace=")
            || arg.starts_with("--super-prefix=")
        {
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return Some(arg);
    }

    None
}

fn passthrough(argv: &[String]) -> Result<i32> {
    let program = storage::resolve_command(&argv[0]);
    let status = Command::new(&program).args(&argv[1..]).status();
    let status = match status {
        Ok(status) => status,
        Err(err) => {
            eprintln!(
                "kds: failed to passthrough `{}`: {err}",
                storage::command_string(argv)
            );
            return Ok(1);
        }
    };

    Ok(status.code().unwrap_or(1))
}

fn update_metrics(paths: &Paths, sidecar: &SummarySidecar) -> Result<()> {
    storage::with_state_lock(paths, || {
        let mut metrics = storage::load_metrics(paths);
        metrics.metrics_schema_version = METRICS_SCHEMA_VERSION;
        metrics.command_count += 1;
        metrics.raw_line_count += sidecar.raw_total_lines as u64;
        metrics.shown_line_count += sidecar.shown_lines as u64;
        metrics.estimated_saved_lines += sidecar.estimated_saved_lines as u64;
        if sidecar.exit_code != 0 {
            metrics.failure_count += 1;
        }
        if sidecar.repeat_status.is_repeat {
            metrics.repeated_failure_count += 1;
        }
        metrics.last_command_time = Some(sidecar.started_at.clone());
        *metrics
            .per_command_kind
            .entry(sidecar.command_kind.clone())
            .or_insert(0) += 1;
        storage::write_metrics(paths, &metrics)
    })
}

struct SpawnFailureRecord<'a> {
    paths: &'a Paths,
    run_paths: &'a RunPaths,
    mode: Mode,
    command: &'a str,
    safe_argv: &'a [String],
    safe_command_identity: &'a str,
    command_kind: &'a str,
    cwd: &'a std::path::Path,
    started: chrono::DateTime<Local>,
    elapsed_duration: Duration,
    failure: &'a str,
}

fn record_spawn_failure(record: SpawnFailureRecord<'_>) {
    let exit_code = 1;
    let elapsed = format_elapsed(record.elapsed_duration.as_millis());
    let failure = summarize::redact_sensitive_text(record.failure);
    let extracted = summarize::extract("", &failure, exit_code);
    let cwd_string = record.cwd.display().to_string();
    let digest = digest::make_digest(
        record.command_kind,
        record.safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let repeat_status = match digest::update_repeat_state(
        record.paths,
        &digest,
        record.safe_command_identity,
        &cwd_string,
        exit_code,
        &record.run_paths.log_path,
        &record.run_paths.run_id,
    ) {
        Ok(status) => status,
        Err(err) => {
            eprintln!("kds: digest state write failed: {err:#}");
            storage::RepeatStatus {
                is_repeat: false,
                message: "digest state unavailable".to_string(),
                first_seen: None,
                previous_log_path: None,
                current_log_path: record.run_paths.log_path.display().to_string(),
                repeat_count: 0,
            }
        }
    };

    let mut runtime_warnings = Vec::new();
    if let Err(err) = storage::write_raw_log(storage::RawLog {
        path: &record.run_paths.log_path,
        sidecar_hint: "command did not start; no child stdout/stderr captured",
        command: record.command,
        cwd: record.cwd,
        stdout: b"",
        stderr: failure.as_bytes(),
        exit_code,
        elapsed: &elapsed,
    }) {
        record_runtime_warning(&mut runtime_warnings, "raw log write failed", &err);
    }

    let mut sidecar = SummarySidecar {
        summary_schema_version: SUMMARY_SCHEMA_VERSION,
        kds_version: env!("CARGO_PKG_VERSION").to_string(),
        run_id: record.run_paths.run_id.clone(),
        summary_path: record.run_paths.summary_path.display().to_string(),
        command: record.command.to_string(),
        argv: record.safe_argv.to_vec(),
        cwd: cwd_string.clone(),
        mode: record.mode.as_str().to_string(),
        exit_code,
        elapsed: elapsed.clone(),
        elapsed_ms: record.elapsed_duration.as_millis(),
        digest: digest.clone(),
        repeat_status,
        raw_stdout_lines: 0,
        raw_stderr_lines: storage::line_count(&failure),
        raw_total_lines: storage::line_count(&failure),
        shown_lines: 0,
        estimated_saved_lines: 0,
        estimated_output_reduction_percent: 0.0,
        error_count: extracted.error_count,
        warning_count: extracted.warning_count,
        primary_failure: extracted.primary_failure,
        delta: None,
        top_errors: extracted.top_errors,
        file_hits: extracted.file_hits,
        tail: extracted.tail,
        suggested_next_reads: extracted.suggested_next_reads,
        log_path: record.run_paths.log_path.display().to_string(),
        previous_exact_match_run: None,
        started_at: record.started.to_rfc3339(),
        command_kind: record.command_kind.to_string(),
        capture_mode: "not started; spawn failed".to_string(),
        spawn_error: Some(failure),
        runtime_warnings,
    };

    let display_once = summarize::format_compact_with_paths(&sidecar, false);
    sidecar.shown_lines = storage::line_count(&display_once);
    sidecar.estimated_saved_lines = sidecar.raw_total_lines.saturating_sub(sidecar.shown_lines);
    sidecar.estimated_output_reduction_percent =
        storage::display_percent(sidecar.estimated_saved_lines, sidecar.raw_total_lines);
    if record.mode == Mode::Compact {
        print!("{}", summarize::format_compact_with_paths(&sidecar, false));
    }

    let entry = IndexEntry {
        index_schema_version: INDEX_SCHEMA_VERSION,
        run_id: record.run_paths.run_id.clone(),
        summary_path: record.run_paths.summary_path.display().to_string(),
        exit_code,
        command_kind: record.command_kind.to_string(),
        command: record.command.to_string(),
        argv: record.safe_argv.to_vec(),
        cwd: cwd_string,
        started_at: sidecar.started_at.clone(),
        log_path: record.run_paths.log_path.display().to_string(),
    };
    write_sidecar_index_metrics(record.paths, record.run_paths, &sidecar, &entry);
}

fn write_sidecar_index_metrics(
    paths: &Paths,
    run_paths: &RunPaths,
    sidecar: &SummarySidecar,
    entry: &IndexEntry,
) {
    if let Err(err) = storage::write_sidecar(&run_paths.summary_path, sidecar) {
        eprintln!("kds: sidecar write failed: {err:#}; wrapped exit code preserved");
    }
    if let Err(err) = storage::append_index(paths, entry) {
        eprintln!("kds: run index write failed: {err:#}; wrapped exit code preserved");
    }
    if let Err(err) = update_metrics(paths, sidecar) {
        eprintln!("kds: metrics write failed: {err:#}; wrapped exit code preserved");
    }
}

fn record_runtime_warning(warnings: &mut Vec<String>, label: &str, err: &anyhow::Error) {
    let warning = summarize::redact_sensitive_text(&format!("{label}: {err:#}"));
    eprintln!("kds: {warning}; wrapped exit code preserved");
    warnings.push(warning);
}

fn cleanup_stale_temps(paths: &Paths) {
    let stale_after = stale_temp_after();
    match paths.cleanup_stale_temp_files(stale_after) {
        Ok(0) => {}
        Ok(removed) => eprintln!("kds: removed {removed} stale temp file(s)"),
        Err(err) => eprintln!("kds: stale temp cleanup failed: {err:#}"),
    }
}

fn stale_temp_after() -> Duration {
    match std::env::var("KDS_STALE_TMP_SECS") {
        Ok(raw) => match raw.parse::<u64>() {
            Ok(seconds) => Duration::from_secs(seconds),
            Err(_) => {
                eprintln!("kds: ignoring invalid KDS_STALE_TMP_SECS={raw:?}");
                Duration::from_secs(24 * 60 * 60)
            }
        },
        Err(_) => Duration::from_secs(24 * 60 * 60),
    }
}

struct ProcessTracker {
    child_pid: AtomicU32,
    interrupted: AtomicBool,
}

struct ChildProcessGuard {
    tracker: Arc<ProcessTracker>,
}

fn process_tracker() -> Arc<ProcessTracker> {
    static TRACKER: OnceLock<Arc<ProcessTracker>> = OnceLock::new();
    TRACKER
        .get_or_init(|| {
            let tracker = Arc::new(ProcessTracker {
                child_pid: AtomicU32::new(0),
                interrupted: AtomicBool::new(false),
            });
            let handler_tracker = tracker.clone();
            if let Err(err) = ctrlc::set_handler(move || {
                handler_tracker.interrupted.store(true, Ordering::SeqCst);
                let pid = handler_tracker.child_pid.load(Ordering::SeqCst);
                if pid != 0 {
                    terminate_process_tree(pid);
                }
            }) {
                eprintln!("kds: failed to install interrupt handler: {err}");
            }
            tracker
        })
        .clone()
}

fn track_child(pid: u32) -> ChildProcessGuard {
    let tracker = process_tracker();
    tracker.interrupted.store(false, Ordering::SeqCst);
    tracker.child_pid.store(pid, Ordering::SeqCst);
    ChildProcessGuard { tracker }
}

impl ChildProcessGuard {
    fn was_interrupted(&self) -> bool {
        self.tracker.interrupted.load(Ordering::SeqCst)
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        self.tracker.child_pid.store(0, Ordering::SeqCst);
    }
}

fn terminate_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn format_elapsed(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.2}s", ms as f64 / 1000.0)
    }
}

#[derive(Default)]
struct PipeCapture {
    captured_bytes: u64,
    discarded_bytes: u64,
}

fn spawn_pipe_copy<R>(
    mut reader: R,
    mut file: fs::File,
    raw_byte_limit: Option<u64>,
) -> thread::JoinHandle<io::Result<PipeCapture>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut capture = PipeCapture::default();
        let mut buffer = [0_u8; 8192];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let writable = match raw_byte_limit {
                Some(limit) => limit
                    .saturating_sub(capture.captured_bytes)
                    .min(read as u64),
                None => read as u64,
            } as usize;
            if writable > 0 {
                file.write_all(&buffer[..writable])?;
                capture.captured_bytes += writable as u64;
            }
            if writable < read {
                capture.discarded_bytes += (read - writable) as u64;
            }
        }
        file.sync_all()?;
        Ok(capture)
    })
}

fn join_pipe_copy(
    name: &str,
    handle: thread::JoinHandle<io::Result<PipeCapture>>,
) -> Result<PipeCapture> {
    match handle.join() {
        Ok(result) => Ok(result?),
        Err(_) => anyhow::bail!("{name} capture thread panicked"),
    }
}

fn copy_file_to_stdout(path: &std::path::Path) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut stdout = io::stdout().lock();
    io::copy(&mut file, &mut stdout)?;
    stdout.flush()?;
    Ok(())
}

fn copy_file_to_stderr(path: &std::path::Path) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut stderr = io::stderr().lock();
    io::copy(&mut file, &mut stderr)?;
    stderr.flush()?;
    Ok(())
}

fn raw_byte_limit() -> Option<u64> {
    let Ok(raw) = std::env::var("KDS_MAX_RAW_BYTES") else {
        return None;
    };
    match raw.parse::<u64>() {
        Ok(0) => None,
        Ok(limit) => Some(limit),
        Err(_) => {
            eprintln!("kds: ignoring invalid KDS_MAX_RAW_BYTES={raw:?}");
            None
        }
    }
}

struct TempFileCleanup(Vec<std::path::PathBuf>);

impl Drop for TempFileCleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}
