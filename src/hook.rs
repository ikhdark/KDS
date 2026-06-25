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
    if updated == current {
        println!("KDS PowerShell hook is already installed:");
        println!("{}", profile.display());
        return Ok(0);
    }
    if let Some(parent) = profile.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if profile.exists() {
        let backup = profile_backup_path(&profile);
        storage::write_text_atomic(&backup, &current)?;
        println!("Backed up PowerShell profile:");
        println!("{}", backup.display());
    }
    storage::write_text_atomic(&profile, &updated)?;
    println!("Installed automatic KDS PowerShell hook:");
    println!("{}", profile.display());
    Ok(0)
}

pub fn uninstall_powershell_hook() -> Result<i32> {
    let profile = powershell_profile_path();
    let current = match fs::read_to_string(&profile) {
        Ok(current) => current,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("No KDS PowerShell hook found:");
            println!("{}", profile.display());
            return Ok(0);
        }
        Err(err) => {
            return Err(err).with_context(|| format!("read {}", profile.display()));
        }
    };
    if !has_block(&current) {
        println!("No KDS PowerShell hook found:");
        println!("{}", profile.display());
        return Ok(0);
    }
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

fn profile_backup_path(profile: &std::path::Path) -> PathBuf {
    let stamp = storage::iso_now().replace([':', '+'], "").replace('.', "-");
    let file_name = profile
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Microsoft.PowerShell_profile.ps1");
    profile.with_file_name(format!("{file_name}.kds-backup-{stamp}"))
}

