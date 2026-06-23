use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::cli::InitArgs;
use crate::storage;

const START: &str = "<!-- kds-instructions -->";
const END: &str = "<!-- /kds-instructions -->";
const BLOCK: &str = "<!-- kds-instructions -->\n@KDS.md\n<!-- /kds-instructions -->\n";

pub fn run(args: InitArgs) -> Result<i32> {
    if !args.global || !args.codex {
        anyhow::bail!("init currently supports `kds init -g --codex`");
    }
    let home = codex_home();
    let agents = home.join("AGENTS.md");
    let kds_md = home.join("KDS.md");

    if args.uninstall {
        return uninstall(&agents, args.dry_run);
    }

    let kds_text = kds_guidance();
    let current_agents = fs::read_to_string(&agents).unwrap_or_default();
    let new_agents = upsert_block(&current_agents);

    if args.dry_run || !args.yes {
        println!("KDS Codex init dry run");
        println!("Would write: {}", kds_md.display());
        println!("Would update: {}", agents.display());
        println!("Managed block:\n{BLOCK}");
        if !args.dry_run && !args.yes {
            println!("No files written. Re-run with --yes to apply.");
        }
        return Ok(0);
    }

    fs::create_dir_all(&home).with_context(|| format!("create {}", home.display()))?;
    if let Some(backup) = backup_existing(&agents)? {
        println!("Backed up: {}", backup.display());
    }
    storage::write_text_atomic(&kds_md, &kds_text)?;
    storage::write_text_atomic(&agents, &new_agents)?;
    println!("Wrote: {}", kds_md.display());
    println!("Updated: {}", agents.display());
    Ok(0)
}

pub fn codex_home() -> PathBuf {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

fn uninstall(agents: &PathBuf, dry_run: bool) -> Result<i32> {
    let current = fs::read_to_string(agents).unwrap_or_default();
    let updated = remove_block(&current);
    if dry_run {
        println!("KDS Codex uninstall dry run");
        println!("Would update: {}", agents.display());
        return Ok(0);
    }
    if let Some(backup) = backup_existing(agents)? {
        println!("Backed up: {}", backup.display());
    }
    storage::write_text_atomic(agents, &updated)?;
    println!("Removed KDS block from: {}", agents.display());
    Ok(0)
}

fn upsert_block(current: &str) -> String {
    let without = remove_block(current);
    let mut out = without.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(BLOCK);
    out
}

fn remove_block(current: &str) -> String {
    if let (Some(start), Some(end_start)) = (current.find(START), current.find(END)) {
        let end = end_start + END.len();
        let mut out = String::new();
        out.push_str(current[..start].trim_end());
        out.push('\n');
        out.push_str(current[end..].trim_start());
        return out.trim_matches('\n').to_string() + "\n";
    }
    current.to_string()
}

fn backup_existing(path: &PathBuf) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = chrono::Local::now()
        .timestamp_nanos_opt()
        .map(|value| value.to_string())
        .unwrap_or_else(|| chrono::Local::now().timestamp_micros().to_string());
    for attempt in 0..100 {
        let suffix = if attempt == 0 {
            format!("md.{stamp}.bak")
        } else {
            format!("md.{stamp}.{attempt}.bak")
        };
        let backup = path.with_extension(suffix);
        if !backup.exists() {
            fs::copy(path, &backup)
                .with_context(|| format!("backup {} to {}", path.display(), backup.display()))?;
            return Ok(Some(backup));
        }
    }
    anyhow::bail!(
        "could not allocate unique backup path for {}",
        path.display()
    )
}

fn kds_guidance() -> String {
    r#"# KDS Usage

Use KDS for noisy non-interactive commands. Prefer explicit forms when hooks are
unavailable:

- `kds -- cargo test`
- `kds -- just ...`
- `kds -- npm test`
- `kds -- pnpm test`
- `kds -- pytest`

Do not use KDS for precise `rg`, `git grep`, small commands, interactive
commands, password prompts, SSH sessions, long-running daemons, or commands
likely to print secrets.

KDS preserves full raw logs at the printed log path. KDS summaries are compact
evidence, not proof of correctness beyond the wrapped command exit code.
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_block_is_idempotent_and_preserves_content() {
        let original = "before\n";
        let once = upsert_block(original);
        let twice = upsert_block(&once);
        assert_eq!(once, twice);
        assert!(twice.contains("before"));
        let removed = remove_block(&twice);
        assert!(removed.contains("before"));
        assert!(!removed.contains("@KDS.md"));
    }
}
