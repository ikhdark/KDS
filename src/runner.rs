use anyhow::Result;
use chrono::Local;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::digest;
use crate::storage::{
    self, IndexEntry, Paths, RepeatStatus, RunPaths, SummarySidecar, INDEX_SCHEMA_VERSION,
    SUMMARY_SCHEMA_VERSION,
};
use crate::summarize;

const DEFAULT_RAW_BYTE_LIMIT: u64 = 10 * 1024 * 1024;
const DEFAULT_SUMMARY_BUDGET: &str = "auto";

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

pub fn run(
    argv: Vec<String>,
    mode: Mode,
    show_paths: bool,
    summary_budget: Option<String>,
    save_artifacts: bool,
) -> Result<i32> {
    if argv.is_empty() {
        eprintln!("kds: no wrapped command provided");
        return Ok(2);
    }

    if should_passthrough(&argv) {
        return passthrough(&argv);
    }

    if !artifacts_enabled(save_artifacts) {
        return run_memory_only(argv, mode, show_paths, summary_budget);
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
    apply_retention_controls(&paths);
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
            eprintln!("kds: failed to run `{safe_command}`: {err}");
            record_spawn_failure(SpawnFailureRecord {
                paths: &paths,
                run_paths: &run_paths,
                mode,
                summary_budget: effective_summary_budget(summary_budget.as_deref()),
                command: &safe_command,
                safe_argv: &safe_argv,
                safe_command_identity: &safe_command_identity,
                command_kind: &command_kind,
                cwd: &cwd,
                started,
                elapsed_duration: begin.elapsed(),
                failure: &failure,
                raw_byte_limit,
            });
            return Ok(1);
        }
    };
    let child_guard = track_child(child.id());
    let progress_notice = ProgressNotice::start(mode, begin);

    let stdout = child
        .stdout
        .take()
        .expect("child stdout was piped but unavailable");
    let stderr = child
        .stderr
        .take()
        .expect("child stderr was piped but unavailable");
    let live_tee = mode == Mode::Raw;
    let stdout_reader = spawn_pipe_copy(
        stdout,
        stdout_temp_file,
        raw_byte_limit,
        "stdout",
        if live_tee {
            Some(StreamTee::Stdout)
        } else {
            None
        },
    );
    let stderr_reader = spawn_pipe_copy(
        stderr,
        stderr_temp_file,
        raw_byte_limit,
        "stderr",
        if live_tee {
            Some(StreamTee::Stderr)
        } else {
            None
        },
    );

    let status = child.wait()?;
    drop(progress_notice);
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

    if mode == Mode::Raw
        && (stdout_capture.discarded_bytes > 0 || stderr_capture.discarded_bytes > 0)
    {
        eprintln!(
            "kds: persisted raw log hit the configured byte limit; live raw output was still teed"
        );
    }

    let extracted_output = summarize::merge_stream_summaries(
        stdout_capture.summary,
        stderr_capture.summary,
        exit_code,
    );
    let raw_stdout_lines = extracted_output.stdout_lines;
    let raw_stderr_lines = extracted_output.stderr_lines;
    let raw_total_lines = raw_stdout_lines + raw_stderr_lines;
    let raw_stdout_chars = extracted_output.stdout_chars;
    let raw_stderr_chars = extracted_output.stderr_chars;
    let raw_total_chars = raw_stdout_chars + raw_stderr_chars;
    let extracted = extracted_output.summary;
    let cwd_string = cwd.display().to_string();
    let exact_digest = digest::make_exact_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let normalized_digest = digest::make_normalized_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let digest = normalized_digest.clone();
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

    let sidecar = SummarySidecar {
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
        exact_digest,
        normalized_digest,
        repeat_status: placeholder_repeat_status(&run_paths),
        raw_stdout_lines,
        raw_stderr_lines,
        raw_total_lines,
        raw_stdout_chars,
        raw_stderr_chars,
        raw_total_chars,
        raw_byte_limit,
        raw_stdout_truncated: stdout_capture.discarded_bytes > 0,
        raw_stderr_truncated: stderr_capture.discarded_bytes > 0,
        raw_stdout_discarded_bytes: stdout_capture.discarded_bytes,
        raw_stderr_discarded_bytes: stderr_capture.discarded_bytes,
        shown_lines: 0,
        shown_chars: 0,
        estimated_saved_lines: 0,
        estimated_saved_chars: 0,
        estimated_output_reduction_percent: 0.0,
        estimated_char_reduction_percent: 0.0,
        approx_raw_tokens: approximate_tokens(raw_total_chars),
        approx_shown_tokens: 0,
        approx_saved_tokens: 0,
        error_count: extracted.error_count,
        warning_count: extracted.warning_count,
        primary_failure: extracted.primary_failure,
        delta,
        top_errors: extracted.top_errors,
        top_warnings: extracted.top_warnings,
        file_hits: extracted.file_hits,
        tail: extracted.tail,
        suggested_next_reads: extracted.suggested_next_reads,
        error_windows: extracted.error_windows,
        digest_error_lines: extracted.digest_error_lines,
        digest_file_hits: extracted.digest_file_hits,
        test_or_package_hint: extracted.test_or_package_hint,
        log_path: run_paths.log_path.display().to_string(),
        previous_exact_match_run: previous_match,
        started_at: started.to_rfc3339(),
        command_kind: command_kind.clone(),
        summary_budget: effective_summary_budget(summary_budget.as_deref()),
        capture_mode: "stdout/stderr piped to local temp files".to_string(),
        spawn_error: None,
        runtime_warnings,
    };

    let entry = IndexEntry {
        index_schema_version: INDEX_SCHEMA_VERSION,
        run_id: run_paths.run_id.clone(),
        summary_path: run_paths.summary_path.display().to_string(),
        exit_code,
        command_kind,
        command: safe_command,
        argv: safe_argv,
        cwd: cwd_string.clone(),
        started_at: sidecar.started_at.clone(),
        log_path: run_paths.log_path.display().to_string(),
    };
    let (_sidecar, display) = commit_run_state(CommitRunState {
        paths: &paths,
        run_paths: &run_paths,
        sidecar,
        entry: &entry,
        digest: &digest,
        command_identity: &safe_command_identity,
        cwd: &cwd_string,
        show_paths,
    });

    if mode == Mode::Compact {
        print!("{display}");
    }

    Ok(exit_code)
}

