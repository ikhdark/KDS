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
    let combined = strip_ansi(&raw_combined);
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
            lower.contains("warning:") || lower.starts_with("warn ") || lower.contains(" npm warn ")
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
        || lower.contains(" failed")
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
}
