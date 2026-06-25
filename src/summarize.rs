use regex::Regex;
use std::collections::VecDeque;
use std::sync::OnceLock;

use crate::cli::SummaryBudget;
use crate::storage::{ErrorWindow, SummarySidecar};

const COMPACT_RUN_HEADER: &str = "KDS";

#[derive(Debug, Clone)]
pub struct ExtractedSummary {
    pub error_count: usize,
    pub warning_count: usize,
    pub primary_failure: Option<String>,
    pub top_errors: Vec<String>,
    pub file_hits: Vec<String>,
    pub tail: Vec<String>,
    pub suggested_next_reads: Vec<String>,
    pub error_windows: Vec<ErrorWindow>,
    pub digest_error_lines: Vec<String>,
    pub digest_file_hits: Vec<String>,
    pub test_or_package_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedOutput {
    pub summary: ExtractedSummary,
    pub stdout_lines: usize,
    pub stderr_lines: usize,
    pub stdout_chars: usize,
    pub stderr_chars: usize,
}

#[derive(Debug, Clone, Default)]
pub struct StreamSummary {
    pub builder: SummaryBuilder,
    pub line_count: usize,
    pub char_count: usize,
}

pub fn extract(stdout: &str, stderr: &str, exit_code: i32) -> ExtractedSummary {
    let mut builder = SummaryBuilder::new();
    for line in stdout.lines() {
        builder.push_stream_line("stdout", line);
    }
    for line in stderr.lines() {
        builder.push_stream_line("stderr", line);
    }
    builder.finish(exit_code)
}

#[derive(Debug, Clone, Default)]
pub struct SummaryBuilder {
    warning_count: usize,
    error_count: usize,
    top_errors: Vec<String>,
    file_hits: Vec<String>,
    tail: VecDeque<String>,
    before: VecDeque<String>,
    line_number: usize,
    error_windows: Vec<ErrorWindow>,
    pending_error_windows: Vec<(usize, usize)>,
    test_or_package_hint: Option<String>,
}

impl SummaryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_stream_line(&mut self, stream: &str, raw_line: &str) {
        let line = redact_sensitive_text(&strip_ansi(raw_line))
            .trim_end()
            .to_string();
        self.line_number += 1;
        let pending = std::mem::take(&mut self.pending_error_windows);
        for (index, remaining) in pending {
            if remaining > 0 {
                if let Some(window) = self.error_windows.get_mut(index) {
                    window.after.push(line.clone());
                }
                if remaining > 1 {
                    self.pending_error_windows.push((index, remaining - 1));
                }
            }
        }
        let lower = line.to_ascii_lowercase();
        if lower.contains("warning:")
            || lower.starts_with("warn ")
            || lower.starts_with("npm warn ")
            || lower.contains(" npm warn ")
        {
            self.warning_count += 1;
        }
        if is_error_line(&line) {
            self.error_count += 1;
        }
        if !line.trim().is_empty() {
            if self.test_or_package_hint.is_none() {
                self.test_or_package_hint = detect_test_or_package_hint(&line);
            }
            if is_error_line(&line) {
                self.push_error_window(stream, &line);
                push_unique_cap(&mut self.top_errors, line.clone(), 8);
            }
            for hit in extract_file_hits(&line, 10) {
                push_unique_cap(&mut self.file_hits, hit, 10);
            }
            self.tail.push_back(line.clone());
            if self.tail.len() > 40 {
                self.tail.pop_front();
            }
            self.before.push_back(line);
            if self.before.len() > 3 {
                self.before.pop_front();
            }
        }
    }

    pub fn merge(&mut self, _line_count: usize, other: SummaryBuilder) {
        self.warning_count += other.warning_count;
        self.error_count += other.error_count;
        for item in other.top_errors {
            push_unique_cap(&mut self.top_errors, item, 8);
        }
        for item in other.file_hits {
            push_unique_cap(&mut self.file_hits, item, 10);
        }
        for line in other.tail {
            self.tail.push_back(line);
            if self.tail.len() > 40 {
                self.tail.pop_front();
            }
        }
        if self.test_or_package_hint.is_none() {
            self.test_or_package_hint = other.test_or_package_hint;
        }
        for window in other.error_windows {
            if self.error_windows.len() < 3 {
                self.error_windows.push(window);
            }
        }
    }

    pub fn finish(mut self, exit_code: i32) -> ExtractedSummary {
        if self.top_errors.is_empty() && exit_code != 0 {
            for line in self
                .tail
                .iter()
                .rev()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                push_unique_cap(&mut self.top_errors, line, 8);
            }
        }
        let suggested_next_reads = self.file_hits.iter().take(5).cloned().collect();
        let primary_failure = self.top_errors.first().cloned();
        let digest_error_lines = self
            .top_errors
            .iter()
            .take(3)
            .map(|line| normalize_digest_signal(line))
            .collect();
        let digest_file_hits = self
            .file_hits
            .iter()
            .take(3)
            .map(|line| normalize_digest_signal(line))
            .collect();
        ExtractedSummary {
            error_count: self.error_count,
            warning_count: self.warning_count,
            primary_failure,
            top_errors: self.top_errors,
            file_hits: self.file_hits,
            tail: self.tail.into_iter().collect(),
            suggested_next_reads,
            error_windows: self.error_windows,
            digest_error_lines,
            digest_file_hits,
            test_or_package_hint: self.test_or_package_hint,
        }
    }

    fn push_error_window(&mut self, stream: &str, line: &str) {
        if self.error_windows.len() >= 3 {
            return;
        }
        let index = self.error_windows.len();
        self.error_windows.push(ErrorWindow {
            stream: stream.to_string(),
            line: self.line_number,
            before: self.before.iter().cloned().collect(),
            matched: line.to_string(),
            after: Vec::new(),
        });
        self.pending_error_windows.push((index, 3));
    }
}