fn run_memory_only(
    argv: Vec<String>,
    mode: Mode,
    show_paths: bool,
    summary_budget: Option<String>,
) -> Result<i32> {
    let cwd = std::env::current_dir()?;
    let started = Local::now();
    let command = storage::command_string(&argv);
    let safe_command = summarize::redact_sensitive_text(&command);
    let safe_argv = summarize::redact_argv(&argv);
    let safe_command_identity = storage::command_identity(&safe_argv);
    let command_kind = storage::command_kind(&argv);
    let program = storage::resolve_command(&argv[0]);

    let begin = Instant::now();
    let child = Command::new(&program)
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            let failure =
                summarize::redact_sensitive_text(&format!("failed to start command: {err}"));
            eprintln!("kds: failed to run `{safe_command}`: {err}");
            let elapsed_duration = begin.elapsed();
            let extracted = summarize::extract("", &failure, 1);
            let cwd_string = cwd.display().to_string();
            let exact_digest = digest::make_exact_digest(
                &command_kind,
                &safe_command_identity,
                &cwd_string,
                1,
                &extracted,
            );
            let normalized_digest = digest::make_normalized_digest(
                &command_kind,
                &safe_command_identity,
                &cwd_string,
                1,
                &extracted,
            );
            let mut sidecar = memory_sidecar(MemorySidecarInput {
                run_id: memory_run_id(started, &safe_argv, &cwd, &normalized_digest),
                command: safe_command,
                argv: safe_argv,
                cwd: cwd_string,
                mode: mode.as_str().to_string(),
                exit_code: 1,
                elapsed: format_elapsed(elapsed_duration.as_millis()),
                elapsed_ms: elapsed_duration.as_millis(),
                exact_digest,
                normalized_digest,
                extracted,
                raw_stdout_lines: 0,
                raw_stderr_lines: storage::line_count(&failure),
                raw_stdout_chars: 0,
                raw_stderr_chars: failure.chars().count(),
                started_at: started.to_rfc3339(),
                command_kind,
                summary_budget: effective_summary_budget(summary_budget.as_deref()),
                capture_mode: "memory-only; artifacts disabled".to_string(),
                spawn_error: Some(failure),
            });
            let display = finalize_sidecar_counts(&mut sidecar, show_paths);
            record_memory_metric(&sidecar);
            if mode == Mode::Compact {
                print!("{display}");
            }
            return Ok(1);
        }
    };
    let child_guard = track_child(child.id());
    let progress_notice = ProgressNotice::start(mode, begin);

    let stdout = child
        .stdout
        .take()
        .expect("child stdout was piped but unavailable");
    let stderr = child
        .stderr
        .take()
        .expect("child stderr was piped but unavailable");
    let live_tee = mode == Mode::Raw;
    let stdout_reader = spawn_pipe_summary(
        stdout,
        "stdout",
        if live_tee {
            Some(StreamTee::Stdout)
        } else {
            None
        },
    );
    let stderr_reader = spawn_pipe_summary(
        stderr,
        "stderr",
        if live_tee {
            Some(StreamTee::Stderr)
        } else {
            None
        },
    );

    let status = child.wait()?;
    drop(progress_notice);
    let stdout_capture = join_pipe_copy("stdout", stdout_reader).unwrap_or_else(|err| {
        eprintln!("kds: stdout capture failed: {err:#}");
        PipeCapture::default()
    });
    let stderr_capture = join_pipe_copy("stderr", stderr_reader).unwrap_or_else(|err| {
        eprintln!("kds: stderr capture failed: {err:#}");
        PipeCapture::default()
    });

    let elapsed_duration = begin.elapsed();
    let interrupted = child_guard.was_interrupted();
    drop(child_guard);
    let exit_code = if interrupted {
        eprintln!("kds: interrupt received; terminated wrapped command");
        130
    } else {
        status.code().unwrap_or_else(|| {
            eprintln!("kds: wrapped command did not provide a normal exit code; exiting 1");
            1
        })
    };

    let extracted_output = summarize::merge_stream_summaries(
        stdout_capture.summary,
        stderr_capture.summary,
        exit_code,
    );
    let raw_stdout_lines = extracted_output.stdout_lines;
    let raw_stderr_lines = extracted_output.stderr_lines;
    let raw_stdout_chars = extracted_output.stdout_chars;
    let raw_stderr_chars = extracted_output.stderr_chars;
    let extracted = extracted_output.summary;
    let cwd_string = cwd.display().to_string();
    let exact_digest = digest::make_exact_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let normalized_digest = digest::make_normalized_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let mut sidecar = memory_sidecar(MemorySidecarInput {
        run_id: memory_run_id(started, &safe_argv, &cwd, &normalized_digest),
        command: safe_command,
        argv: safe_argv,
        cwd: cwd_string,
        mode: mode.as_str().to_string(),
        exit_code,
        elapsed: format_elapsed(elapsed_duration.as_millis()),
        elapsed_ms: elapsed_duration.as_millis(),
        exact_digest,
        normalized_digest,
        extracted,
        raw_stdout_lines,
        raw_stderr_lines,
        raw_stdout_chars,
        raw_stderr_chars,
        started_at: started.to_rfc3339(),
        command_kind,
        summary_budget: effective_summary_budget(summary_budget.as_deref()),
        capture_mode: "memory-only; artifacts disabled".to_string(),
        spawn_error: None,
    });
    let display = finalize_sidecar_counts(&mut sidecar, show_paths);
    record_memory_metric(&sidecar);

    if mode == Mode::Compact {
        print!("{display}");
    }

    Ok(exit_code)
}

