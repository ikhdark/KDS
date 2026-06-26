use anyhow::Result;
use std::path::Path;

use crate::cli::LogsArgs;
use crate::storage;
use crate::summarize;

pub fn run(args: LogsArgs) -> Result<i32> {
    let paths = storage::Paths::discover()?;
    match resolve_request(args)? {
        LogsRequest::Stats { show_paths } => {
            let stats = storage::log_stats(&paths)?;
            println!("KDS logs stats");
            println!("Runs indexed: {}", stats.indexed_runs);
            println!("Raw logs: {}", stats.raw_logs);
            println!("Summary sidecars: {}", stats.summary_sidecars);
            println!("Temp files: {}", stats.temp_files);
            println!(
                "Artifact bytes: {}",
                crate::gc::format_bytes(stats.artifact_bytes)
            );
            println!(
                "Oldest run: {}",
                stats.oldest_run.unwrap_or_else(|| "none".to_string())
            );
            println!(
                "Newest run: {}",
                stats.newest_run.unwrap_or_else(|| "none".to_string())
            );
            if show_paths {
                println!("Logs directory: {}", paths.logs_dir.display());
            } else {
                println!("Logs directory: use `kds logs --show-paths`");
            }
            Ok(0)
        }
        LogsRequest::Run {
            id,
            show_paths,
            section,
        } => show(&paths, &id, show_paths, section),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogsSection {
    Metadata,
    Summary,
    Errors,
    ErrorWindow,
    Tail,
    FileHits,
}

#[derive(Debug, PartialEq, Eq)]
enum LogsRequest {
    Stats {
        show_paths: bool,
    },
    Run {
        id: String,
        show_paths: bool,
        section: LogsSection,
    },
}

enum Positional {
    None,
    Run(String),
}

fn resolve_request(args: LogsArgs) -> Result<LogsRequest> {
    let section = section_from_flags(&args)?;
    let positional = positional_request(args.target)?;
    let section_requested = section != LogsSection::Metadata;

    if let Positional::Run(id) = positional {
        return Ok(LogsRequest::Run {
            id,
            show_paths: args.show_paths,
            section,
        });
    }

    if section_requested {
        return Ok(LogsRequest::Run {
            id: "last".to_string(),
            show_paths: args.show_paths,
            section,
        });
    }

    Ok(LogsRequest::Stats {
        show_paths: args.show_paths,
    })
}

fn section_from_flags(args: &LogsArgs) -> Result<LogsSection> {
    let sections = [
        args.summary,
        args.errors,
        args.error_window,
        args.tail,
        args.file_hits,
    ]
    .iter()
    .filter(|enabled| **enabled)
    .count();
    if sections > 1 {
        anyhow::bail!("choose only one logs section flag");
    }
    if args.summary {
        Ok(LogsSection::Summary)
    } else if args.errors {
        Ok(LogsSection::Errors)
    } else if args.error_window {
        Ok(LogsSection::ErrorWindow)
    } else if args.tail {
        Ok(LogsSection::Tail)
    } else if args.file_hits {
        Ok(LogsSection::FileHits)
    } else {
        Ok(LogsSection::Metadata)
    }
}

fn positional_request(target: Option<String>) -> Result<Positional> {
    match target.as_deref() {
        None => Ok(Positional::None),
        Some("dir") | Some("stats") => anyhow::bail!(
            "logs dir/stats aliases were removed; use `kds logs` or `kds logs --show-paths`"
        ),
        Some("show") => {
            anyhow::bail!("logs show alias was removed; use `kds logs last` or `kds logs <run-id>`")
        }
        Some(id) => Ok(Positional::Run(id.to_string())),
    }
}

fn show(paths: &storage::Paths, id: &str, show_paths: bool, section: LogsSection) -> Result<i32> {
    let entry = if id == "last" {
        storage::last_run(paths)?
    } else {
        storage::resolve_run_id(paths, id)?
    };
    let sidecar = storage::read_sidecar_for_display(Path::new(&entry.summary_path))?;
    if section == LogsSection::Summary {
        print!(
            "{}",
            summarize::format_compact_with_paths(&sidecar, show_paths)
        );
    } else if section == LogsSection::Errors {
        print_section(
            "Top errors",
            &summarize::display_items_for_paths(&sidecar, &sidecar.top_errors, show_paths),
            8,
        );
    } else if section == LogsSection::ErrorWindow {
        print_error_windows(&sidecar, show_paths);
    } else if section == LogsSection::Tail {
        print_section(
            "Final tail",
            &summarize::display_items_for_paths(&sidecar, &sidecar.tail, show_paths),
            40,
        );
    } else if section == LogsSection::FileHits {
        print_section(
            "File hits",
            &summarize::display_items_for_paths(&sidecar, &sidecar.file_hits, show_paths),
            10,
        );
    } else {
        print!(
            "{}",
            summarize::format_safe_metadata_with_paths(&sidecar, show_paths)
        );
    }
    Ok(0)
}

fn print_error_windows(sidecar: &storage::SummarySidecar, show_paths: bool) {
    println!("Error windows");
    if sidecar.error_windows.is_empty() {
        println!("  none");
        return;
    }
    for window in &sidecar.error_windows {
        println!("  {} line {}", window.stream, window.line);
        for line in summarize::display_items_for_paths(sidecar, &window.before, show_paths) {
            println!("    before: {line}");
        }
        for line in summarize::display_items_for_paths(
            sidecar,
            std::slice::from_ref(&window.matched),
            show_paths,
        ) {
            println!("    match: {line}");
        }
        for line in summarize::display_items_for_paths(sidecar, &window.after, show_paths) {
            println!("    after: {line}");
        }
    }
}

fn print_section(title: &str, items: &[String], cap: usize) {
    println!("{title}");
    if items.is_empty() {
        println!("  none");
        return;
    }
    for item in items.iter().take(cap) {
        println!("  {item}");
    }
}