#[derive(Debug, Clone, Default)]
pub struct StreamSummaryBuilder {
    builder: SummaryBuilder,
    pending: Vec<u8>,
    line_count: usize,
    char_count: usize,
    stream: &'static str,
}

impl StreamSummaryBuilder {
    pub fn new(stream: &'static str) -> Self {
        Self {
            stream,
            ..Self::default()
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if *byte == b'\n' {
                self.flush_pending_line();
            } else {
                self.pending.push(*byte);
            }
        }
    }

    pub fn finish(mut self) -> StreamSummary {
        if !self.pending.is_empty() {
            self.flush_pending_line();
        }
        StreamSummary {
            builder: self.builder,
            line_count: self.line_count,
            char_count: self.char_count,
        }
    }

    fn flush_pending_line(&mut self) {
        if self.pending.last() == Some(&b'\r') {
            let _ = self.pending.pop();
        }
        let line = String::from_utf8_lossy(&self.pending);
        self.char_count += line.chars().count();
        self.builder.push_stream_line(self.stream, &line);
        self.line_count += 1;
        self.pending.clear();
    }
}

pub fn merge_stream_summaries(
    stdout: StreamSummary,
    stderr: StreamSummary,
    exit_code: i32,
) -> ExtractedOutput {
    let mut builder = SummaryBuilder::new();
    builder.merge(stdout.line_count, stdout.builder);
    builder.merge(stderr.line_count, stderr.builder);
    ExtractedOutput {
        summary: builder.finish(exit_code),
        stdout_lines: stdout.line_count,
        stderr_lines: stderr.line_count,
        stdout_chars: stdout.char_count,
        stderr_chars: stderr.char_count,
    }
}

fn strip_ansi(text: &str) -> String {
    static ANSI_RE: OnceLock<Regex> = OnceLock::new();
    let re = ANSI_RE.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap());
    re.replace_all(text, "").to_string()
}

pub fn redact_sensitive_text(text: &str) -> String {
    if !needs_redaction_scan(text) {
        return text.to_string();
    }
    let mut redacted = text.to_string();
    for (pattern, replacement) in [
        (url_credentials_re(), "${1}[redacted]@"),
        (flag_assignment_re(), "${1}${2}[redacted]"),
        (flag_value_re(), "${1}${2}[redacted]"),
        (authorization_re(), "${1}[redacted]"),
        (keyed_secret_re(), "${1}${2}[redacted]${3}"),
        (bearer_re(), "${1}[redacted]"),
        (known_secret_re(), "[redacted-secret]"),
    ] {
        redacted = pattern.replace_all(&redacted, replacement).to_string();
    }
    redacted
}

fn needs_redaction_scan(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("pwd")
        || lower.contains("authorization")
        || lower.contains("bearer")
        || lower.contains("api")
        || lower.contains("://")
        || lower.contains("ghp_")
        || lower.contains("github_pat_")
        || lower.contains("glpat-")
        || lower.contains("sk-")
        || lower.contains("rk_live")
        || lower.contains("rk_test")
        || lower.contains("sk_live")
        || lower.contains("sk_test")
        || lower.contains("akia")
        || lower.contains("asia")
        || lower.contains("aiza")
        || lower.contains("xox")
        || lower.contains("npm_")
        || lower.contains("eyj")
        || looks_like_dot_secret(text)
}

fn looks_like_dot_secret(text: &str) -> bool {
    text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' && ch != '.')
        .any(|token| {
            let mut parts = token.split('.');
            let Some(first) = parts.next() else {
                return false;
            };
            let Some(second) = parts.next() else {
                return false;
            };
            let Some(third) = parts.next() else {
                return false;
            };
            parts.next().is_none()
                && (23..=28).contains(&first.len())
                && (6..=10).contains(&second.len())
                && third.len() >= 27
                && first
                    .chars()
                    .chain(second.chars())
                    .chain(third.chars())
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        })
}

pub fn redact_argv(argv: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(argv.len());
    let mut redact_next = false;
    for arg in argv {
        if redact_next {
            redacted.push("[redacted]".to_string());
            redact_next = false;
            continue;
        }
        if is_sensitive_flag(arg) {
            redacted.push(arg.clone());
            redact_next = true;
            continue;
        }
        redacted.push(redact_sensitive_text(arg));
    }
    redacted
}

fn is_sensitive_flag(arg: &str) -> bool {
    if !arg.starts_with('-') || arg.contains('=') {
        return false;
    }
    let normalized = arg
        .trim_start_matches('-')
        .to_ascii_lowercase()
        .replace('_', "-");
    matches!(
        normalized.as_str(),
        "api-key"
            | "token"
            | "access-token"
            | "auth-token"
            | "refresh-token"
            | "id-token"
            | "secret"
            | "client-secret"
            | "password"
            | "passwd"
            | "pwd"
    )
}

