use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::cli::{HookCommand, HookShell};
use crate::storage;

const START: &str = "# kds-hook-start";
const END: &str = "# kds-hook-end";

pub struct HookStatus {
    pub installed: bool,
    pub profile: PathBuf,
    pub label: String,
}

pub fn run(command: HookCommand) -> Result<i32> {
    match command {
        HookCommand::Status => {
            let status = powershell_hook_status();
            println!("PowerShell hook: {}", status.label);
            println!("Installed: {}", status.installed);
            println!("Profile: {}", status.profile.display());
            Ok(0)
        }
        HookCommand::Doctor => {
            let status = powershell_hook_status();
            println!("KDS hook doctor");
            println!("PowerShell hook: {}", status.label);
            println!("Installed: {}", status.installed);
            println!("Profile: {}", status.profile.display());
            Ok(0)
        }
        HookCommand::Install(args) => match args.shell {
            HookShell::Powershell => install_powershell_hook(),
        },
        HookCommand::Uninstall(args) => match args.shell {
            HookShell::Powershell => uninstall_powershell_hook(),
        },
    }
}

pub fn powershell_hook_status() -> HookStatus {
    let profile = powershell_profile_path();
    let installed = fs::read_to_string(&profile)
        .map(|text| text.contains(START) && text.contains(END))
        .unwrap_or(false);
    HookStatus {
        installed,
        label: if installed {
            "installed"
        } else {
            "not installed"
        }
        .to_string(),
        profile,
    }
}

pub fn install_powershell_hook() -> Result<i32> {
    let profile = powershell_profile_path();
    let current = fs::read_to_string(&profile).unwrap_or_default();
    let block = hook_block()?;
    let updated = upsert_block(&current, &block);
    if let Some(parent) = profile.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    storage::write_text_atomic(&profile, &updated)?;
    println!("Installed automatic KDS PowerShell hook:");
    println!("{}", profile.display());
    Ok(0)
}

pub fn uninstall_powershell_hook() -> Result<i32> {
    let profile = powershell_profile_path();
    let current = fs::read_to_string(&profile).unwrap_or_default();
    let updated = remove_block(&current);
    storage::write_text_atomic(&profile, &updated)?;
    println!("Removed KDS PowerShell hook:");
    println!("{}", profile.display());
    Ok(0)
}

fn powershell_profile_path() -> PathBuf {
    if let Ok(path) = std::env::var("KDS_POWERSHELL_PROFILE") {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Documents")
        .join("PowerShell")
        .join("Microsoft.PowerShell_profile.ps1")
}

fn hook_block() -> Result<String> {
    let exe = std::env::current_exe().context("resolve current kds executable")?;
    let exe = exe.display().to_string().replace('\'', "''");
    Ok(format!(
        r#"{START}
# Managed by KDS. Remove with: kds hook uninstall powershell
$script:KdsExe = '{exe}'
function _kds_native {{
  param([string]$Name)
  $candidates = @("$Name.exe", "$Name.cmd", "$Name.ps1", $Name)
  foreach ($candidate in $candidates) {{
    $cmd = Get-Command $candidate -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) {{ return $cmd.Source }}
  }}
  return $Name
}}
function _kds_call_native {{
  param([string]$Name, [object[]]$Rest)
  $native = _kds_native $Name
  & $native @Rest
}}
function _kds_wrap {{
  param([string]$Name, [object[]]$Rest)
  & $script:KdsExe -- $Name @Rest
}}
function cargo {{
  if ($args.Count -gt 0 -and @('check','test','build','clippy') -contains $args[0]) {{ _kds_wrap 'cargo' $args }} else {{ _kds_call_native 'cargo' $args }}
}}
function just {{ _kds_wrap 'just' $args }}
function npm {{
  if ($args.Count -gt 0 -and @('test','run') -contains $args[0]) {{ _kds_wrap 'npm' $args }} else {{ _kds_call_native 'npm' $args }}
}}
function pnpm {{
  if ($args.Count -gt 0 -and @('test','run') -contains $args[0]) {{ _kds_wrap 'pnpm' $args }} else {{ _kds_call_native 'pnpm' $args }}
}}
function pytest {{ _kds_wrap 'pytest' $args }}
function python {{
  if ($args.Count -ge 2 -and $args[0] -eq '-m' -and $args[1] -eq 'pytest') {{ _kds_wrap 'python' $args }} else {{ _kds_call_native 'python' $args }}
}}
function git {{
  if ($args.Count -gt 0 -and @('status','diff','log') -contains $args[0]) {{ _kds_wrap 'git' $args }} else {{ _kds_call_native 'git' $args }}
}}
{END}
"#
    ))
}

fn upsert_block(current: &str, block: &str) -> String {
    let without = remove_block(current);
    let mut out = without.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(block);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_block_is_managed_and_removable() {
        let block = "# kds-hook-start\nx\n# kds-hook-end\n";
        let original = "before\n";
        let with = upsert_block(original, block);
        assert!(with.contains("before"));
        assert!(with.contains("# kds-hook-start"));
        let removed = remove_block(&with);
        assert!(removed.contains("before"));
        assert!(!removed.contains("# kds-hook-start"));
    }
}