pub fn summarize_import(args: crate::cli::SummarizeArgs) -> Result<i32> {
    if !artifacts_enabled(args.save_artifacts) {
        return summarize_import_memory_only(args);
    }

    let cwd = std::env::current_dir()?;
    let started = Local::now();
    let begin = Instant::now();
    let paths = Paths::discover()?;
    let label = import_label(&args);
    let safe_label = summarize::redact_sensitive_text(&label);
    let safe_argv = vec!["kds-summarize".to_string(), safe_label];
    let safe_command = storage::command_string(&safe_argv);
    let safe_command_identity = storage::command_identity(&safe_argv);
    let run_paths = paths.prepare_run_paths(&safe_argv, &cwd, started)?;
    cleanup_stale_temps(&paths);
    apply_retention_controls(&paths);
    let command_kind = storage::command_kind(&safe_argv);
    let raw_byte_limit = raw_byte_limit();

    let (stdout_temp_path, stdout_temp_file) =
        storage::create_temp_file_near(&run_paths.log_path, "stdin")?;
    let (stderr_temp_path, stderr_temp_file) =
        storage::create_temp_file_near(&run_paths.log_path, "stderr")?;
    drop(stderr_temp_file);
    let _temp_cleanup = TempFileCleanup(vec![stdout_temp_path.clone(), stderr_temp_path.clone()]);

    let stream = if let Some(path) = args.file.as_deref() {
        let file = fs::File::open(path)?;
        capture_imported_reader(
            BufReader::new(file),
            stdout_temp_file,
            raw_byte_limit,
            "file",
        )?
    } else {
        let stdin = io::stdin();
        capture_imported_reader(stdin.lock(), stdout_temp_file, raw_byte_limit, "stdin")?
    };

    let elapsed_duration = begin.elapsed();
    let elapsed = format_elapsed(elapsed_duration.as_millis());
    if stream.discarded_bytes > 0 {
        eprintln!(
            "kds: persisted imported log hit the configured byte limit; full input was still summarized"
        );
    }

    let extracted_output = summarize::merge_stream_summaries(
        stream.summary,
        summarize::StreamSummary::default(),
        args.exit_code,
    );
    let raw_stdout_lines = extracted_output.stdout_lines;
    let raw_stderr_lines = extracted_output.stderr_lines;
    let raw_total_lines = raw_stdout_lines + raw_stderr_lines;
    let raw_stdout_chars = extracted_output.stdout_chars;
    let raw_stderr_chars = extracted_output.stderr_chars;
    let raw_total_chars = raw_stdout_chars + raw_stderr_chars;
    let extracted = extracted_output.summary;
    let cwd_string = cwd.display().to_string();
    let exact_digest = digest::make_exact_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        args.exit_code,
        &extracted,
    );
    let normalized_digest = digest::make_normalized_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        args.exit_code,
        &extracted,
    );
    let digest = normalized_digest.clone();
    let (previous_match, previous_sidecar) =
        match storage::previous_exact_match_with_sidecar(&paths, &safe_argv, &cwd_string) {
            Some((previous_match, previous_sidecar)) => (Some(previous_match), previous_sidecar),
            None => (None, None),
        };
    let delta = summarize::delta_line(
        previous_sidecar.as_ref(),
        &extracted,
        args.exit_code,
        previous_sidecar.as_ref().map(|s| s.digest.as_str()) != Some(digest.as_str()),
    );

    let log_hint = "redacted imported log preserved below";
    let mut runtime_warnings = Vec::new();
    if let Err(err) = storage::write_raw_log_from_paths(storage::RawLogPaths {
        path: &run_paths.log_path,
        sidecar_hint: log_hint,
        command: &safe_command,
        cwd: &cwd,
        stdout_path: &stdout_temp_path,
        stderr_path: &stderr_temp_path,
        stdout_discarded_bytes: stream.discarded_bytes,
        stderr_discarded_bytes: 0,
        raw_byte_limit,
        exit_code: args.exit_code,
        elapsed: &elapsed,
    }) {
        record_import_runtime_warning(&mut runtime_warnings, "imported log write failed", &err);
    }

    let sidecar = SummarySidecar {
        summary_schema_version: SUMMARY_SCHEMA_VERSION,
        kds_version: env!("CARGO_PKG_VERSION").to_string(),
        run_id: run_paths.run_id.clone(),
        summary_path: run_paths.summary_path.display().to_string(),
        command: safe_command.clone(),
        argv: safe_argv.clone(),
        cwd: cwd_string.clone(),
        mode: "import".to_string(),
        exit_code: args.exit_code,
        elapsed: elapsed.clone(),
        elapsed_ms: elapsed_duration.as_millis(),
        digest: digest.clone(),
        exact_digest,
        normalized_digest,
        repeat_status: placeholder_repeat_status(&run_paths),
        raw_stdout_lines,
        raw_stderr_lines,
        raw_total_lines,
        raw_stdout_chars,
        raw_stderr_chars,
        raw_total_chars,
        raw_byte_limit,
        raw_stdout_truncated: stream.discarded_bytes > 0,
        raw_stderr_truncated: false,
        raw_stdout_discarded_bytes: stream.discarded_bytes,
        raw_stderr_discarded_bytes: 0,
        shown_lines: 0,
        shown_chars: 0,
        estimated_saved_lines: 0,
        estimated_saved_chars: 0,
        estimated_output_reduction_percent: 0.0,
        estimated_char_reduction_percent: 0.0,
        approx_raw_tokens: approximate_tokens(raw_total_chars),
        approx_shown_tokens: 0,
        approx_saved_tokens: 0,
        error_count: extracted.error_count,
        warning_count: extracted.warning_count,
        primary_failure: extracted.primary_failure,
        delta,
        top_errors: extracted.top_errors,
        top_warnings: extracted.top_warnings,
        file_hits: extracted.file_hits,
        tail: extracted.tail,
        suggested_next_reads: extracted.suggested_next_reads,
        error_windows: extracted.error_windows,
        digest_error_lines: extracted.digest_error_lines,
        digest_file_hits: extracted.digest_file_hits,
        test_or_package_hint: extracted.test_or_package_hint,
        log_path: run_paths.log_path.display().to_string(),
        previous_exact_match_run: previous_match,
        started_at: started.to_rfc3339(),
        command_kind: command_kind.clone(),
        summary_budget: effective_summary_budget(args.budget.map(|budget| budget.as_str())),
        capture_mode: import_capture_mode(args.file.is_some()).to_string(),
        spawn_error: None,
        runtime_warnings,
    };

    let entry = IndexEntry {
        index_schema_version: INDEX_SCHEMA_VERSION,
        run_id: run_paths.run_id.clone(),
        summary_path: run_paths.summary_path.display().to_string(),
        exit_code: args.exit_code,
        command_kind,
        command: safe_command,
        argv: safe_argv,
        cwd: cwd_string.clone(),
        started_at: sidecar.started_at.clone(),
        log_path: run_paths.log_path.display().to_string(),
    };
    let (_sidecar, display) = commit_run_state(CommitRunState {
        paths: &paths,
        run_paths: &run_paths,
        sidecar,
        entry: &entry,
        digest: &digest,
        command_identity: &safe_command_identity,
        cwd: &cwd_string,
        show_paths: args.show_paths,
    });

    print!("{display}");
    Ok(0)
}