fn url_credentials_re() -> &'static Regex {
    static URL_CREDENTIALS_RE: OnceLock<Regex> = OnceLock::new();
    URL_CREDENTIALS_RE
        .get_or_init(|| Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@").unwrap())
}

fn flag_assignment_re() -> &'static Regex {
    static FLAG_ASSIGNMENT_RE: OnceLock<Regex> = OnceLock::new();
    FLAG_ASSIGNMENT_RE.get_or_init(|| {
        Regex::new(
            r"(?i)(^|\s)(--[A-Za-z0-9_-]*(?:api[-_]?key|token|secret|password|passwd|pwd)[A-Za-z0-9_-]*=)[^\s]+",
        )
        .unwrap()
    })
}

fn flag_value_re() -> &'static Regex {
    static FLAG_VALUE_RE: OnceLock<Regex> = OnceLock::new();
    FLAG_VALUE_RE.get_or_init(|| {
        Regex::new(
            r"(?i)(^|\s)(--[A-Za-z0-9_-]*(?:api[-_]?key|token|secret|password|passwd|pwd)[A-Za-z0-9_-]*\s+)[^\s]+",
        )
        .unwrap()
    })
}

fn authorization_re() -> &'static Regex {
    static AUTHORIZATION_RE: OnceLock<Regex> = OnceLock::new();
    AUTHORIZATION_RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(authorization\s*[:=]\s*)(?:bearer\s+)?[^\s'",;]+"#).unwrap()
    })
}

fn keyed_secret_re() -> &'static Regex {
    static KEYED_SECRET_RE: OnceLock<Regex> = OnceLock::new();
    KEYED_SECRET_RE.get_or_init(|| {
        Regex::new(
            r#"(?i)\b([A-Z0-9_-]*(?:API[-_]?KEY|TOKEN|SECRET|PASSWORD|PASSWD|PWD)[A-Z0-9_-]*\s*[:=]\s*)(['"]?)[^\s'",;]+(['"]?)"#,
        )
        .unwrap()
    })
}

fn bearer_re() -> &'static Regex {
    static BEARER_RE: OnceLock<Regex> = OnceLock::new();
    BEARER_RE.get_or_init(|| Regex::new(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{8,}").unwrap())
}

fn known_secret_re() -> &'static Regex {
    static KNOWN_SECRET_RE: OnceLock<Regex> = OnceLock::new();
    KNOWN_SECRET_RE.get_or_init(|| {
        Regex::new(
            r"\b(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9_-]{20,}|(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}|AKIA[0-9A-Z]{16}|ASIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{30,}|xox(?:b|p|a|r|s)-[A-Za-z0-9-]{10,}|npm_[A-Za-z0-9]{20,}|[A-Za-z0-9_-]{23,28}\.[A-Za-z0-9_-]{6,10}\.[A-Za-z0-9_-]{27,}|eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})\b",
        )
        .unwrap()
    })
}

pub fn format_compact_with_paths(sidecar: &SummarySidecar, show_paths: bool) -> String {
    format_compact_with_budget(sidecar, show_paths, None)
}

pub fn format_compact_with_budget(
    sidecar: &SummarySidecar,
    show_paths: bool,
    budget: Option<SummaryBudget>,
) -> String {
    apply_output_budget(
        format_compact_unbounded(sidecar, show_paths, budget),
        budget,
        &sidecar.run_id,
    )
}

pub fn compact_line_count_with_budget(
    sidecar: &SummarySidecar,
    show_paths: bool,
    budget: Option<SummaryBudget>,
) -> usize {
    storage_line_count(&format_compact_with_budget(sidecar, show_paths, budget))
}

