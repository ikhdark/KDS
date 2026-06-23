use regex::Regex;
use std::sync::OnceLock;

use crate::storage::SummarySidecar;

#[derive(Debug, Clone)]
pub struct ExtractedSummary {
    pub error_count: usize,
    pub warning_count: usize,
    pub primary_failure: Option<String>,
    pub top_errors: Vec<String>,
    pub file_hits: Vec<String>,
    pub tail: Vec<String>,
    pub suggested_next_reads: Vec<String>,
}

pub fn extract(stdout: &str, stderr: &str, exit_code: i32) -> ExtractedSummary {
    let raw_combined = if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    };
    let combined = redact_sensitive_text(&strip_ansi(&raw_combined));
    let lines: Vec<String> = combined
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect();
    let nonblank: Vec<String> = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .cloned()
        .collect();

    let warning_count = lines
        .iter()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("warning:")
                || lower.starts_with("warn ")
                || lower.starts_with("npm warn ")
                || lower.contains(" npm warn ")
        })
        .count();

    let error_count = lines.iter().filter(|line| is_error_line(line)).count();

    let mut top_errors = Vec::new();
    for line in nonblank.iter().filter(|line| is_error_line(line)) {
        push_unique_cap(&mut top_errors, line.clone(), 8);
    }
    if top_errors.is_empty() && exit_code != 0 {
        for line in nonblank.iter().rev().take(8).rev() {
            push_unique_cap(&mut top_errors, line.clone(), 8);
        }
    }

    let file_hits = extract_file_hits(&combined, 10);
    let suggested_next_reads = file_hits.iter().take(5).cloned().collect();
    let tail = nonblank
        .iter()
        .rev()
        .take(40)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let primary_failure = top_errors.first().cloned();

    ExtractedSummary {
        error_count,
        warning_count,
        primary_failure,
        top_errors,
        file_hits,
        tail,
        suggested_next_reads,
    }
}

fn strip_ansi(text: &str) -> String {
    static ANSI_RE: OnceLock<Regex> = OnceLock::new();
    let re = ANSI_RE.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap());
    re.replace_all(text, "").to_string()
}

pub fn redact_sensitive_text(text: &str) -> String {
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

pub fn format_compact(sidecar: &SummarySidecar) -> String {
    if sidecar.exit_code == 0 && sidecar.warning_count == 0 {
        return format!(
            "KDS\nRun ID: {}\nExit code: 0\nElapsed: {}\nLog: {}\nEstimated output reduction: {} lines ({:.1}%)\nSummary: success\nWarnings: 0\n",
            sidecar.run_id,
            sidecar.elapsed,
            sidecar.log_path,
            sidecar.estimated_saved_lines,
            sidecar.estimated_output_reduction_percent
        );
    }

    let mut out = String::new();
    out.push_str("KDS\n");
    out.push_str(&format!("Run ID: {}\n", sidecar.run_id));
    out.push_str(&format!("Command: {}\n", sidecar.command));
    out.push_str(&format!("CWD: {}\n", sidecar.cwd));
    out.push_str(&format!("Exit code: {}\n", sidecar.exit_code));
    out.push_str(&format!("Elapsed: {}\n", sidecar.elapsed));
    out.push_str(&format!("Log: {}\n", sidecar.log_path));
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
    out.push_str("Top errors:\n");
    write_list(&mut out, &sidecar.top_errors, 3);
    out.push_str("File hits:\n");
    write_list(&mut out, &sidecar.file_hits, 10);
    out.push_str(&format!("Warnings: {}\n", sidecar.warning_count));
    out.push_str("Final tail:\n");
    write_list(&mut out, &sidecar.tail, 40);
    out.push_str("Suggested next read:\n");
    write_list(&mut out, &sidecar.suggested_next_reads, 5);
    out
}

pub fn format_safe_metadata(sidecar: &SummarySidecar) -> String {
    format!(
        "KDS run\nRun ID: {}\nCommand: {}\nExit code: {}\nElapsed: {}\nLog: {}\nDigest: {}\nRepeat: {}\nAvailable:\n  --summary\n  --errors\n  --tail\n  --file-hits\nWarning: raw logs may contain secrets, paths, tokens, stack traces, environment values, or file contents.\n",
        sidecar.run_id,
        sidecar.command,
        sidecar.exit_code,
        sidecar.elapsed,
        sidecar.log_path,
        sidecar.digest,
        sidecar.repeat_status.message
    )
}

pub fn format_evidence(sidecar: &SummarySidecar) -> String {
    let mut out = String::new();
    out.push_str("KDS evidence\n");
    out.push_str(&format!("Run ID: {}\n", sidecar.run_id));
    out.push_str(&format!("Command: {}\n", sidecar.command));
    out.push_str(&format!("Exit code: {}\n", sidecar.exit_code));
    out.push_str(&format!("Digest: {}\n", sidecar.digest));
    out.push_str(&format!("Repeat: {}\n", sidecar.repeat_status.message));
    if let Some(delta) = &sidecar.delta {
        out.push_str(&format!("Changed since previous run: {delta}\n"));
    }
    out.push_str("Top errors:\n");
    write_list(&mut out, &sidecar.top_errors, 3);
    out.push_str("File hits:\n");
    write_list(&mut out, &sidecar.file_hits, 5);
    out.push_str("Suggested next reads:\n");
    write_list(&mut out, &sidecar.suggested_next_reads, 5);
    out.push_str(&format!("Log: {}\n", sidecar.log_path));
    out.push_str(&format!(
        "Estimated output reduction: {} lines ({:.1}%)\n",
        sidecar.estimated_saved_lines, sidecar.estimated_output_reduction_percent
    ));
    out
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
        || lower.starts_with("error ")
        || lower.contains("err!")
        || lower.contains("error: ")
        || lower.contains("panicked at")
        || lower.contains("could not compile")
        || lower.contains("failed to")
        || lower.starts_with("failed ")
        || lower.starts_with("traceback ")
        || lower.starts_with("e   ")
        || lower.contains("exception")
        || lower.contains("fatal:")
}

fn extract_file_hits(text: &str, cap: usize) -> Vec<String> {
    let re = Regex::new(
        r"(?m)([A-Za-z]:\\[^:\r\n]+|(?:\.{0,2}[\\/])?[\w .\-/\\]+\.[A-Za-z0-9_]+):(\d+)(?::(\d+))?",
    )
    .unwrap();
    let mut hits = Vec::new();
    for caps in re.captures_iter(text) {
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
    hits
}

fn push_unique_cap(items: &mut Vec<String>, item: String, cap: usize) {
    if items.len() < cap && !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