fn summarize_import_memory_only(args: crate::cli::SummarizeArgs) -> Result<i32> {
    let cwd = std::env::current_dir()?;
    let started = Local::now();
    let begin = Instant::now();
    let label = import_label(&args);
    let safe_label = summarize::redact_sensitive_text(&label);
    let safe_argv = vec!["kds-summarize".to_string(), safe_label];
    let safe_command = storage::command_string(&safe_argv);
    let safe_command_identity = storage::command_identity(&safe_argv);
    let command_kind = storage::command_kind(&safe_argv);

    let stream = if let Some(path) = args.file.as_deref() {
        let file = fs::File::open(path)?;
        capture_imported_reader_memory(BufReader::new(file), "file")?
    } else {
        let stdin = io::stdin();
        capture_imported_reader_memory(stdin.lock(), "stdin")?
    };

    let elapsed_duration = begin.elapsed();
    let extracted_output = summarize::merge_stream_summaries(
        stream,
        summarize::StreamSummary::default(),
        args.exit_code,
    );
    let raw_stdout_lines = extracted_output.stdout_lines;
    let raw_stderr_lines = extracted_output.stderr_lines;
    let raw_stdout_chars = extracted_output.stdout_chars;
    let raw_stderr_chars = extracted_output.stderr_chars;
    let extracted = extracted_output.summary;
    let cwd_string = cwd.display().to_string();
    let exact_digest = digest::make_exact_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        args.exit_code,
        &extracted,
    );
    let normalized_digest = digest::make_normalized_digest(
        &command_kind,
        &safe_command_identity,
        &cwd_string,
        args.exit_code,
        &extracted,
    );
    let mut sidecar = memory_sidecar(MemorySidecarInput {
        run_id: memory_run_id(started, &safe_argv, &cwd, &normalized_digest),
        command: safe_command,
        argv: safe_argv,
        cwd: cwd_string,
        mode: "import".to_string(),
        exit_code: args.exit_code,
        elapsed: format_elapsed(elapsed_duration.as_millis()),
        elapsed_ms: elapsed_duration.as_millis(),
        exact_digest,
        normalized_digest,
        extracted,
        raw_stdout_lines,
        raw_stderr_lines,
        raw_stdout_chars,
        raw_stderr_chars,
        started_at: started.to_rfc3339(),
        command_kind,
        summary_budget: effective_summary_budget(args.budget.map(|budget| budget.as_str())),
        capture_mode: import_memory_capture_mode(args.file.is_some()).to_string(),
        spawn_error: None,
    });
    let display = finalize_sidecar_counts(&mut sidecar, args.show_paths);
    record_memory_metric(&sidecar);

    print!("{display}");
    Ok(0)
}

fn import_label(args: &crate::cli::SummarizeArgs) -> String {
    if let Some(name) = args.name.as_deref().filter(|name| !name.trim().is_empty()) {
        return name.trim().to_string();
    }
    if let Some(path) = args.file.as_deref() {
        return path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("file")
            .to_string();
    }
    "stdin".to_string()
}

struct MemorySidecarInput {
    run_id: String,
    command: String,
    argv: Vec<String>,
    cwd: String,
    mode: String,
    exit_code: i32,
    elapsed: String,
    elapsed_ms: u128,
    exact_digest: String,
    normalized_digest: String,
    extracted: summarize::ExtractedSummary,
    raw_stdout_lines: usize,
    raw_stderr_lines: usize,
    raw_stdout_chars: usize,
    raw_stderr_chars: usize,
    started_at: String,
    command_kind: String,
    summary_budget: String,
    capture_mode: String,
    spawn_error: Option<String>,
}