fn format_compact_unbounded(
    sidecar: &SummarySidecar,
    show_paths: bool,
    budget: Option<SummaryBudget>,
) -> String {
    if sidecar.exit_code == 0 && sidecar.warning_count == 0 {
        let mut out = format!(
            "{COMPACT_RUN_HEADER}\nRun ID: {}\nExit code: 0\nElapsed: {}\n{}\nEstimated output reduction: {} lines ({:.1}%)\nSummary: success\nNext action: {}\nWarnings: 0\n",
            sidecar.run_id,
            sidecar.elapsed,
            log_line(sidecar, show_paths),
            sidecar.estimated_saved_lines,
            sidecar.estimated_output_reduction_percent,
            next_action(sidecar)
        );
        append_runtime_warnings(&mut out, sidecar);
        return out;
    }

    if sidecar.repeat_status.is_repeat && sidecar.exit_code != 0 && !digest_changed(sidecar) {
        let mut out = String::new();
        out.push_str(COMPACT_RUN_HEADER);
        out.push('\n');
        out.push_str(&format!("Run ID: {}\n", sidecar.run_id));
        out.push_str(&format!("Exit code: {}\n", sidecar.exit_code));
        out.push_str(&format!("Elapsed: {}\n", sidecar.elapsed));
        out.push_str(&format!("Repeat: {}\n", sidecar.repeat_status.message));
        if let Some(delta) = &sidecar.delta {
            out.push_str(&format!("Changed since previous run: {delta}\n"));
        }
        out.push_str(&format!(
            "Primary failure: {}\n",
            sidecar
                .primary_failure
                .as_deref()
                .map(|text| display_text(text, sidecar, show_paths))
                .unwrap_or_else(|| "none".to_string())
        ));
        out.push_str(&format!("Next action: {}\n", next_action(sidecar)));
        out.push_str("Suggested next read:\n");
        write_list(&mut out, &suggested_next_commands(sidecar), 3);
        out.push_str(&format!(
            "Estimated savings: {} lines ({:.1}%)\n",
            sidecar.estimated_saved_lines, sidecar.estimated_output_reduction_percent
        ));
        out.push_str(&format!("{}\n", log_line(sidecar, show_paths)));
        append_runtime_warnings(&mut out, sidecar);
        return out;
    }

    let caps = display_caps(budget);
    let mut out = String::new();
    out.push_str(COMPACT_RUN_HEADER);
    out.push('\n');
    out.push_str(&format!("Run ID: {}\n", sidecar.run_id));
    out.push_str(&format!(
        "Command: {}\n",
        display_text(&sidecar.command, sidecar, show_paths)
    ));
    if show_paths {
        out.push_str(&format!("CWD: {}\n", sidecar.cwd));
    }
    out.push_str(&format!("Exit code: {}\n", sidecar.exit_code));
    out.push_str(&format!("Elapsed: {}\n", sidecar.elapsed));
    out.push_str(&format!("{}\n", log_line(sidecar, show_paths)));
    out.push_str(&format!("Digest: {}\n", sidecar.digest));
    out.push_str(&format!("Repeat: {}\n", sidecar.repeat_status.message));
    out.push_str(&format!(
        "Estimated savings: {} lines ({:.1}%)\n",
        sidecar.estimated_saved_lines, sidecar.estimated_output_reduction_percent
    ));
    if sidecar.exit_code == 0 {
        out.push_str("Summary: success with warnings\n");
    } else {
        out.push_str("Summary: failed; compact evidence follows\n");
    }
    if let Some(delta) = &sidecar.delta {
        out.push_str(&format!("Changed since previous run: {delta}\n"));
    }
    out.push_str(&format!("Next action: {}\n", next_action(sidecar)));
    out.push_str("Top errors:\n");
    write_list(
        &mut out,
        &display_list(&sidecar.top_errors, sidecar, show_paths),
        caps.top_errors,
    );
    out.push_str("File hits:\n");
    write_list(
        &mut out,
        &display_list(&sidecar.file_hits, sidecar, show_paths),
        caps.file_hits,
    );
    out.push_str(&format!("Warnings: {}\n", sidecar.warning_count));
    out.push_str("Final tail:\n");
    write_list(
        &mut out,
        &display_list(&sidecar.tail, sidecar, show_paths),
        caps.tail,
    );
    out.push_str("Suggested next read:\n");
    write_list(&mut out, &suggested_next_commands(sidecar), caps.suggested);
    append_runtime_warnings(&mut out, sidecar);
    out
}

pub fn format_safe_metadata_with_paths(sidecar: &SummarySidecar, show_paths: bool) -> String {
    let mut out = format!(
        "KDS run\nRun ID: {}\nCommand: {}\nExit code: {}\nElapsed: {}\nCapture: {}\n{}\nDigest: {}\nRepeat: {}\nAvailable:\n  --summary\n  --errors\n  --error-window\n  --tail\n  --file-hits\nWarning: raw logs may contain secrets, paths, tokens, stack traces, environment values, or file contents.\n",
        sidecar.run_id,
        display_text(&sidecar.command, sidecar, show_paths),
        sidecar.exit_code,
        sidecar.elapsed,
        sidecar.capture_mode,
        log_line(sidecar, show_paths),
        sidecar.digest,
        sidecar.repeat_status.message
    );
    append_runtime_warnings(&mut out, sidecar);
    out
}

pub fn format_evidence_with_paths(sidecar: &SummarySidecar, show_paths: bool) -> String {
    let mut out = String::new();
    out.push_str("KDS evidence\n");
    out.push_str(&format!("Run ID: {}\n", sidecar.run_id));
    out.push_str(&format!(
        "Command: {}\n",
        display_text(&sidecar.command, sidecar, show_paths)
    ));
    out.push_str(&format!("Exit code: {}\n", sidecar.exit_code));
    out.push_str(&format!("Digest: {}\n", sidecar.digest));
    out.push_str(&format!("Repeat: {}\n", sidecar.repeat_status.message));
    if let Some(delta) = &sidecar.delta {
        out.push_str(&format!("Changed since previous run: {delta}\n"));
    }
    out.push_str(&format!("Next action: {}\n", next_action(sidecar)));
    out.push_str("Top errors:\n");
    write_list(
        &mut out,
        &display_list(&sidecar.top_errors, sidecar, show_paths),
        3,
    );
    out.push_str("File hits:\n");
    write_list(
        &mut out,
        &display_list(&sidecar.file_hits, sidecar, show_paths),
        5,
    );
    out.push_str("Suggested next reads:\n");
    write_list(&mut out, &suggested_next_commands(sidecar), 5);
    out.push_str(&format!("{}\n", log_line(sidecar, show_paths)));
    out.push_str(&format!(
        "Estimated output reduction: {} lines ({:.1}%)\n",
        sidecar.estimated_saved_lines, sidecar.estimated_output_reduction_percent
    ));
    append_runtime_warnings(&mut out, sidecar);
    out
}

