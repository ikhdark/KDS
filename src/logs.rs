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
        LogsCommand::Last(args) => {
            let entry = storage::last_run(&paths)?;
            let sidecar = storage::read_sidecar(Path::new(&entry.summary_path))?;
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
    let entry = storage::resolve_run_id(&paths, &args.id)?;
    let sidecar = storage::read_sidecar(Path::new(&entry.summary_path))?;
    let sections = [args.summary, args.errors, args.tail, args.file_hits]
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