fn memory_sidecar(input: MemorySidecarInput) -> SummarySidecar {
    let raw_total_lines = input.raw_stdout_lines + input.raw_stderr_lines;
    let raw_total_chars = input.raw_stdout_chars + input.raw_stderr_chars;
    SummarySidecar {
        summary_schema_version: SUMMARY_SCHEMA_VERSION,
        kds_version: env!("CARGO_PKG_VERSION").to_string(),
        run_id: input.run_id.clone(),
        summary_path: String::new(),
        command: input.command,
        argv: input.argv,
        cwd: input.cwd,
        mode: input.mode,
        exit_code: input.exit_code,
        elapsed: input.elapsed,
        elapsed_ms: input.elapsed_ms,
        digest: input.normalized_digest.clone(),
        exact_digest: input.exact_digest,
        normalized_digest: input.normalized_digest,
        repeat_status: RepeatStatus {
            is_repeat: false,
            message: "not tracked; artifacts disabled".to_string(),
            first_seen: None,
            previous_log_path: None,
            current_log_path: String::new(),
            repeat_count: 0,
        },
        raw_stdout_lines: input.raw_stdout_lines,
        raw_stderr_lines: input.raw_stderr_lines,
        raw_total_lines,
        raw_stdout_chars: input.raw_stdout_chars,
        raw_stderr_chars: input.raw_stderr_chars,
        raw_total_chars,
        raw_byte_limit: None,
        raw_stdout_truncated: false,
        raw_stderr_truncated: false,
        raw_stdout_discarded_bytes: 0,
        raw_stderr_discarded_bytes: 0,
        shown_lines: 0,
        shown_chars: 0,
        estimated_saved_lines: 0,
        estimated_saved_chars: 0,
        estimated_output_reduction_percent: 0.0,
        estimated_char_reduction_percent: 0.0,
        approx_raw_tokens: approximate_tokens(raw_total_chars),
        approx_shown_tokens: 0,
        approx_saved_tokens: 0,
        error_count: input.extracted.error_count,
        warning_count: input.extracted.warning_count,
        primary_failure: input.extracted.primary_failure,
        delta: None,
        top_errors: input.extracted.top_errors,
        top_warnings: input.extracted.top_warnings,
        file_hits: input.extracted.file_hits,
        tail: input.extracted.tail,
        suggested_next_reads: input.extracted.suggested_next_reads,
        error_windows: input.extracted.error_windows,
        digest_error_lines: input.extracted.digest_error_lines,
        digest_file_hits: input.extracted.digest_file_hits,
        test_or_package_hint: input.extracted.test_or_package_hint,
        log_path: String::new(),
        previous_exact_match_run: None,
        started_at: input.started_at,
        command_kind: input.command_kind,
        summary_budget: input.summary_budget,
        capture_mode: input.capture_mode,
        spawn_error: input.spawn_error,
        runtime_warnings: Vec::new(),
    }
}

fn memory_run_id(
    started: chrono::DateTime<Local>,
    argv: &[String],
    cwd: &std::path::Path,
    digest: &str,
) -> String {
    let stamp = started.format("%Y-%m-%d-%H%M%S").to_string();
    let slug = memory_slug(argv);
    let cwd_len = cwd.to_string_lossy().len();
    let hash = digest.get(..6).unwrap_or("memory");
    format!("{stamp}-{slug}-{cwd_len:x}{hash}")
}

fn memory_slug(argv: &[String]) -> String {
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
        "memory".to_string()
    } else {
        trimmed
    }
}

fn artifacts_enabled(save_arg: bool) -> bool {
    save_arg
        || env_truthy("KDS_SAVE_ARTIFACTS")
        || env_truthy("KDS_DURABLE_ARTIFACTS")
        || env_truthy("KDS_DURABLE_LOGS")
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn import_capture_mode(from_file: bool) -> &'static str {
    if from_file {
        "file import; redacted before local artifact write"
    } else {
        "stdin import; redacted before local artifact write"
    }
}

fn import_memory_capture_mode(from_file: bool) -> &'static str {
    if from_file {
        "file import; memory-only; artifacts disabled"
    } else {
        "stdin import; memory-only; artifacts disabled"
    }
}

fn capture_imported_reader_memory<R>(
    mut reader: R,
    stream: &'static str,
) -> io::Result<summarize::StreamSummary>
where
    R: BufRead,
{
    let mut summary = summarize::StreamSummaryBuilder::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        summary.push_bytes(&line);
    }
    Ok(summary.finish())
}

fn capture_imported_reader<R>(
    mut reader: R,
    mut file: fs::File,
    raw_byte_limit: Option<u64>,
    stream: &'static str,
) -> io::Result<PipeCapture>
where
    R: BufRead,
{
    let mut capture = PipeCapture::default();
    let mut summary = summarize::StreamSummaryBuilder::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        summary.push_bytes(&line);
        let text = String::from_utf8_lossy(&line);
        let redacted = summarize::redact_sensitive_text(&text);
        write_capped_import_bytes(&mut file, redacted.as_bytes(), raw_byte_limit, &mut capture)?;
    }
    if durable_logs_enabled() {
        file.sync_all()?;
    }
    capture.summary = summary.finish();
    Ok(capture)
}

fn write_capped_import_bytes(
    file: &mut fs::File,
    bytes: &[u8],
    raw_byte_limit: Option<u64>,
    capture: &mut PipeCapture,
) -> io::Result<()> {
    let writable = match raw_byte_limit {
        Some(limit) => limit
            .saturating_sub(capture.captured_bytes)
            .min(bytes.len() as u64),
        None => bytes.len() as u64,
    } as usize;
    if writable > 0 {
        file.write_all(&bytes[..writable])?;
        capture.captured_bytes += writable as u64;
    }
    if writable < bytes.len() {
        capture.discarded_bytes += (bytes.len() - writable) as u64;
    }
    Ok(())
}

fn should_passthrough(argv: &[String]) -> bool {
    let Some(command) = git_subcommand(argv) else {
        return false;
    };
    match command.name.to_ascii_lowercase().as_str() {
        "describe" | "diff" | "hash-object" | "ls-files" | "rev-parse" | "show" | "status"
        | "tag" => true,
        "log" => command.args.iter().any(|arg| {
            *arg == "--oneline" || *arg == "--format=oneline" || *arg == "--pretty=oneline"
        }),
        _ => false,
    }
}

struct GitSubcommand<'a> {
    name: &'a str,
    args: Vec<&'a str>,
}