pub fn display_items_for_paths(
    sidecar: &SummarySidecar,
    items: &[String],
    show_paths: bool,
) -> Vec<String> {
    display_list(items, sidecar, show_paths)
}

pub fn delta_line(
    previous: Option<&SummarySidecar>,
    current: &ExtractedSummary,
    exit_code: i32,
    digest_changed: bool,
) -> Option<String> {
    let previous = previous?;
    let previous_errors: std::collections::BTreeSet<_> = previous.top_errors.iter().collect();
    let current_errors: std::collections::BTreeSet<_> = current.top_errors.iter().collect();
    let new_errors = current_errors.difference(&previous_errors).count();
    let resolved_errors = previous_errors.difference(&current_errors).count();
    let previous_files: std::collections::BTreeSet<_> = previous.file_hits.iter().collect();
    let current_files: std::collections::BTreeSet<_> = current.file_hits.iter().collect();
    let changed_files = previous_files != current_files;
    let warning_delta = current.warning_count as isize - previous.warning_count as isize;
    Some(format!(
        "exit code {}; digest {}; {} new error signal(s); {} resolved error signal(s); file hits {}; warnings {:+}",
        if previous.exit_code == exit_code { "same" } else { "changed" },
        if digest_changed { "changed" } else { "unchanged" },
        new_errors,
        resolved_errors,
        if changed_files { "changed" } else { "same" },
        warning_delta
    ))
}

fn is_error_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("error[")
        || lower.contains("error:")
        || lower.contains(": error ")
        || lower.contains(" error ts")
        || lower.starts_with("error ")
        || lower.starts_with("fail ")
        || lower.contains("err!")
        || lower.contains("error: ")
        || lower.contains("panicked at")
        || lower.contains("assertionerror")
        || lower.contains("typeerror")
        || lower.contains("could not compile")
        || lower.contains("failed to")
        || lower.starts_with("failed ")
        || lower.starts_with("traceback ")
        || lower.starts_with("e   ")
        || lower.contains("exception")
        || lower.contains("fatal:")
}

fn extract_file_hits(text: &str, cap: usize) -> Vec<String> {
    let mut hits = Vec::new();
    for caps in file_hit_re().captures_iter(text) {
        let mut hit = format!("{}:{}", &caps[1], &caps[2]);
        if let Some(col) = caps.get(3) {
            hit.push(':');
            hit.push_str(col.as_str());
        }
        push_unique_cap(&mut hits, hit, cap);
        if hits.len() >= cap {
            break;
        }
    }
    for caps in paren_file_hit_re().captures_iter(text) {
        let hit = format!("{}:{}:{}", &caps[1], &caps[2], &caps[3]);
        push_unique_cap(&mut hits, hit, cap);
        if hits.len() >= cap {
            break;
        }
    }
    for caps in pytest_node_re().captures_iter(text) {
        let hit = format!("{}::{}", &caps[1], &caps[2]);
        push_unique_cap(&mut hits, hit, cap);
        if hits.len() >= cap {
            break;
        }
    }
    for caps in fail_file_re().captures_iter(text) {
        push_unique_cap(&mut hits, caps[1].trim().to_string(), cap);
        if hits.len() >= cap {
            break;
        }
    }
    hits
}

fn file_hit_re() -> &'static Regex {
    static FILE_HIT_RE: OnceLock<Regex> = OnceLock::new();
    FILE_HIT_RE.get_or_init(|| {
        Regex::new(
            r"(?m)([A-Za-z]:\\[^:\r\n]+|(?:\.{0,2}[\\/])?[\w .\-/\\]+\.[A-Za-z0-9_]+):(\d+)(?::(\d+))?",
        )
        .unwrap()
    })
}

fn paren_file_hit_re() -> &'static Regex {
    static PAREN_FILE_HIT_RE: OnceLock<Regex> = OnceLock::new();
    PAREN_FILE_HIT_RE.get_or_init(|| {
        Regex::new(
            r"(?m)([A-Za-z]:\\[^(\r\n]+|(?:\.{0,2}[\\/])?[\w .\-/\\]+\.[A-Za-z0-9_]+)\((\d+),(\d+)\)",
        )
        .unwrap()
    })
}

fn pytest_node_re() -> &'static Regex {
    static PYTEST_NODE_RE: OnceLock<Regex> = OnceLock::new();
    PYTEST_NODE_RE.get_or_init(|| {
        Regex::new(r"(?m)(?:^|\s)((?:\.{0,2}[\\/])?[\w.\-/\\]+\.py)::([A-Za-z_][\w\[\].:-]*)")
            .unwrap()
    })
}

fn fail_file_re() -> &'static Regex {
    static FAIL_FILE_RE: OnceLock<Regex> = OnceLock::new();
    FAIL_FILE_RE.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:FAIL|FAILED)\s+((?:\.{0,2}[\\/])?[\w .\-/\\]+\.[A-Za-z0-9_]+)")
            .unwrap()
    })
}

fn log_line(sidecar: &SummarySidecar, show_paths: bool) -> String {
    if show_paths {
        format!("Log: {}", sidecar.log_path)
    } else {
        format!(
            "Log: use `kds logs show {} --show-paths` or `kds logs dir`",
            sidecar.run_id
        )
    }
}

