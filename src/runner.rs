use anyhow::Result;
use chrono::Local;
use std::fs;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Instant;

use crate::digest;
use crate::storage::{
    self, IndexEntry, Paths, SummarySidecar, INDEX_SCHEMA_VERSION, METRICS_SCHEMA_VERSION,
    SUMMARY_SCHEMA_VERSION,
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

    let cwd = std::env::current_dir()?;
    let started = Local::now();
    let command = storage::command_string(&argv);
    let safe_command = summarize::redact_sensitive_text(&command);
    let safe_argv = summarize::redact_argv(&argv);
    let safe_command_identity = storage::command_identity(&safe_argv);
    let paths = Paths::discover()?;
    let run_paths = paths.prepare_run_paths(&safe_argv, &cwd, started)?;
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
            eprintln!("kds: failed to run `{command}`: {err}");
            let _ = fs::remove_file(&stdout_temp_path);
            let _ = fs::remove_file(&stderr_temp_path);
            return Ok(1);
        }
    };

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

    let status = child.wait()?;
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

    let exit_code = match status.code() {
        Some(code) => code,
        None => {
            eprintln!("kds: wrapped command did not provide a normal exit code; exiting 1");
            1
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
        eprintln!("kds: raw log write failed: {err:#}");
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

    if let Err(err) = storage::write_sidecar(&run_paths.summary_path, &sidecar) {
        eprintln!("kds: sidecar write failed: {err:#}");
    }
    let entry = IndexEntry {
        index_schema_version: INDEX_SCHEMA_VERSION,
        run_id: run_paths.run_id,
        summary_path: run_paths.summary_path.display().to_string(),
        exit_code,
        command_kind,
        command: safe_command,
        argv: safe_argv,
        cwd: cwd_string,
        started_at: sidecar.started_at.clone(),
        log_path: run_paths.log_path.display().to_string(),
    };
    if let Err(err) = storage::append_index(&paths, &entry) {
        eprintln!("kds: run index write failed: {err:#}");
    }
    if let Err(err) = update_metrics(&paths, &sidecar) {
        eprintln!("kds: metrics write failed: {err:#}");
    }

    Ok(exit_code)
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