fn git_subcommand(argv: &[String]) -> Option<GitSubcommand<'_>> {
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
        return Some(GitSubcommand {
            name: arg,
            args: args.collect(),
        });
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

struct SpawnFailureRecord<'a> {
    paths: &'a Paths,
    run_paths: &'a RunPaths,
    mode: Mode,
    summary_budget: String,
    command: &'a str,
    safe_argv: &'a [String],
    safe_command_identity: &'a str,
    command_kind: &'a str,
    cwd: &'a std::path::Path,
    started: chrono::DateTime<Local>,
    elapsed_duration: Duration,
    failure: &'a str,
    raw_byte_limit: Option<u64>,
}

fn record_spawn_failure(record: SpawnFailureRecord<'_>) {
    let exit_code = 1;
    let elapsed = format_elapsed(record.elapsed_duration.as_millis());
    let failure = summarize::redact_sensitive_text(record.failure);
    let extracted = summarize::extract("", &failure, exit_code);
    let cwd_string = record.cwd.display().to_string();
    let exact_digest = digest::make_exact_digest(
        record.command_kind,
        record.safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let normalized_digest = digest::make_normalized_digest(
        record.command_kind,
        record.safe_command_identity,
        &cwd_string,
        exit_code,
        &extracted,
    );
    let digest = normalized_digest.clone();

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

    let sidecar = SummarySidecar {
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
        exact_digest,
        normalized_digest,
        repeat_status: placeholder_repeat_status(record.run_paths),
        raw_stdout_lines: 0,
        raw_stderr_lines: storage::line_count(&failure),
        raw_total_lines: storage::line_count(&failure),
        raw_stdout_chars: 0,
        raw_stderr_chars: failure.chars().count(),
        raw_total_chars: failure.chars().count(),
        raw_byte_limit: record.raw_byte_limit,
        raw_stdout_truncated: false,
        raw_stderr_truncated: false,
        raw_stdout_discarded_bytes: 0,
        raw_stderr_discarded_bytes: 0,
        shown_lines: 0,
        shown_chars: 0,
        estimated_saved_lines: 0,
        estimated_saved_chars: 0,
        estimated_output_reduction_percent: 0.0,
        estimated_char_reduction_percent: 0.0,
        approx_raw_tokens: approximate_tokens(failure.chars().count()),
        approx_shown_tokens: 0,
        approx_saved_tokens: 0,
        error_count: extracted.error_count,
        warning_count: extracted.warning_count,
        primary_failure: extracted.primary_failure,
        delta: None,
        top_errors: extracted.top_errors,
        top_warnings: extracted.top_warnings,
        file_hits: extracted.file_hits,
        tail: extracted.tail,
        suggested_next_reads: extracted.suggested_next_reads,
        error_windows: extracted.error_windows,
        digest_error_lines: extracted.digest_error_lines,
        digest_file_hits: extracted.digest_file_hits,
        test_or_package_hint: extracted.test_or_package_hint,
        log_path: record.run_paths.log_path.display().to_string(),
        previous_exact_match_run: None,
        started_at: record.started.to_rfc3339(),
        command_kind: record.command_kind.to_string(),
        summary_budget: record.summary_budget,
        capture_mode: "not started; spawn failed".to_string(),
        spawn_error: Some(failure),
        runtime_warnings,
    };

    let entry = IndexEntry {
        index_schema_version: INDEX_SCHEMA_VERSION,
        run_id: record.run_paths.run_id.clone(),
        summary_path: record.run_paths.summary_path.display().to_string(),
        exit_code,
        command_kind: record.command_kind.to_string(),
        command: record.command.to_string(),
        argv: record.safe_argv.to_vec(),
        cwd: cwd_string.clone(),
        started_at: sidecar.started_at.clone(),
        log_path: record.run_paths.log_path.display().to_string(),
    };
    let (_sidecar, display) = commit_run_state(CommitRunState {
        paths: record.paths,
        run_paths: record.run_paths,
        sidecar,
        entry: &entry,
        digest: &digest,
        command_identity: record.safe_command_identity,
        cwd: &cwd_string,
        show_paths: false,
    });
    if record.mode == Mode::Compact {
        print!("{display}");
    }
}

struct CommitRunState<'a> {
    paths: &'a Paths,
    run_paths: &'a RunPaths,
    sidecar: SummarySidecar,
    entry: &'a IndexEntry,
    digest: &'a str,
    command_identity: &'a str,
    cwd: &'a str,
    show_paths: bool,
}

fn commit_run_state(commit: CommitRunState<'_>) -> (SummarySidecar, String) {
    let CommitRunState {
        paths,
        run_paths,
        mut sidecar,
        entry,
        digest,
        command_identity,
        cwd,
        show_paths,
    } = commit;
    let mut display = String::new();
    if let Err(err) = storage::with_state_lock(paths, || {
        sidecar.repeat_status = digest::update_repeat_state_unlocked(
            paths,
            digest,
            command_identity,
            cwd,
            sidecar.exit_code,
            &run_paths.log_path,
            &run_paths.run_id,
        )?;
        display = finalize_sidecar_counts(&mut sidecar, show_paths);
        storage::write_sidecar(&run_paths.summary_path, &sidecar)?;
        storage::record_run_state_unlocked(paths, entry, &sidecar)?;
        Ok(())
    }) {
        if sidecar.repeat_status.message == "pending" {
            sidecar.repeat_status.message = "state unavailable".to_string();
        }
        record_runtime_warning(&mut sidecar.runtime_warnings, "state commit failed", &err);
        display = finalize_sidecar_counts(&mut sidecar, show_paths);
    }
    (sidecar, display)
}

