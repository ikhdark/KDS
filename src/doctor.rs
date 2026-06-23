use anyhow::Result;
use std::path::Path;

use crate::hook;
use crate::init_codex;
use crate::storage;

pub fn run() -> Result<i32> {
    let paths = storage::Paths::discover()?;
    println!("KDS doctor");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    print_path_status("Storage root", &paths.root);
    print_path_status("Logs directory", &paths.logs_dir);
    print_path_status("State directory", &paths.state_dir);
    print_path_status("Runs index", &paths.runs_index);
    print_path_status("Metrics", &paths.metrics);

    let codex_home = init_codex::codex_home();
    let agents = codex_home.join("AGENTS.md");
    let kds_md = codex_home.join("KDS.md");
    println!("Codex home: {}", codex_home.display());
    println!(
        "Codex AGENTS reference: {}",
        if std::fs::read_to_string(&agents)
            .map(|text| text.contains("@KDS.md"))
            .unwrap_or(false)
        {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        "KDS.md: {}",
        if kds_md.exists() {
            "present"
        } else {
            "missing"
        }
    );
    println!("PowerShell hook: {}", hook::powershell_hook_status().label);
    println!("Common commands:");
    for command in ["cargo", "git", "node", "npm", "pnpm", "python", "pytest"] {
        println!(
            "  {command}: {}",
            if command_available(command) {
                "available"
            } else {
                "missing"
            }
        );
    }
    Ok(0)
}

fn print_path_status(label: &str, path: &Path) {
    if path.exists() {
        println!("{label}: exists ({})", path.display());
    } else {
        println!(
            "{label}: missing, will be created on first run ({})",
            path.display()
        );
    }
}

fn command_available(command: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.PS1".to_string())
            .split(';')
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .collect()
    } else {
        vec!["".to_string()]
    };
    std::env::split_paths(&path).any(|dir| {
        if cfg!(windows) {
            exts.iter()
                .any(|ext| dir.join(format!("{command}.{ext}")).exists())
        } else {
            dir.join(command).exists()
        }
    })
}