fn next_action(sidecar: &SummarySidecar) -> &'static str {
    if sidecar.spawn_error.is_some() {
        return "spawn failure";
    }
    if sidecar.exit_code == 0 && sidecar.warning_count == 0 {
        return "success";
    }
    if sidecar.exit_code == 0 {
        return "success with warnings";
    }
    if sidecar.repeat_status.is_repeat {
        return "repeat failure";
    }

    let evidence = sidecar
        .top_errors
        .iter()
        .chain(sidecar.tail.iter())
        .take(20)
        .map(|line| line.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    if evidence.trim().is_empty() {
        return "raw log needed";
    }
    if evidence.contains("command not found")
        || evidence.contains("no such file or directory")
        || evidence.contains("cannot find module")
        || evidence.contains("module not found")
        || evidence.contains("no module named")
        || evidence.contains("could not find")
        || evidence.contains("unresolved import")
        || evidence.contains("failed to resolve")
    {
        return "likely missing dependency";
    }
    if evidence.contains("could not compile")
        || evidence.contains("error[")
        || evidence.contains("error ts")
        || evidence.contains("failed to compile")
        || evidence.contains("compilation failed")
    {
        return "likely compile error";
    }
    if evidence.contains("assertionerror")
        || evidence.contains("assertion failed")
        || evidence.contains("panicked at")
        || evidence.contains("test result: failed")
        || evidence.contains("failures:")
        || evidence.contains(" expected ")
    {
        return "likely test assertion failure";
    }
    "new failure"
}

#[derive(Debug, Clone, Copy)]
struct DisplayCaps {
    top_errors: usize,
    file_hits: usize,
    tail: usize,
    suggested: usize,
    lines: usize,
    chars: usize,
}

fn display_caps(budget: Option<SummaryBudget>) -> DisplayCaps {
    let mut caps = match budget.or_else(budget_from_env) {
        Some(SummaryBudget::Tight) => DisplayCaps {
            top_errors: 2,
            file_hits: 3,
            tail: 6,
            suggested: 2,
            lines: 18,
            chars: 2500,
        },
        Some(SummaryBudget::Wide) => DisplayCaps {
            top_errors: 5,
            file_hits: 10,
            tail: 40,
            suggested: 5,
            lines: 80,
            chars: 12000,
        },
        _ => DisplayCaps {
            top_errors: 3,
            file_hits: 5,
            tail: 12,
            suggested: 3,
            lines: 30,
            chars: 4000,
        },
    };
    if let Ok(raw) = std::env::var("KDS_SUMMARY_BUDGET_LINES") {
        if let Ok(lines) = raw.parse::<usize>() {
            caps.lines = lines.max(6);
        }
    }
    if let Ok(raw) = std::env::var("KDS_SUMMARY_BUDGET_CHARS") {
        if let Ok(chars) = raw.parse::<usize>() {
            caps.chars = chars.max(500);
        }
    }
    caps
}

fn budget_from_env() -> Option<SummaryBudget> {
    match std::env::var("KDS_SUMMARY_BUDGET")
        .ok()
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("tight") => Some(SummaryBudget::Tight),
        Some("wide") => Some(SummaryBudget::Wide),
        Some("normal") => Some(SummaryBudget::Normal),
        _ => None,
    }
}

fn apply_output_budget(text: String, budget: Option<SummaryBudget>, run_id: &str) -> String {
    let caps = display_caps(budget);
    let mut out = String::new();
    let mut used_chars = 0;
    let mut truncated = false;
    for (index, line) in text.lines().enumerate() {
        let next_chars = line.chars().count() + 1;
        if index >= caps.lines || used_chars + next_chars > caps.chars {
            truncated = true;
            break;
        }
        out.push_str(line);
        out.push('\n');
        used_chars += next_chars;
    }
    if truncated {
        out.push_str(&format!(
            "Summary budget reached; use `kds logs show {run_id} --errors` or `--error-window` for more.\n",
        ));
    }
    out
}

fn storage_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

fn digest_changed(sidecar: &SummarySidecar) -> bool {
    sidecar
        .delta
        .as_deref()
        .map(|delta| delta.contains("digest changed"))
        .unwrap_or(false)
}

fn suggested_next_commands(sidecar: &SummarySidecar) -> Vec<String> {
    let mut commands = vec![
        format!("kds logs show {} --errors", sidecar.run_id),
        format!("kds logs show {} --error-window", sidecar.run_id),
        format!("kds logs show {} --file-hits", sidecar.run_id),
    ];
    if sidecar.top_errors.is_empty() && sidecar.error_windows.is_empty() {
        commands[0] = format!("kds logs show {} --tail", sidecar.run_id);
    }
    commands
}

fn display_list(items: &[String], sidecar: &SummarySidecar, show_paths: bool) -> Vec<String> {
    items
        .iter()
        .map(|item| display_text(item, sidecar, show_paths))
        .collect()
}

fn display_text(text: &str, sidecar: &SummarySidecar, show_paths: bool) -> String {
    if show_paths {
        return text.to_string();
    }
    let mut out = text.to_string();
    out = replace_path_prefix(&out, &sidecar.cwd, "<cwd>");
    if let Some(home) = home_dir_string() {
        out = replace_path_prefix(&out, &home, "~");
    }
    out
}

