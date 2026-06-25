use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::hook;
use crate::init_codex;
use crate::storage;

pub fn run() -> Result<i32> {
    let paths = storage::Paths::discover()?;
    println!("KDS doctor");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    match std::env::current_exe() {
        Ok(exe) => {
            println!("Current executable: {}", exe.display());
            println!(
                "Executable directory on PATH: {}",
                exe.parent()
                    .map(path_dir_on_path)
                    .map(yes_no)
                    .unwrap_or("unknown")
            );
        }
        Err(err) => println!("Current executable: unavailable ({err})"),
    }
    println!(
        "KDS_HOME: {}",
        std::env::var("KDS_HOME").unwrap_or_else(|_| "default local data directory".to_string())
    );
    print_path_status("Storage root", &paths.root);
    print_path_status("Logs directory", &paths.logs_dir);
    print_path_status("State directory", &paths.state_dir);
    print_path_status("Runs index", &paths.runs_index);
    print_path_status("Metrics", &paths.metrics);
    print_state_health(&paths);

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
    println!(
        "kds command on PATH: {}",
        if command_available("kds") {
            "available"
        } else {
            "missing"
        }
    );
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
        let kind = fs::metadata(path)
            .map(|metadata| {
                if metadata.is_dir() {
                    "directory"
                } else if metadata.is_file() {
                    "file"
                } else {
                    "other"
                }
            })
            .unwrap_or("unknown");
        println!("{label}: exists, {kind} ({})", path.display());
    } else {
        println!(
            "{label}: missing, will be created on first run ({})",
            path.display()
        );
    }
}

fn print_state_health(paths: &storage::Paths) {
    let diagnostics = storage::state_diagnostics(paths);
    println!(
        "Runs index health: {}",
        if diagnostics.runs_index_present {
            format!(
                "{} valid run(s), {} malformed line(s), {} unreadable line(s)",
                diagnostics.runs_index_entries,
                diagnostics.runs_index_malformed_lines,
                diagnostics.runs_index_read_errors
            )
        } else {
            "missing".to_string()
        }
    );
    println!(
        "Metrics state: {}",
        file_json_status(diagnostics.metrics_present, diagnostics.metrics_valid)
    );
    println!(
        "Digest state: {}",
        file_json_status(diagnostics.digest_present, diagnostics.digest_valid_json)
    );
}

fn file_json_status(present: bool, valid: bool) -> &'static str {
    match (present, valid) {
        (false, _) => "missing",
        (true, true) => "valid",
        (true, false) => "present but invalid",
    }
}

fn path_dir_on_path(dir: &Path) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|entry| same_path(&entry, dir))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => path_string(&left) == path_string(&right),
        _ => path_string(left) == path_string(right),
    }
}

fn path_string(path: &Path) -> String {
    if cfg!(windows) {
        path.display().to_string().to_ascii_lowercase()
    } else {
        path.display().to_string()
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
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