fn finalize_sidecar_counts(sidecar: &mut SummarySidecar, show_paths: bool) -> String {
    for _ in 0..3 {
        let display = summarize::format_compact_with_paths(sidecar, show_paths);
        let shown_lines = storage::line_count(&display);
        let shown_chars = display.chars().count();
        let changed = sidecar.shown_lines != shown_lines || sidecar.shown_chars != shown_chars;

        sidecar.shown_lines = shown_lines;
        sidecar.shown_chars = shown_chars;
        sidecar.estimated_saved_lines = sidecar.raw_total_lines.saturating_sub(shown_lines);
        sidecar.estimated_saved_chars = sidecar.raw_total_chars.saturating_sub(shown_chars);
        sidecar.estimated_output_reduction_percent =
            storage::display_percent(sidecar.estimated_saved_lines, sidecar.raw_total_lines);
        sidecar.estimated_char_reduction_percent =
            storage::display_percent(sidecar.estimated_saved_chars, sidecar.raw_total_chars);
        sidecar.approx_shown_tokens = approximate_tokens(shown_chars);
        sidecar.approx_saved_tokens = sidecar
            .approx_raw_tokens
            .saturating_sub(sidecar.approx_shown_tokens);

        if !changed {
            return display;
        }
    }

    summarize::format_compact_with_paths(sidecar, show_paths)
}

fn record_memory_metric(sidecar: &SummarySidecar) {
    let result = (|| -> Result<()> {
        let paths = Paths::discover()?;
        storage::with_state_lock(&paths, || storage::record_metric_only(&paths, sidecar))
    })();
    if let Err(err) = result {
        eprintln!("kds: metric update failed: {err:#}; wrapped exit code preserved");
    }
}

fn effective_summary_budget(cli_budget: Option<&str>) -> String {
    let selected = cli_budget
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var("KDS_SUMMARY_BUDGET").ok());
    selected
        .as_deref()
        .map(normalize_summary_budget)
        .unwrap_or_else(|| DEFAULT_SUMMARY_BUDGET.to_string())
}

fn normalize_summary_budget(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "tiny" => "tiny".to_string(),
        "normal" => "normal".to_string(),
        "verbose" => "verbose".to_string(),
        "auto" => "auto".to_string(),
        _ => DEFAULT_SUMMARY_BUDGET.to_string(),
    }
}

fn placeholder_repeat_status(run_paths: &RunPaths) -> RepeatStatus {
    RepeatStatus {
        is_repeat: false,
        message: "pending".to_string(),
        first_seen: None,
        previous_log_path: None,
        current_log_path: run_paths.log_path.display().to_string(),
        repeat_count: 0,
    }
}

fn approximate_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

fn record_runtime_warning(warnings: &mut Vec<String>, label: &str, err: &anyhow::Error) {
    let warning = summarize::redact_sensitive_text(&format!("{label}: {err:#}"));
    eprintln!("kds: {warning}; wrapped exit code preserved");
    warnings.push(warning);
}

fn record_import_runtime_warning(warnings: &mut Vec<String>, label: &str, err: &anyhow::Error) {
    let warning = summarize::redact_sensitive_text(&format!("{label}: {err:#}"));
    eprintln!("kds: {warning}; continuing with imported summary");
    warnings.push(warning);
}

fn cleanup_stale_temps(paths: &Paths) {
    let stale_after = stale_temp_after();
    let interval = temp_cleanup_interval();
    match paths.cleanup_stale_temp_files_amortized(stale_after, interval) {
        Ok(None) | Ok(Some(0)) => {}
        Ok(Some(removed)) => eprintln!("kds: removed {removed} stale temp file(s)"),
        Err(err) => eprintln!("kds: stale temp cleanup failed: {err:#}"),
    }
}

fn apply_retention_controls(paths: &Paths) {
    if let Some(days) = retention_days() {
        match storage::gc_artifacts(
            paths,
            Duration::from_secs(days.saturating_mul(24 * 60 * 60)),
            false,
        ) {
            Ok(report) if report.deleted_artifacts > 0 => {
                eprintln!(
                    "kds: pruned {} old artifact(s) by KDS_RETENTION_DAYS",
                    report.deleted_artifacts
                );
                reconcile_after_retention_cleanup(paths);
            }
            Ok(_) => {}
            Err(err) => eprintln!("kds: retention pruning failed: {err:#}"),
        }
    }
    if let Some(days) = compress_after_days() {
        match storage::compress_artifacts_older_than(
            paths,
            Duration::from_secs(days.saturating_mul(24 * 60 * 60)),
        ) {
            Ok(report) if report.deleted_artifacts > 0 => {
                eprintln!(
                    "kds: compressed {} old raw log artifact(s)",
                    report.deleted_artifacts
                );
                reconcile_after_retention_cleanup(paths);
            }
            Ok(_) => {}
            Err(err) => eprintln!("kds: log compression failed: {err:#}"),
        }
    }
    if let Some(max_bytes) = max_total_log_bytes() {
        match storage::prune_to_max_artifact_bytes(paths, max_bytes, false) {
            Ok(report) if report.deleted_artifacts > 0 => {
                eprintln!(
                    "kds: pruned {} artifact(s) by KDS_MAX_TOTAL_LOG_BYTES",
                    report.deleted_artifacts
                );
                reconcile_after_retention_cleanup(paths);
            }
            Ok(_) => {}
            Err(err) => eprintln!("kds: max-log pruning failed: {err:#}"),
        }
    }
}

fn reconcile_after_retention_cleanup(paths: &Paths) {
    if let Err(err) = storage::reconcile_state_after_artifact_cleanup(paths) {
        eprintln!("kds: state reconciliation after cleanup failed: {err:#}");
    }
}

fn retention_days() -> Option<u64> {
    let raw = std::env::var("KDS_RETENTION_DAYS").ok()?;
    match raw.parse::<u64>() {
        Ok(0) => None,
        Ok(days) => Some(days),
        Err(_) => {
            eprintln!("kds: ignoring invalid KDS_RETENTION_DAYS={raw:?}");
            None
        }
    }
}