fn replace_path_prefix(text: &str, prefix: &str, replacement: &str) -> String {
    if prefix.is_empty() {
        return text.to_string();
    }
    let mut out = text.replace(prefix, replacement);
    let slash_prefix = prefix.replace('\\', "/");
    if slash_prefix != prefix {
        out = out.replace(&slash_prefix, replacement);
    }
    let backslash_prefix = prefix.replace('/', "\\");
    if backslash_prefix != prefix {
        out = out.replace(&backslash_prefix, replacement);
    }
    out
}

fn home_dir_string() -> Option<String> {
    std::env::var("USERPROFILE")
        .ok()
        .filter(|path| !path.is_empty())
        .or_else(|| std::env::var("HOME").ok().filter(|path| !path.is_empty()))
}

fn push_unique_cap(items: &mut Vec<String>, item: String, cap: usize) {
    if items.len() < cap && !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
}

fn normalize_digest_signal(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

fn detect_test_or_package_hint(line: &str) -> Option<String> {
    if let Some(caps) = rust_test_stdout_re().captures(line) {
        return Some(format!("rust test {}", normalize_digest_signal(&caps[1])));
    }
    if let Some(caps) = could_not_compile_re().captures(line) {
        return Some(format!(
            "cargo package {}",
            normalize_digest_signal(&caps[1])
        ));
    }
    if let Some(caps) = pytest_node_re().captures(line) {
        return Some(format!(
            "pytest {}::{}",
            normalize_digest_signal(&caps[1]),
            normalize_digest_signal(&caps[2])
        ));
    }
    None
}

fn rust_test_stdout_re() -> &'static Regex {
    static RUST_TEST_STDOUT_RE: OnceLock<Regex> = OnceLock::new();
    RUST_TEST_STDOUT_RE.get_or_init(|| Regex::new(r"^----\s+(.+?)\s+stdout\s+----$").unwrap())
}

