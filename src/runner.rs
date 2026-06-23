use anyhow::Result;
use chrono::Local;
use std::process::Command;
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

pub fn run(argv: Vec<String>, mode: Mode) -> Result<i32> {
    if argv.is_empty() {
        eprintln!("kds: no wrapped command provided");
        return Ok(2);
    }

    let cwd = std::env::current_dir()?;
    let started = Local::now();
    let command = storage::command_string(&argv);
    let safe_command = summarize::redact_sensitive_text(&command);
    let safe_argv = summarize::redact_argv(&argv);
    let paths = Paths::discover()?;
    let run_paths = paths.prepare_run_paths(&safe_argv, &cwd, started)?;
    let command_kind = storage::command_kind(&argv);
    let program = storage::resolve_command(&argv[0]);

    let begin = Instant::now();
    let output = Command::new(&program).args(&argv[1..]).output();
    let elapsed_duration = begin.elapsed();
    let elapsed = format_elapsed(elapsed_duration.as_millis());

    let output = match output {
        Ok(output) => output,
        Err(err) => {
            eprintln!("kds: failed to run `{command}`: {err}");
            return Ok(1);
        }
    };

    let exit_code = match output.status.code() {
        Some(code) => code,
        None => {
            eprintln!("kds: wrapped command did not provide a normal exit code; exiting 1");
            1
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if mode == Mode::Raw {
        print!("{stdout}");
        eprint!("{stderr}");
    }

    let raw_stdout_lines = storage::line_count(&stdout);
    let raw_stderr_lines = storage::line_count(&stderr);
    let raw_total_lines = raw_stdout_lines + raw_stderr_lines;
    let extracted = summarize::extract(&stdout, &stderr, exit_code);
    let cwd_string = cwd.display().to_string();
    let digest = digest::make_digest(
        &command_kind,
        &safe_command,
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
    if let Err(err) = storage::write_raw_log(storage::RawLog {
        path: &run_paths.log_path,
        sidecar_hint: log_hint,
        command: &command,
        cwd: &cwd,
        stdout: &output.stdout,
        stderr: &output.stderr,
        exit_code,
        elapsed: &elapsed,
    }) {
        eprintln!("kds: raw log write failed: {err:#}");
    }

    let repeat_status = match digest::update_repeat_state(
        &paths,
        &digest,
        &safe_command,
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

    let display_once = summarize::format_compact(&sidecar);
    sidecar.shown_lines = storage::line_count(&display_once);
    sidecar.estimated_saved_lines = raw_total_lines.saturating_sub(sidecar.shown_lines);
    sidecar.estimated_output_reduction_percent =
        storage::display_percent(sidecar.estimated_saved_lines, raw_total_lines);
    let display = summarize::format_compact(&sidecar);

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