fn max_total_log_bytes() -> Option<u64> {
    let raw = std::env::var("KDS_MAX_TOTAL_LOG_BYTES").ok()?;
    parse_byte_limit(&raw).or_else(|| {
        eprintln!("kds: ignoring invalid KDS_MAX_TOTAL_LOG_BYTES={raw:?}");
        None
    })
}

fn compress_after_days() -> Option<u64> {
    let raw = std::env::var("KDS_COMPRESS_AFTER_DAYS").ok()?;
    match raw.parse::<u64>() {
        Ok(0) => None,
        Ok(days) => Some(days),
        Err(_) => {
            eprintln!("kds: ignoring invalid KDS_COMPRESS_AFTER_DAYS={raw:?}");
            None
        }
    }
}

fn parse_byte_limit(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "0" {
        return None;
    }
    let split_at = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let value: u64 = trimmed[..split_at].parse().ok()?;
    let unit = trimmed[split_at..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        _ => return None,
    };
    Some(value.saturating_mul(multiplier))
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

fn temp_cleanup_interval() -> Duration {
    match std::env::var("KDS_TEMP_CLEANUP_INTERVAL_SECS") {
        Ok(raw) => match raw.parse::<u64>() {
            Ok(seconds) => Duration::from_secs(seconds),
            Err(_) => {
                eprintln!("kds: ignoring invalid KDS_TEMP_CLEANUP_INTERVAL_SECS={raw:?}");
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

struct ProgressNotice {
    done: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ProgressNotice {
    fn start(mode: Mode, started: Instant) -> Self {
        if mode != Mode::Compact {
            return Self {
                done: None,
                handle: None,
            };
        }
        let Some(after) = progress_notice_after() else {
            return Self {
                done: None,
                handle: None,
            };
        };
        let (done_tx, done_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            if done_rx.recv_timeout(after).is_ok() {
                return;
            }
            eprintln!(
                "KDS: command still running... {} elapsed",
                format_progress_elapsed(started.elapsed())
            );
            eprintln!("KDS: output captured locally, compact summary will print at completion");
            let _ = done_rx.recv();
        });
        Self {
            done: Some(done_tx),
            handle: Some(handle),
        }
    }
}

impl Drop for ProgressNotice {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            let _ = done.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn progress_notice_after() -> Option<Duration> {
    match std::env::var("KDS_PROGRESS_AFTER_SECS") {
        Ok(raw) => match raw.parse::<u64>() {
            Ok(0) => None,
            Ok(seconds) => Some(Duration::from_secs(seconds)),
            Err(_) => {
                eprintln!("kds: ignoring invalid KDS_PROGRESS_AFTER_SECS={raw:?}");
                Some(Duration::from_secs(120))
            }
        },
        Err(_) => Some(Duration::from_secs(120)),
    }
}

fn format_progress_elapsed(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
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
    summary: summarize::StreamSummary,
}

#[derive(Debug, Clone, Copy)]
enum StreamTee {
    Stdout,
    Stderr,
}

const PIPE_BUFFER_SIZE: usize = 64 * 1024;

fn spawn_pipe_copy<R>(
    mut reader: R,
    mut file: fs::File,
    raw_byte_limit: Option<u64>,
    stream: &'static str,
    tee: Option<StreamTee>,
) -> thread::JoinHandle<io::Result<PipeCapture>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut capture = PipeCapture::default();
        let mut summary = summarize::StreamSummaryBuilder::new(stream);
        let mut buffer = [0_u8; PIPE_BUFFER_SIZE];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let raw = &buffer[..read];
            summary.push_bytes(raw);
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
            if let Some(tee) = tee {
                tee_bytes(tee, raw)?;
            }
            if writable < read {
                capture.discarded_bytes += (read - writable) as u64;
            }
        }
        if durable_logs_enabled() {
            file.sync_all()?;
        }
        capture.summary = summary.finish();
        Ok(capture)
    })
}

fn spawn_pipe_summary<R>(
    mut reader: R,
    stream: &'static str,
    tee: Option<StreamTee>,
) -> thread::JoinHandle<io::Result<PipeCapture>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut capture = PipeCapture::default();
        let mut summary = summarize::StreamSummaryBuilder::new(stream);
        let mut buffer = [0_u8; PIPE_BUFFER_SIZE];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let raw = &buffer[..read];
            summary.push_bytes(raw);
            if let Some(tee) = tee {
                tee_bytes(tee, raw)?;
            }
        }
        capture.summary = summary.finish();
        Ok(capture)
    })
}

fn tee_bytes(tee: StreamTee, bytes: &[u8]) -> io::Result<()> {
    match tee {
        StreamTee::Stdout => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(bytes)?;
            stdout.flush()
        }
        StreamTee::Stderr => {
            let mut stderr = io::stderr().lock();
            stderr.write_all(bytes)?;
            stderr.flush()
        }
    }
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

fn raw_byte_limit() -> Option<u64> {
    if uncapped_raw_logs_enabled() {
        return None;
    }
    let Ok(raw) = std::env::var("KDS_MAX_RAW_BYTES") else {
        return Some(DEFAULT_RAW_BYTE_LIMIT);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "0" {
        return None;
    }
    match parse_byte_limit(&raw) {
        Some(0) => None,
        Some(limit) => Some(limit),
        None => {
            eprintln!("kds: ignoring invalid KDS_MAX_RAW_BYTES={raw:?}");
            Some(DEFAULT_RAW_BYTE_LIMIT)
        }
    }
}

fn uncapped_raw_logs_enabled() -> bool {
    matches!(
        std::env::var("KDS_UNCAPPED_RAW_LOGS")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn durable_logs_enabled() -> bool {
    matches!(
        std::env::var("KDS_DURABLE_LOGS")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

struct TempFileCleanup(Vec<std::path::PathBuf>);

impl Drop for TempFileCleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}
