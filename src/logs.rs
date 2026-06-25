use anyhow::Result;
use std::path::Path;

use crate::cli::{LogsCommand, LogsShowArgs};
use crate::storage;
use crate::summarize;

pub fn run(command: LogsCommand) -> Result<i32> {
    let paths = storage::Paths::discover()?;
    match command {
        LogsCommand::Dir => {
            println!("{}", paths.logs_dir.display());
            Ok(0)
        }
        LogsCommand::Stats => {
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
            println!("Logs directory: use `kds logs dir`");
            Ok(0)
        }
        LogsCommand::Last(args) => {
            let entry = storage::last_run(&paths)?;
            let sidecar = storage::read_sidecar_for_display(Path::new(&entry.summary_path))?;
            print!(
                "{}",
                summarize::format_safe_metadata_with_paths(&sidecar, args.show_paths)
            );
            Ok(0)
        }
        LogsCommand::Show(args) => show(args),
    }
}

fn show(args: LogsShowArgs) -> Result<i32> {
    let paths = storage::Paths::discover()?;
    let entry = if args.id == "last" {
        storage::last_run(&paths)?
    } else {
        storage::resolve_run_id(&paths, &args.id)?
    };
    let sidecar = storage::read_sidecar_for_display(Path::new(&entry.summary_path))?;
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
        anyhow::bail!("choose only one logs show section flag");
    }
    if args.summary {
        print!(
            "{}",
            summarize::format_compact_with_paths(&sidecar, args.show_paths)
        );
    } else if args.errors {
        print_section(
            "Top errors",
            &summarize::display_items_for_paths(&sidecar, &sidecar.top_errors, args.show_paths),
            8,
        );
    } else if args.error_window {
        print_error_windows(&sidecar, args.show_paths);
    } else if args.tail {
        print_section(
            "Final tail",
            &summarize::display_items_for_paths(&sidecar, &sidecar.tail, args.show_paths),
            40,
        );
    } else if args.file_hits {
        print_section(
            "File hits",
            &summarize::display_items_for_paths(&sidecar, &sidecar.file_hits, args.show_paths),
            10,
        );
    } else {
        print!(
            "{}",
            summarize::format_safe_metadata_with_paths(&sidecar, args.show_paths)
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