fn hook_block() -> Result<String> {
    let exe = std::env::current_exe().context("resolve current kds executable")?;
    let exe = exe.display().to_string().replace('\'', "''");
    Ok(format!(
        r#"{START}
# Managed by KDS. Remove with: kds hook uninstall powershell
$script:KdsExe = '{exe}'
$script:KdsCommand = [System.IO.Path]::GetFileName($script:KdsExe)
$script:KdsExeDir = Split-Path -Parent $script:KdsExe
if ($script:KdsExeDir -and -not (($env:PATH -split ';') -contains $script:KdsExeDir)) {{
  $env:PATH = "$script:KdsExeDir;$env:PATH"
}}
function KDS {{
  $kdsArgs = @($args)
  $kdsCommands = @('run','raw','gain','gc','prune','doctor','logs','evidence','init','hook','help')
  if ($kdsArgs.Count -gt 0 -and -not ([string]$kdsArgs[0]).StartsWith('-') -and -not ($kdsCommands -contains [string]$kdsArgs[0])) {{
    $kdsArgs = @('--') + $kdsArgs
  }}
  & $script:KdsCommand @kdsArgs
}}
if (-not $global:KdsPromptWrapped) {{
  $global:KdsPromptWrapped = $true
  $promptCommand = Get-Command prompt -CommandType Function -ErrorAction SilentlyContinue
  $global:KdsPromptOriginal = if ($promptCommand) {{ $promptCommand.ScriptBlock }} else {{ $null }}
  function global:prompt {{
    $basePrompt = if ($global:KdsPromptOriginal) {{
      & $global:KdsPromptOriginal
    }} else {{
      "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) "
    }}
    $text = [string]$basePrompt
    if ($text -match '^\s*KDS(\s|>|:|\]|$)') {{ return $text }}
    return "KDS $text"
  }}
}}
function _kds_native {{
  param([string]$Name)
  $cmd = Get-Command $Name -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($cmd) {{ return $cmd.Source }}
  return $null
}}
function _kds_restore_args {{
  param([object[]]$Rest, [string]$Statement)
  $out = [System.Collections.Generic.List[object]]::new()
  foreach ($arg in $Rest) {{ [void]$out.Add($arg) }}
  $errors = $null
  $tokens = [System.Management.Automation.PSParser]::Tokenize($Statement, [ref]$errors)
  $seenCommand = $false
  $argIndex = 0
  foreach ($token in $tokens) {{
    if (-not $seenCommand) {{
      if ($token.Type -eq 'Command') {{ $seenCommand = $true }}
      continue
    }}
    if ($token.Type -eq 'CommandArgument' -or $token.Type -eq 'String') {{
      $argIndex += 1
    }} elseif ($token.Type -eq 'Operator' -and $token.Content -eq '--') {{
      if ($argIndex -le $out.Count) {{
        $out.Insert($argIndex, '--')
        $argIndex += 1
      }}
    }}
  }}
  return $out.ToArray()
}}
function _kds_call_native {{
  param([string]$Name, [object[]]$Rest)
  $native = _kds_native $Name
  if (-not $native) {{
    [Console]::Error.WriteLine("kds hook: command not found: $Name")
    $global:LASTEXITCODE = 127
    return
  }}
  & $native @Rest
}}
function _kds_wrap {{
  param([string]$Name, [object[]]$Rest)
  $kdsArgs = @('--', $Name) + $Rest
  KDS @kdsArgs
}}
function _kds_safe_task {{
  param([string]$Name)
  return @('test','build','check','lint','typecheck','ci','clippy') -contains $Name
}}
function cargo {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  if ($rest.Count -gt 0 -and @('check','test','build','clippy') -contains $rest[0]) {{ _kds_wrap 'cargo' $rest }} else {{ _kds_call_native 'cargo' $rest }}
}}
function just {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  if ($rest.Count -gt 0 -and (_kds_safe_task $rest[0])) {{ _kds_wrap 'just' $rest }} else {{ _kds_call_native 'just' $rest }}
}}
function npm {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  if ($rest.Count -gt 0 -and $rest[0] -eq 'test') {{ _kds_wrap 'npm' $rest }} elseif ($rest.Count -ge 2 -and $rest[0] -eq 'run' -and (_kds_safe_task $rest[1])) {{ _kds_wrap 'npm' $rest }} else {{ _kds_call_native 'npm' $rest }}
}}
function pnpm {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  if ($rest.Count -gt 0 -and $rest[0] -eq 'test') {{ _kds_wrap 'pnpm' $rest }} elseif ($rest.Count -ge 2 -and $rest[0] -eq 'run' -and (_kds_safe_task $rest[1])) {{ _kds_wrap 'pnpm' $rest }} else {{ _kds_call_native 'pnpm' $rest }}
}}
function pytest {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_wrap 'pytest' $rest
}}
function python {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  if ($rest.Count -ge 2 -and $rest[0] -eq '-m' -and (@('pytest','unittest') -contains $rest[1])) {{ _kds_wrap 'python' $rest }} else {{ _kds_call_native 'python' $rest }}
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
    if let Some((start, end)) = block_bounds(current) {
        let mut out = String::new();
        out.push_str(current[..start].trim_end());
        out.push('\n');
        out.push_str(current[end..].trim_start());
        return out.trim_matches('\n').to_string() + "\n";
    }
    current.to_string()
}

fn has_block(current: &str) -> bool {
    block_bounds(current).is_some()
}

