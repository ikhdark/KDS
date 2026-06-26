use anyhow::{bail, Result};
use std::time::Duration;

use crate::cli::{GcArgs, PruneArgs};
use crate::storage;

pub fn run(args: GcArgs) -> Result<i32> {
    let older_than = parse_age(&args.older_than)?;
    let paths = storage::Paths::discover()?;
    let report = storage::gc_artifacts(&paths, older_than, args.dry_run)?;
    let state_report = if should_reconcile_state(&paths, &report, args.dry_run) {
        Some(storage::reconcile_state_after_artifact_cleanup(&paths)?)
    } else {
        None
    };

    println!("KDS gc");
    println!("Older than: {}", args.older_than);
    println!("Mode: {}", if args.dry_run { "dry run" } else { "delete" });
    println!("Artifacts matched: {}", report.matched_artifacts);
    println!("Bytes matched: {}", format_bytes(report.matched_bytes));
    if args.dry_run {
        println!("Deleted: 0");
    } else {
        println!("Deleted: {}", report.deleted_artifacts);
        println!("Bytes deleted: {}", format_bytes(report.deleted_bytes));
        print_state_reconciliation(state_report.as_ref());
    }
    Ok(0)
}

pub fn run_prune(args: PruneArgs) -> Result<i32> {
    let older_than = parse_age(&args.before)?;
    let paths = storage::Paths::discover()?;
    let report = storage::gc_artifacts(&paths, older_than, args.dry_run)?;
    let state_report = if should_reconcile_state(&paths, &report, args.dry_run) {
        Some(storage::reconcile_state_after_artifact_cleanup(&paths)?)
    } else {
        None
    };

    println!("KDS prune");
    println!("Before: {}", args.before);
    println!("Mode: {}", if args.dry_run { "dry run" } else { "delete" });
    println!("Artifacts matched: {}", report.matched_artifacts);
    println!("Bytes matched: {}", format_bytes(report.matched_bytes));
    if args.dry_run {
        println!("Deleted: 0");
    } else {
        println!("Deleted: {}", report.deleted_artifacts);
        println!("Bytes deleted: {}", format_bytes(report.deleted_bytes));
        print_state_reconciliation(state_report.as_ref());
    }
    Ok(0)
}

fn should_reconcile_state(
    paths: &storage::Paths,
    report: &storage::GcReport,
    dry_run: bool,
) -> bool {
    !dry_run
        && (report.deleted_artifacts > 0
            || paths.runs_index.exists()
            || paths.latest_by_command.exists()
            || paths.digest_dir.exists()
            || paths.state_dir.join("unresolved-by-command").exists())
}

fn print_state_reconciliation(report: Option<&storage::StateReconciliationReport>) {
    let Some(report) = report else {
        return;
    };
    println!(
        "State reconciled: {} stale index entry(s), {} latest entry(s), {} digest shard(s), {} unresolved digest ref(s)",
        report.index_entries_removed,
        report.latest_entries_rebuilt,
        report.digest_shards_removed,
        report.unresolved_digest_refs_removed
    );
}

fn parse_age(raw: &str) -> Result<Duration> {
    let raw = raw.trim();
    if raw.len() < 2 {
        bail!("age must look like 30d, 12h, or 90m");
    }
    let (number, unit) = raw.split_at(raw.len() - 1);
    let value: u64 = number
        .parse()
        .map_err(|_| anyhow::anyhow!("age must start with a positive number"))?;
    if value == 0 {
        bail!("age must be greater than zero");
    }
    let seconds = match unit {
        "d" | "D" => value.saturating_mul(24 * 60 * 60),
        "h" | "H" => value.saturating_mul(60 * 60),
        "m" | "M" => value.saturating_mul(60),
        "s" | "S" => value,
        _ => bail!("age unit must be d, h, m, or s"),
    };
    Ok(Duration::from_secs(seconds))
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.1} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gc_age_units() {
        assert_eq!(
            parse_age("30d").unwrap(),
            Duration::from_secs(30 * 24 * 60 * 60)
        );
        assert_eq!(parse_age("12h").unwrap(), Duration::from_secs(12 * 60 * 60));
        assert_eq!(parse_age("90m").unwrap(), Duration::from_secs(90 * 60));
        assert_eq!(parse_age("45s").unwrap(), Duration::from_secs(45));
        assert!(parse_age("0d").is_err());
        assert!(parse_age("30").is_err());
    }
}
