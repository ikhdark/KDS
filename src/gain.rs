use anyhow::Result;

use crate::storage::{self, CommandMetrics};

pub fn run() -> Result<i32> {
    let paths = storage::Paths::discover()?;
    let metrics = storage::load_metrics(&paths);
    let percent = if metrics.raw_line_count == 0 {
        0.0
    } else {
        (metrics.estimated_saved_lines as f64 / metrics.raw_line_count as f64) * 100.0
    };

    println!("KDS usage savings");
    println!("Commands cleaned up: {}", metrics.command_count);
    println!("Usage savings: {:.1}%", percent);
    println!(
        "Approx token reduction: {} saved of {} raw (~{} shown)",
        metrics.approx_saved_tokens, metrics.approx_raw_tokens, metrics.approx_shown_tokens
    );
    println!("Full logs saved locally: yes");
    println!("Raw command output: {} lines", metrics.raw_line_count);
    println!("Codex saw: {} summary lines", metrics.shown_line_count);
    println!("Raw command output: {} chars", metrics.raw_char_count);
    println!("Codex saw: {} summary chars", metrics.shown_char_count);
    println!(
        "Repeat failure savings: {} lines, ~{} tokens",
        metrics.repeated_failure_saved_lines,
        metrics.repeated_failure_saved_chars / 4
    );
    println!(
        "Last command: {}",
        metrics.last_command_time.as_deref().unwrap_or("none")
    );
    if metrics.per_command_kind.is_empty() {
        println!("Command kinds: none");
    } else {
        println!("Command kinds:");
        for (kind, count) in &metrics.per_command_kind {
            println!("  {kind}: {count}");
        }
    }
    print_top("Top noisy commands", &metrics.per_command, |metric| {
        metric.raw_lines
    });
    print_top("Top savings commands", &metrics.per_command, |metric| {
        metric.saved_lines
    });
    print_negative_savings(&metrics.per_command);
    print_kind_percentiles(&metrics.per_command_kind_stats);
    print_low_value_wraps(&metrics.per_command);
    Ok(0)
}

fn print_top(
    title: &str,
    metrics: &std::collections::BTreeMap<String, CommandMetrics>,
    score: impl Fn(&CommandMetrics) -> u64,
) {
    let mut rows: Vec<_> = metrics.iter().collect();
    rows.sort_by_key(|(_, metric)| std::cmp::Reverse(score(metric)));
    println!("{title}:");
    if rows.is_empty() {
        println!("  none");
        return;
    }
    for (command, metric) in rows.into_iter().take(5) {
        println!(
            "  {}: {} run(s), raw {} lines, saved {} lines",
            command, metric.count, metric.raw_lines, metric.saved_lines
        );
    }
}

fn print_negative_savings(metrics: &std::collections::BTreeMap<String, CommandMetrics>) {
    println!("Commands with negative savings:");
    let mut printed = 0;
    for (command, metric) in metrics {
        if metric.shown_lines > metric.raw_lines {
            println!(
                "  {}: raw {} lines, summary {} lines",
                command, metric.raw_lines, metric.shown_lines
            );
            printed += 1;
        }
    }
    if printed == 0 {
        println!("  none");
    }
}

fn print_kind_percentiles(metrics: &std::collections::BTreeMap<String, CommandMetrics>) {
    println!("p50/p95 raw lines by command kind:");
    if metrics.is_empty() {
        println!("  none");
        return;
    }
    for (kind, metric) in metrics {
        let mut samples = metric.raw_line_samples.clone();
        samples.sort_unstable();
        println!(
            "  {}: p50 {}, p95 {}",
            kind,
            percentile(&samples, 50),
            percentile(&samples, 95)
        );
    }
}

fn print_low_value_wraps(metrics: &std::collections::BTreeMap<String, CommandMetrics>) {
    println!("Low-value wraps:");
    let mut printed = 0;
    for (command, metric) in metrics {
        if metric.count >= 2 && metric.raw_lines <= metric.shown_lines.saturating_add(metric.count)
        {
            let avg_raw = metric.raw_lines as f64 / metric.count as f64;
            let avg_shown = metric.shown_lines as f64 / metric.count as f64;
            println!(
                "  {}: avg raw {:.1} lines, avg summary {:.1} lines",
                command, avg_raw, avg_shown
            );
            println!("    suggested: consider a local hook exclude for this repo");
            printed += 1;
        }
    }
    if printed == 0 {
        println!("  none");
    }
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let index = ((samples.len() - 1) * percentile) / 100;
    samples[index]
}
