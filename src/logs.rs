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
        LogsCommand::Last => {
            let entry = storage::last_run(&paths)?;
            let sidecar = storage::read_sidecar(Path::new(&entry.summary_path))?;
            print!("{}", summarize::format_safe_metadata(&sidecar));
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
        print!("{}", summarize::format_compact(&sidecar));
    } else if args.errors {
        print_section("Top errors", &sidecar.top_errors, 8);
    } else if args.tail {
        print_section("Final tail", &sidecar.tail, 40);
    } else if args.file_hits {
        print_section("File hits", &sidecar.file_hits, 10);
    } else {
        print!("{}", summarize::format_safe_metadata(&sidecar));
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
