use anyhow::Result;

use crate::storage;

pub fn run() -> Result<i32> {
    let paths = storage::Paths::discover()?;
    let metrics = storage::load_metrics(&paths);
    let percent = if metrics.raw_line_count == 0 {
        0.0
    } else {
        (metrics.estimated_saved_lines as f64 / metrics.raw_line_count as f64) * 100.0
    };

    println!("KDS gain");
    println!("Commands: {}", metrics.command_count);
    println!("Raw lines: {}", metrics.raw_line_count);
    println!("Shown lines: {}", metrics.shown_line_count);
    println!("Estimated saved lines: {}", metrics.estimated_saved_lines);
    println!("Estimated output reduction: {:.1}%", percent);
    println!("Failures: {}", metrics.failure_count);
    println!(
        "Repeated failure signals: {}",
        metrics.repeated_failure_count
    );
    println!(
        "Last command: {}",
        metrics.last_command_time.as_deref().unwrap_or("none")
    );
    if metrics.per_command_kind.is_empty() {
        println!("Command kinds: none");
    } else {
        println!("Command kinds:");
        for (kind, count) in metrics.per_command_kind {
            println!("  {kind}: {count}");
        }
    }
    Ok(0)
}