fn could_not_compile_re() -> &'static Regex {
    static COULD_NOT_COMPILE_RE: OnceLock<Regex> = OnceLock::new();
    COULD_NOT_COMPILE_RE
        .get_or_init(|| Regex::new(r#"(?i)could not compile [`']([^`']+)[`']"#).unwrap())
}

fn write_list(out: &mut String, items: &[String], cap: usize) {
    if items.is_empty() {
        out.push_str("  none\n");
        return;
    }
    for item in items.iter().take(cap) {
        out.push_str("  ");
        out.push_str(item);
        out.push('\n');
    }
}

fn append_runtime_warnings(out: &mut String, sidecar: &SummarySidecar) {
    if sidecar.runtime_warnings.is_empty() {
        return;
    }
    out.push_str("Runtime warnings:\n");
    write_list(out, &sidecar.runtime_warnings, 5);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RepeatStatus;

    fn sidecar_for_display() -> SummarySidecar {
        SummarySidecar {
            summary_schema_version: 1,
            kds_version: "0.1.0".into(),
            run_id: "run-123".into(),
            summary_path: "C:\\Users\\tester\\repo\\.kds\\run.summary.json".into(),
            command: "cargo test".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: "C:\\Users\\tester\\repo".into(),
            mode: "compact".into(),
            exit_code: 1,
            elapsed: "10ms".into(),
            elapsed_ms: 10,
            digest: "digest".into(),
            exact_digest: "exact".into(),
            normalized_digest: "digest".into(),
            repeat_status: RepeatStatus {
                is_repeat: false,
                message: "new failure signal".into(),
                first_seen: None,
                previous_log_path: None,
                current_log_path: "C:\\Users\\tester\\kds\\run.log".into(),
                repeat_count: 0,
            },
            raw_stdout_lines: 10,
            raw_stderr_lines: 5,
            raw_total_lines: 15,
            raw_stdout_chars: 100,
            raw_stderr_chars: 50,
            raw_total_chars: 150,
            shown_lines: 0,
            shown_chars: 0,
            estimated_saved_lines: 5,
            estimated_saved_chars: 50,
            estimated_output_reduction_percent: 33.3,
            estimated_char_reduction_percent: 33.3,
            approx_raw_tokens: 38,
            approx_shown_tokens: 25,
            approx_saved_tokens: 13,
            error_count: 1,
            warning_count: 0,
            primary_failure: Some("error: C:\\Users\\tester\\repo\\src\\main.rs:1".into()),
            delta: None,
            top_errors: vec!["error: C:\\Users\\tester\\repo\\src\\main.rs:1".into()],
            file_hits: vec!["C:\\Users\\tester\\repo\\src\\main.rs:1".into()],
            tail: vec!["failed at C:\\Users\\tester\\repo\\src\\main.rs:1".into()],
            suggested_next_reads: vec!["C:\\Users\\tester\\repo\\src\\main.rs:1".into()],
            error_windows: vec![ErrorWindow {
                stream: "stderr".into(),
                line: 1,
                before: Vec::new(),
                matched: "error: C:\\Users\\tester\\repo\\src\\main.rs:1".into(),
                after: Vec::new(),
            }],
            digest_error_lines: vec!["error: C:\\Users\\tester\\repo\\src\\main.rs:1".into()],
            digest_file_hits: vec!["C:\\Users\\tester\\repo\\src\\main.rs:1".into()],
            test_or_package_hint: None,
            log_path: "C:\\Users\\tester\\kds\\run.log".into(),
            previous_exact_match_run: None,
            started_at: "2026-01-01T00:00:00Z".into(),
            command_kind: "cargo".into(),
            summary_budget: "normal".into(),
            capture_mode: "stdout/stderr piped to local temp files".into(),
            spawn_error: None,
            runtime_warnings: Vec::new(),
        }
    }

    #[test]
    fn extracts_rust_error_and_file_hit() {
        let summary = extract("error[E0425]: missing\n --> src/main.rs:10:5\n", "", 101);
        assert!(summary.error_count > 0);
        assert!(summary
            .file_hits
            .iter()
            .any(|hit| hit.contains("src/main.rs:10:5")));
    }

    #[test]
    fn extracts_pytest_node_ids_and_fail_lines() {
        let output = "FAILED tests/test_api.py::test_create_user - AssertionError: bad\nE   AssertionError: bad\n";
        let summary = extract(output, "", 1);
        assert!(summary.error_count >= 2);
        assert_eq!(
            summary.primary_failure.as_deref(),
            Some("FAILED tests/test_api.py::test_create_user - AssertionError: bad")
        );
        assert!(summary
            .file_hits
            .iter()
            .any(|hit| hit == "tests/test_api.py::test_create_user"));
    }

    #[test]
    fn extracts_typescript_paren_locations() {
        let output = "src/app/service.ts(12,7): error TS2322: Type 'string' is not assignable\n";
        let summary = extract(output, "", 2);
        assert!(summary.error_count > 0);
        assert!(summary
            .file_hits
            .iter()
            .any(|hit| hit == "src/app/service.ts:12:7"));
    }

    #[test]
    fn strips_ansi_from_summary_signals() {
        let summary = extract("", "\x1b[31;1mError: noisy failure\x1b[0m\n", 1);
        assert_eq!(summary.top_errors[0], "Error: noisy failure");
    }

    #[test]
    fn counts_npm_warnings_at_line_start() {
        let summary = extract("npm WARN deprecated package\n", "", 0);
        assert_eq!(summary.warning_count, 1);
    }

    #[test]
    fn does_not_treat_success_count_as_error() {
        let summary = extract("test result: ok. 10 passed; 0 failed\n", "", 0);
        assert_eq!(summary.error_count, 0);
    }

    #[test]
    fn redacts_secrets_from_summary_signals() {
        let output = "\
error: token=sk-testabcdefghijklmnopqrstuvwxyz
Authorization: Bearer abcdefghijklmnopqrstuvwxyz
fatal: https://user:password@example.com/repo.git failed
slack=xoxb-123456789012-123456789012-abcdefghijklmnopqrstuvwx
google=AIzaSyB123456789012345678901234567890123
jwt=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.sflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c
discord=aaaaaaaaaaaaaaaaaaaaaaaa.bbbbbb.cccccccccccccccccccccccccccccc
";
        let summary = extract("", output, 1);
        let rendered = format!(
            "{}\n{}",
            summary.top_errors.join("\n"),
            summary.tail.join("\n")
        );
        assert!(!rendered.contains("sk-testabcdefghijklmnopqrstuvwxyz"));
        assert!(!rendered.contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(!rendered.contains("user:password"));
        assert!(rendered.contains("token=[redacted]"));
        assert!(rendered.contains("Authorization: [redacted]"));
        assert!(rendered.contains("https://[redacted]@example.com/repo.git"));
        assert!(!rendered.contains("xoxb-123456789012"));
        assert!(!rendered.contains("AIzaSyB123456"));
        assert!(!rendered.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!rendered.contains("aaaaaaaaaaaaaaaaaaaaaaaa.bbbbbb"));
    }

    #[test]
    fn redacts_sensitive_argv_values() {
        let argv = vec![
            "deploy".to_string(),
            "--token".to_string(),
            "secret-value".to_string(),
            "--api-key=abc123".to_string(),
            "Authorization: Bearer abcdefghijklmnopqrstuvwxyz".to_string(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[redacted]");
        assert_eq!(redacted[3], "--api-key=[redacted]");
        assert_eq!(redacted[4], "Authorization: [redacted]");

        let line = redact_sensitive_text("deploy --token SECRET_CANARY_PATH_LEAK --api-key=abc123");
        assert!(!line.contains("SECRET_CANARY_PATH_LEAK"));
        assert!(!line.contains("abc123"));
        assert_eq!(line, "deploy --token [redacted] --api-key=[redacted]");
    }

    #[test]
    fn compact_output_hides_paths_until_show_paths_is_enabled() {
        let sidecar = sidecar_for_display();
        let hidden = format_compact_with_paths(&sidecar, false);
        assert!(!hidden.contains("CWD:"), "hidden:\n{hidden}");
        assert!(
            !hidden.contains("C:\\Users\\tester\\kds\\run.log"),
            "hidden:\n{hidden}"
        );
        assert!(hidden.contains("Log: use `kds logs show run-123 --show-paths`"));
        assert!(
            hidden.contains("<cwd>\\src\\main.rs:1"),
            "hidden:\n{hidden}"
        );

        let shown = format_compact_with_paths(&sidecar, true);
        assert!(
            shown.contains("CWD: C:\\Users\\tester\\repo"),
            "shown:\n{shown}"
        );
        assert!(
            shown.contains("Log: C:\\Users\\tester\\kds\\run.log"),
            "shown:\n{shown}"
        );
    }
}
