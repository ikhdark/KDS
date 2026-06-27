use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::hook;
use crate::init_codex;
use crate::storage;

pub fn run() -> Result<i32> {
    let paths = storage::Paths::discover()?;
    println!("KDS doctor");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!("Releases: https://github.com/ikhdark/KDS/releases");
    println!("Update check: run `kds update check` (explicit network opt-in)");
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
    print_codex_desktop_hook_health();
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
        if diagnostics.digest_shards > 0 {
            format!("{} shard(s)", diagnostics.digest_shards)
        } else {
            file_json_status(diagnostics.digest_present, diagnostics.digest_valid_json).to_string()
        }
    );
    println!(
        "Latest-by-command state: {}",
        file_json_status(
            diagnostics.latest_by_command_present,
            diagnostics.latest_by_command_valid
        )
    );
}

fn print_codex_desktop_hook_health() {
    let health = codex_desktop_hook_health();
    println!(
        "Codex Desktop hook: {}",
        health_status(
            health.hook_installed,
            health.homes,
            "installed",
            "not installed"
        )
    );
    println!(
        "Codex Desktop hook trust: {}",
        health_status(health.trust_current, health.homes, "current", "missing")
    );
    println!(
        "Codex Desktop hook script: {}",
        health_status(health.hook_script_valid, health.homes, "valid", "missing")
    );
    println!(
        "Codex hooks.json: {}",
        hooks_json_status(
            health.hooks_json_present,
            health.hooks_json_parseable,
            health.homes
        )
    );
}

fn health_status(ok: usize, total: usize, ok_label: &str, empty_label: &str) -> String {
    match (ok, total) {
        (_, 0) => "not found".to_string(),
        (ok, total) if ok == total => ok_label.to_string(),
        (0, _) => empty_label.to_string(),
        (ok, total) => format!("partial ({ok}/{total})"),
    }
}

fn hooks_json_status(present: usize, parseable: usize, total: usize) -> String {
    if total == 0 {
        return "not found".to_string();
    }
    if present == 0 {
        return "missing".to_string();
    }
    if parseable == present && present == total {
        return "parseable".to_string();
    }
    if parseable == 0 {
        return "present but invalid".to_string();
    }
    format!("partial ({parseable}/{present} parseable)")
}

#[derive(Debug, Default)]
struct DesktopHookHealth {
    homes: usize,
    hooks_json_present: usize,
    hooks_json_parseable: usize,
    hook_installed: usize,
    hook_script_valid: usize,
    trust_current: usize,
}

#[derive(Debug, Default)]
struct DesktopHomeStatus {
    hooks_json_present: bool,
    hooks_json_parseable: bool,
    hook_installed: bool,
    hook_script_valid: bool,
    trust_current: bool,
}

#[derive(Debug, Clone)]
struct DesktopHookTrustEntry {
    key: String,
    trusted_hash: String,
}

fn codex_desktop_hook_health() -> DesktopHookHealth {
    let homes = codex_desktop_home_candidates();
    let mut health = DesktopHookHealth {
        homes: homes.len(),
        ..DesktopHookHealth::default()
    };
    for home in homes {
        let status = inspect_desktop_home(&home);
        health.hooks_json_present += usize::from(status.hooks_json_present);
        health.hooks_json_parseable += usize::from(status.hooks_json_parseable);
        health.hook_installed += usize::from(status.hook_installed);
        health.hook_script_valid += usize::from(status.hook_script_valid);
        health.trust_current += usize::from(status.trust_current);
    }
    health
}

fn inspect_desktop_home(home: &Path) -> DesktopHomeStatus {
    let hooks_json = home.join("hooks.json");
    let hook_script = home.join("hooks").join("kds-pre-tool-use.ps1");
    let config_toml = home.join("config.toml");
    let hook_script_valid = fs::read_to_string(&hook_script)
        .map(|text| {
            text.contains("PreToolUse")
                && text.contains("tool_name")
                && text.contains("updatedInput")
                && text.contains("CodexKD\\bin\\kds.exe")
        })
        .unwrap_or(false);

    let hooks_json_present = hooks_json.is_file();
    let mut status = DesktopHomeStatus {
        hooks_json_present,
        hook_script_valid,
        ..DesktopHomeStatus::default()
    };
    if !hooks_json_present {
        return status;
    }

    let parsed = fs::read_to_string(&hooks_json)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(config) = parsed else {
        return status;
    };
    status.hooks_json_parseable = true;
    let trust_entries = desktop_hook_trust_entries(&hooks_json, &config);
    status.hook_installed = !trust_entries.is_empty();
    status.trust_current = status.hook_installed
        && fs::read_to_string(&config_toml)
            .map(|config| {
                trust_entries
                    .iter()
                    .all(|entry| config_has_trust(&config, &entry.key, &entry.trusted_hash))
            })
            .unwrap_or(false);
    status
}