fn block_bounds(current: &str) -> Option<(usize, usize)> {
    let start = current.find(START)?;
    let end_start = current[start..].find(END)? + start;
    Some((start, end_start + END.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::Command;

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

    #[test]
    fn hook_block_wraps_with_friendly_kds_entrypoint() {
        let block = hook_block().unwrap();
        assert!(block.contains("function KDS {"), "block:\n{block}");
        assert!(
            block.contains("$kdsCommands = @('run','raw','gain','gc','prune','doctor','logs','evidence','init','hook','help')"),
            "block:\n{block}"
        );
        assert!(
            block.contains("$kdsArgs = @('--', $Name) + $Rest"),
            "block:\n{block}"
        );
        assert!(
            block.contains("& $script:KdsCommand @kdsArgs"),
            "block:\n{block}"
        );
        assert!(block.contains("KDS @kdsArgs"), "block:\n{block}");
        assert!(
            !block.contains("& $script:KdsExe -- $Name @Rest"),
            "block:\n{block}"
        );
        assert!(
            !block.contains("& $script:KdsExe @kdsArgs"),
            "block:\n{block}"
        );
    }

    #[test]
    fn remove_block_noops_when_managed_block_is_absent() {
        let original = "before\n";
        assert!(!has_block(original));
        assert_eq!(remove_block(original), original);
    }

    #[test]
    fn remove_block_ignores_reversed_markers() {
        let original = "# kds-hook-end\nbefore\n# kds-hook-start\n";
        assert!(!has_block(original));
        assert_eq!(remove_block(original), original);
    }

    #[test]
    fn upsert_block_repairs_existing_managed_block_without_duplication() {
        let current = "before\n# kds-hook-start\nold\n# kds-hook-end\nafter\n";
        let repaired = upsert_block(current, "# kds-hook-start\nnew\n# kds-hook-end\n");
        assert!(repaired.contains("before"), "repaired:\n{repaired}");
        assert!(repaired.contains("after"), "repaired:\n{repaired}");
        assert!(repaired.contains("new"), "repaired:\n{repaired}");
        assert!(!repaired.contains("old"), "repaired:\n{repaired}");
        assert_eq!(repaired.matches(START).count(), 1, "repaired:\n{repaired}");
        assert_eq!(repaired.matches(END).count(), 1, "repaired:\n{repaired}");
    }

    #[test]
    fn powershell_hook_prompt_marks_existing_prompt_without_duplication() {
        if !cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("prompt.ps1");
        let mut script = std::fs::File::create(&script_path).unwrap();
        write!(
            script,
            r#"
function prompt {{ 'BASE> ' }}
{}
"prompt1=$(prompt)"
{}
"prompt2=$(prompt)"
"#,
            hook_block().unwrap(),
            hook_block().unwrap()
        )
        .unwrap();
        drop(script);

        let output = match Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            .output()
        {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => panic!("run pwsh: {err}"),
        };

        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("prompt1=KDS BASE>"), "stdout:\n{stdout}");
        assert!(stdout.contains("prompt2=KDS BASE>"), "stdout:\n{stdout}");
        assert!(!stdout.contains("KDS KDS"), "stdout:\n{stdout}");
    }

    #[test]
    fn powershell_hook_preserves_bare_double_dash() {
        if !cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let fake_cargo = dir.path().join("cargo.cmd");
        let fake_git = dir.path().join("git.cmd");
        let fake_just = dir.path().join("just.cmd");
        let fake_npm = dir.path().join("npm.cmd");
        let fake_pnpm = dir.path().join("pnpm.cmd");
        let fake_python = dir.path().join("python.cmd");
        let fake_kds = dir.path().join("kds.cmd");
        std::fs::write(
            &fake_cargo,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho [%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();
        std::fs::write(
            &fake_git,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho [%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();
        for fake in [&fake_just, &fake_npm, &fake_pnpm, &fake_python] {
            std::fs::write(
                fake,
                "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho native:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
            )
            .unwrap();
        }
        std::fs::write(
            &fake_kds,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho kds:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();

        let script_path = dir.path().join("test.ps1");
        let mut script = std::fs::File::create(&script_path).unwrap();
        write!(
            script,
            r#"
{}
$env:PATH = '{};' + $env:PATH
$script:KdsExe = '{}'
$script:KdsCommand = 'kds.cmd'
"manual-kds-command"
KDS gain
"manual-kds-wrap"
KDS -- cargo test
"native"
cargo run -- --help
"wrapped"
cargo test -- --nocapture
"git-status"
git status --short
"git-status-capture"
$s = git status --porcelain
"captured:$($s -join '|')"
"git-diff"
git diff --exit-code
"npm-test"
npm test
"npm-run-test"
npm run test
"npm-run-deploy"
npm run deploy
"pnpm-run-build"
pnpm run build
"pnpm-run-deploy"
pnpm run deploy
"just-test"
just test
"just-deploy"
just deploy
"python-pytest"
python -m pytest scripts/test_example.py
"python-unittest"
python -m unittest scripts.test_publish_local_codex
"python-script"
python scripts/test_publish_local_codex.py
"#,
            hook_block().unwrap(),
            dir.path().display(),
            fake_kds.display()
        )
        .unwrap();
        drop(script);

        let output = match Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            .output()
        {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => panic!("run pwsh: {err}"),
        };

        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("manual-kds-command\r\nkds:[gain]")
                || stdout.contains("manual-kds-command\nkds:[gain]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("manual-kds-wrap\r\nkds:[--]\r\nkds:[cargo]\r\nkds:[test]")
                || stdout.contains("manual-kds-wrap\nkds:[--]\nkds:[cargo]\nkds:[test]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("native\r\n[run]\r\n[--]\r\n[--help]")
                || stdout.contains("native\n[run]\n[--]\n[--help]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains(
                "wrapped\r\nkds:[--]\r\nkds:[cargo]\r\nkds:[test]\r\nkds:[--]\r\nkds:[--nocapture]"
            ) || stdout.contains(
                "wrapped\nkds:[--]\nkds:[cargo]\nkds:[test]\nkds:[--]\nkds:[--nocapture]"
            ),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("git-status\r\n[status]\r\n[--short]")
                || stdout.contains("git-status\n[status]\n[--short]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("git-status-capture\r\ncaptured:[status]|[--porcelain]")
                || stdout.contains("git-status-capture\ncaptured:[status]|[--porcelain]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("git-diff\r\n[diff]\r\n[--exit-code]")
                || stdout.contains("git-diff\n[diff]\n[--exit-code]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("npm-test\r\nkds:[--]\r\nkds:[npm]\r\nkds:[test]")
                || stdout.contains("npm-test\nkds:[--]\nkds:[npm]\nkds:[test]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("npm-run-test\r\nkds:[--]\r\nkds:[npm]\r\nkds:[run]\r\nkds:[test]")
                || stdout.contains("npm-run-test\nkds:[--]\nkds:[npm]\nkds:[run]\nkds:[test]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("npm-run-deploy\r\nnative:[run]\r\nnative:[deploy]")
                || stdout.contains("npm-run-deploy\nnative:[run]\nnative:[deploy]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("pnpm-run-build\r\nkds:[--]\r\nkds:[pnpm]\r\nkds:[run]\r\nkds:[build]")
                || stdout.contains("pnpm-run-build\nkds:[--]\nkds:[pnpm]\nkds:[run]\nkds:[build]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("pnpm-run-deploy\r\nnative:[run]\r\nnative:[deploy]")
                || stdout.contains("pnpm-run-deploy\nnative:[run]\nnative:[deploy]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("just-test\r\nkds:[--]\r\nkds:[just]\r\nkds:[test]")
                || stdout.contains("just-test\nkds:[--]\nkds:[just]\nkds:[test]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("just-deploy\r\nnative:[deploy]")
                || stdout.contains("just-deploy\nnative:[deploy]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("python-pytest\r\nkds:[--]\r\nkds:[python]\r\nkds:[-m]\r\nkds:[pytest]\r\nkds:[scripts/test_example.py]")
                || stdout.contains("python-pytest\nkds:[--]\nkds:[python]\nkds:[-m]\nkds:[pytest]\nkds:[scripts/test_example.py]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("python-unittest\r\nkds:[--]\r\nkds:[python]\r\nkds:[-m]\r\nkds:[unittest]\r\nkds:[scripts.test_publish_local_codex]")
                || stdout.contains("python-unittest\nkds:[--]\nkds:[python]\nkds:[-m]\nkds:[unittest]\nkds:[scripts.test_publish_local_codex]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("python-script\r\nnative:[scripts/test_publish_local_codex.py]")
                || stdout.contains("python-script\nnative:[scripts/test_publish_local_codex.py]"),
            "stdout:\n{stdout}"
        );
    }

    #[test]
    fn powershell_hook_uses_native_applications_despite_prior_functions() {
        if !cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let fake_cargo = dir.path().join("cargo.cmd");
        let fake_npm = dir.path().join("npm.cmd");
        let fake_kds = dir.path().join("kds.cmd");
        std::fs::write(
            &fake_cargo,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho native-cargo:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();
        std::fs::write(
            &fake_npm,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho native-npm:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();
        std::fs::write(
            &fake_kds,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho kds:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();

        let script_path = dir.path().join("prior-functions.ps1");
        let mut script = std::fs::File::create(&script_path).unwrap();
        write!(
            script,
            r#"
function cargo {{ "user-cargo-function" }}
function npm {{ "user-npm-function" }}
{}
$env:PATH = '{};' + $env:PATH
$script:KdsExe = '{}'
$script:KdsCommand = 'kds.cmd'
"cargo-run-native"
cargo run
"cargo-check-wrapped"
cargo check
"npm-deploy-native"
npm run deploy
"npm-lint-wrapped"
npm run lint
"npm-test-wrapped"
npm test
"done"
"#,
            hook_block().unwrap(),
            dir.path().display(),
            fake_kds.display()
        )
        .unwrap();
        drop(script);

        let output = match Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            .output()
        {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => panic!("run pwsh: {err}"),
        };

        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("cargo-run-native\r\nnative-cargo:[run]")
                || stdout.contains("cargo-run-native\nnative-cargo:[run]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("cargo-check-wrapped\r\nkds:[--]\r\nkds:[cargo]\r\nkds:[check]")
                || stdout.contains("cargo-check-wrapped\nkds:[--]\nkds:[cargo]\nkds:[check]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("npm-deploy-native\r\nnative-npm:[run]\r\nnative-npm:[deploy]")
                || stdout.contains("npm-deploy-native\nnative-npm:[run]\nnative-npm:[deploy]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("npm-lint-wrapped\r\nkds:[--]\r\nkds:[npm]\r\nkds:[run]\r\nkds:[lint]")
                || stdout.contains("npm-lint-wrapped\nkds:[--]\nkds:[npm]\nkds:[run]\nkds:[lint]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("npm-test-wrapped\r\nkds:[--]\r\nkds:[npm]\r\nkds:[test]")
                || stdout.contains("npm-test-wrapped\nkds:[--]\nkds:[npm]\nkds:[test]"),
            "stdout:\n{stdout}"
        );
        assert!(!stdout.contains("user-cargo-function"), "stdout:\n{stdout}");
        assert!(!stdout.contains("user-npm-function"), "stdout:\n{stdout}");
    }

    #[test]
    fn powershell_hook_missing_native_commands_do_not_recurse() {
        if !cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("missing-native.ps1");
        let mut script = std::fs::File::create(&script_path).unwrap();
        write!(
            script,
            r#"
{}
$env:PATH = ''
"missing-cargo"
cargo run
"missing-just"
just deploy
"missing-npm"
npm publish
"missing-pnpm"
pnpm deploy
"missing-python"
python --version
"after-missing"
exit $LASTEXITCODE
"#,
            hook_block().unwrap()
        )
        .unwrap();
        drop(script);

        let output = match Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            .output()
        {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => panic!("run pwsh: {err}"),
        };

        assert_eq!(output.status.code(), Some(127));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for marker in [
            "missing-cargo",
            "missing-just",
            "missing-npm",
            "missing-pnpm",
            "missing-python",
        ] {
            assert!(stdout.contains(marker), "stdout:\n{stdout}");
        }
        assert!(stdout.contains("after-missing"), "stdout:\n{stdout}");
        for command in ["cargo", "just", "npm", "pnpm", "python"] {
            assert!(
                stderr.contains(&format!("kds hook: command not found: {command}")),
                "stderr:\n{stderr}"
            );
        }
        assert!(!stderr.contains("call depth overflow"), "stderr:\n{stderr}");
    }
}