fn desktop_hook_trust_entries(hooks_json: &Path, config: &Value) -> Vec<DesktopHookTrustEntry> {
    let Some(pre_tool_use) = config
        .get("hooks")
        .and_then(|hooks| hooks.get("PreToolUse"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for (group_index, group) in pre_tool_use.iter().enumerate() {
        let matcher = group
            .get("matcher")
            .and_then(Value::as_str)
            .filter(|matcher| !matcher.trim().is_empty())
            .unwrap_or(".*");
        let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
            continue;
        };
        for (handler_index, handler) in handlers.iter().enumerate() {
            let command = handler.get("command").and_then(Value::as_str).unwrap_or("");
            let command_windows = handler
                .get("commandWindows")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !command.contains("kds-pre-tool-use.ps1")
                && !command_windows.contains("kds-pre-tool-use.ps1")
            {
                continue;
            }
            let effective_command = if command_windows.trim().is_empty() {
                command
            } else {
                command_windows
            };
            let timeout = handler
                .get("timeout")
                .and_then(Value::as_i64)
                .unwrap_or(600);
            let status_message = handler
                .get("statusMessage")
                .and_then(Value::as_str)
                .unwrap_or("");
            entries.push(DesktopHookTrustEntry {
                key: format!(
                    "{}:pre_tool_use:{group_index}:{handler_index}",
                    hooks_json.display()
                ),
                trusted_hash: command_hook_hash(
                    matcher,
                    effective_command,
                    timeout,
                    status_message,
                ),
            });
        }
    }
    entries
}

#[derive(Serialize)]
struct CommandHookIdentity<'a> {
    event_name: &'static str,
    hooks: [CommandHookRecord<'a>; 1],
    matcher: &'a str,
}

#[derive(Serialize)]
struct CommandHookRecord<'a> {
    #[serde(rename = "async")]
    async_field: bool,
    command: &'a str,
    #[serde(rename = "statusMessage")]
    status_message: &'a str,
    timeout: i64,
    #[serde(rename = "type")]
    hook_type: &'static str,
}

fn command_hook_hash(matcher: &str, command: &str, timeout: i64, status_message: &str) -> String {
    let identity = CommandHookIdentity {
        event_name: "pre_tool_use",
        hooks: [CommandHookRecord {
            async_field: false,
            command,
            status_message,
            timeout,
            hook_type: "command",
        }],
        matcher,
    };
    let json = serde_json::to_string(&identity).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    format!("sha256:{}", crate::hash::sha256_finalize_hex(hasher))
}

fn config_has_trust(config: &str, key: &str, trusted_hash: &str) -> bool {
    let header = format!("[hooks.state.{}]", toml_quoted_key(key));
    let trusted_line = format!("trusted_hash = \"{trusted_hash}\"");
    let mut in_section = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == header;
            continue;
        }
        if in_section && trimmed == trusted_line {
            return true;
        }
    }
    false
}

fn toml_quoted_key(value: &str) -> String {
    if !value.contains('\'') {
        return format!("'{value}'");
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn codex_desktop_home_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env_path("KDS_INSTALL_CODEX_HOME") {
        candidates.push(path);
        return existing_unique_paths(candidates);
    }
    if let Some(path) = env_path("CODEX_HOME") {
        candidates.push(path);
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".codex"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.trim().is_empty() {
            candidates.push(PathBuf::from(appdata).join("Codex"));
        }
    }
    if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
        if !local_appdata.trim().is_empty() {
            candidates.push(PathBuf::from(local_appdata).join("Codex"));
        }
    }
    if let Some(desktop) = dirs::desktop_dir() {
        candidates.push(desktop.join("LOCAL-KD"));
    }
    existing_unique_paths(candidates)
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn existing_unique_paths(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if !candidate.is_dir() {
            continue;
        }
        let full = installer_identity_path(candidate);
        if homes.iter().any(|existing| same_path(existing, &full)) {
            continue;
        }
        homes.push(full);
    }
    homes
}

fn installer_identity_path(path: PathBuf) -> PathBuf {
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    };
    lexical_normalize_path(absolute)
}

fn lexical_normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_home_status_detects_installed_trusted_hook() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();
        let hook_script = hooks_dir.join("kds-pre-tool-use.ps1");
        fs::write(
            &hook_script,
            "$event.hook_event_name -ne 'PreToolUse'\n$event.tool_name\n$kdsExe = Join-Path $env:LOCALAPPDATA 'CodexKD\\bin\\kds.exe'\nupdatedInput\n",
        )
        .unwrap();

        let hook_command = format!(
            "pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
            hook_script.display()
        );
        let hooks_json = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "^Bash$",
                        "hooks": [
                            {
                                "type": "command",
                                "command": hook_command,
                                "commandWindows": hook_command,
                                "timeout": 5,
                                "statusMessage": "Routing allowlisted commands through KDS"
                            }
                        ]
                    }
                ]
            }
        });
        let hooks_json_path = dir.path().join("hooks.json");
        fs::write(
            &hooks_json_path,
            serde_json::to_string_pretty(&hooks_json).unwrap(),
        )
        .unwrap();
        let entries = desktop_hook_trust_entries(&hooks_json_path, &hooks_json);
        assert_eq!(entries.len(), 1);
        fs::write(
            dir.path().join("config.toml"),
            format!(
                "[hooks.state.{}]\ntrusted_hash = \"{}\"\n",
                toml_quoted_key(&entries[0].key),
                entries[0].trusted_hash
            ),
        )
        .unwrap();

        let status = inspect_desktop_home(dir.path());
        assert!(status.hooks_json_present);
        assert!(status.hooks_json_parseable);
        assert!(status.hook_installed);
        assert!(status.hook_script_valid);
        assert!(status.trust_current);
    }

    #[test]
    fn config_trust_requires_matching_hash() {
        let key = "C:\\Users\\tester\\LOCAL-KD\\hooks.json:pre_tool_use:0:0";
        let hash = "sha256:abc123";
        let config = format!(
            "[hooks.state.{}]\ntrusted_hash = \"{}\"\n",
            toml_quoted_key(key),
            hash
        );
        assert!(config_has_trust(&config, key, hash));
        assert!(!config_has_trust(&config, key, "sha256:wrong"));
    }

    #[test]
    fn installer_identity_path_normalizes_without_canonicalizing() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir
            .path()
            .join(".")
            .join("missing")
            .join("..")
            .join("hooks.json");

        assert_eq!(
            installer_identity_path(input),
            dir.path().join("hooks.json")
        );
    }
}
