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
$global:KdsExe = '{exe}'
$global:KdsCommand = [System.IO.Path]::GetFileName($global:KdsExe)
$global:KdsExeDir = Split-Path -Parent $global:KdsExe
if ($global:KdsExeDir -and -not (($env:PATH -split ';') -contains $global:KdsExeDir)) {{
  $env:PATH = "$global:KdsExeDir;$env:PATH"
}}
function KDS {{
  $kdsArgs = @($args)
  $kdsStatement = [string]$MyInvocation.Statement
  if ($kdsStatement -match '(?i)^\s*KDS\s+--(?:\s|$)' -and ($kdsArgs.Count -eq 0 -or [string]$kdsArgs[0] -ne '--')) {{
    $kdsArgs = @('--') + $kdsArgs
  }}
  & $global:KdsCommand @kdsArgs
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
  if ($null -eq $Statement -or -not $Statement.Contains('--')) {{
    return $out.ToArray()
  }}
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
  return ([string]$Name) -match '^(test|build|check|lint|typecheck|format-check|fmt-check|ci|clippy|vet|compile)(-[A-Za-z0-9_.-]+)?$'
}}
function _kds_has_flag {{
  param([object[]]$Rest, [string[]]$Flags)
  foreach ($arg in $Rest) {{
    if ($Flags -contains ([string]$arg).ToLowerInvariant()) {{ return $true }}
  }}
  return $false
}}
function _kds_has_blocking_arg {{
  param([object[]]$Rest)
  return (_kds_has_flag $Rest @('--watch','--watchall','--watch-all','-w','--ui','--serve','--dev','--inspect','--inspect-brk'))
}}
function _kds_first_non_flag {{
  param([object[]]$Rest)
  foreach ($arg in $Rest) {{
    $text = [string]$arg
    if ([string]::IsNullOrWhiteSpace($text)) {{ continue }}
    if ($text -eq '--') {{ continue }}
    if ($text.StartsWith('-')) {{ continue }}
    return $text
  }}
  return $null
}}
function _kds_safe_python_module {{
  param([string]$Name)
  return @('pytest','unittest','ruff','mypy','pyright') -contains [string]$Name
}}
function _kds_script_profile {{
  param([object[]]$Rest)
  $first = _kds_first_non_flag $Rest
  if ($null -ne $first -and (_kds_safe_task $first)) {{ return $true }}
  if ($Rest.Count -ge 2 -and @('run','run-script') -contains ([string]$Rest[0]).ToLowerInvariant() -and (_kds_safe_task $Rest[1])) {{ return $true }}
  return $false
}}
function _kds_gradle_profile {{
  param([object[]]$Rest)
  $task = (_kds_first_non_flag $Rest)
  if ($null -eq $task) {{ return $false }}
  $lower = ([string]$task).ToLowerInvariant()
  if ($lower -match '(publish|deploy|upload|release|sign)') {{ return $false }}
  return ($lower -match '^(test|check|build|lint|compile[a-z0-9_.-]*|.*test.*|.*check)$')
}}
function _kds_profile_should_wrap {{
  param([string]$Name, [object[]]$Rest)
  $command = ([string]$Name).ToLowerInvariant()
  $first = $null
  $second = $null
  if ($Rest.Count -gt 0) {{ $first = ([string]$Rest[0]).ToLowerInvariant() }}
  if ($Rest.Count -gt 1) {{ $second = ([string]$Rest[1]).ToLowerInvariant() }}
  switch ($command) {{
    {{ @('just','make','task') -contains $_ }} {{ return (_kds_script_profile $Rest) }}
    {{ @('npm','pnpm','yarn','bun') -contains $_ }} {{
      if ($null -ne $first -and $first -eq 'test') {{ return $true }}
      return (_kds_script_profile $Rest)
    }}
    'deno' {{
      if ($null -ne $first -and @('test','check','lint') -contains $first) {{ return $true }}
      return ($Rest.Count -ge 2 -and $first -eq 'task' -and (_kds_safe_task $Rest[1]))
    }}
    {{ @('tsc','vue-tsc','jest','vitest') -contains $_ }} {{ return -not (_kds_has_blocking_arg $Rest) }}
    'eslint' {{ return $true }}
    'biome' {{ return ($null -ne $first -and @('check','ci','lint') -contains $first) }}
    'prettier' {{ return (_kds_has_flag $Rest @('--check','-c')) }}
    'playwright' {{ return ($null -ne $first -and $first -eq 'test') }}
    'pytest' {{ return $true }}
    {{ @('python','py') -contains $_ }} {{ return ($Rest.Count -ge 2 -and $first -eq '-m' -and (_kds_safe_python_module $Rest[1])) }}
    'ruff' {{
      if ($null -eq $first) {{ return $false }}
      if ($first -eq 'check') {{ return $true }}
      return ($first -eq 'format' -and (_kds_has_flag $Rest @('--check')))
    }}
    {{ @('mypy','pyright') -contains $_ }} {{ return $true }}
    'uv' {{ return ($Rest.Count -ge 2 -and $first -eq 'run' -and (_kds_safe_python_module $Rest[1])) }}
    'go' {{ return ($null -ne $first -and @('test','build','vet') -contains $first) }}
    'dotnet' {{ return ($null -ne $first -and @('test','build') -contains $first) }}
    {{ @('mvn','mvnw','maven') -contains $_ }} {{
      $goal = _kds_first_non_flag $Rest
      return ($null -ne $goal -and @('test','verify','package','compile') -contains ([string]$goal).ToLowerInvariant())
    }}
    {{ @('gradle','gradlew') -contains $_ }} {{ return (_kds_gradle_profile $Rest) }}
    'composer' {{ return (_kds_script_profile $Rest) }}
    'phpunit' {{ return $true }}
    'bundle' {{ return ($Rest.Count -ge 2 -and $first -eq 'exec' -and @('rspec','rake','rails') -contains $second -and ($Rest.Count -lt 3 -or (_kds_safe_task $Rest[2]) -or $second -eq 'rspec')) }}
    'rails' {{ return ($null -ne $first -and $first -eq 'test') }}
    'rspec' {{ return $true }}
    'mix' {{ return ($null -ne $first -and @('test','compile') -contains $first) }}
    'cmake' {{ return ($null -ne $first -and $first -eq '--build') }}
    'ninja' {{ return (_kds_script_profile $Rest) }}
    'ctest' {{ return $true }}
    'mise' {{ return ($Rest.Count -ge 2 -and @('run','r') -contains $first -and (_kds_safe_task $Rest[1])) }}
    default {{ return $false }}
  }}
}}
function _kds_profile_call {{
  param([string]$Name, [object[]]$Rest)
  if (_kds_profile_should_wrap $Name $Rest) {{ _kds_wrap $Name $Rest }} else {{ _kds_call_native $Name $Rest }}
}}
function cargo {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'cargo' $rest
}}
function just {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'just' $rest
}}
function npm {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'npm' $rest
}}
function pnpm {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'pnpm' $rest
}}
function yarn {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'yarn' $rest
}}
function bun {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'bun' $rest
}}
function deno {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'deno' $rest
}}
function pytest {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'pytest' $rest
}}
function python {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'python' $rest
}}
function py {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'py' $rest
}}
function tsc {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'tsc' $rest
}}
function vue-tsc {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'vue-tsc' $rest
}}
function eslint {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'eslint' $rest
}}
function biome {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'biome' $rest
}}
function prettier {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'prettier' $rest
}}
function vitest {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'vitest' $rest
}}
function jest {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'jest' $rest
}}
function playwright {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'playwright' $rest
}}
function ruff {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'ruff' $rest
}}
function mypy {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'mypy' $rest
}}
function pyright {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'pyright' $rest
}}
function uv {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'uv' $rest
}}
function dotnet {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'dotnet' $rest
}}
function go {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'go' $rest
}}
function mvn {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'mvn' $rest
}}
function mvnw {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'mvnw' $rest
}}
function maven {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'maven' $rest
}}
function gradle {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'gradle' $rest
}}
function gradlew {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'gradlew' $rest
}}
function composer {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'composer' $rest
}}
function phpunit {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'phpunit' $rest
}}
function bundle {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'bundle' $rest
}}
function rails {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'rails' $rest
}}
function rspec {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'rspec' $rest
}}
function mix {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'mix' $rest
}}
function cmake {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'cmake' $rest
}}
function make {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'make' $rest
}}
function ninja {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'ninja' $rest
}}
function ctest {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'ctest' $rest
}}
function task {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'task' $rest
}}
function mise {{
  $rest = @(_kds_restore_args $args $MyInvocation.Statement)
  _kds_profile_call 'mise' $rest
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

    fn assert_sequence(stdout: &str, sequence: &[&str]) {
        let crlf = sequence.join("\r\n");
        let lf = sequence.join("\n");
        assert!(
            stdout.contains(&crlf) || stdout.contains(&lf),
            "missing sequence {:?}\nstdout:\n{}",
            sequence,
            stdout
        );
    }

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
        assert!(!block.contains("$kdsCommands = @("), "block:\n{block}");
        assert!(
            block.contains("$kdsArgs = @('--', $Name) + $Rest"),
            "block:\n{block}"
        );
        assert!(
            block.contains("& $global:KdsCommand @kdsArgs"),
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
$global:KdsExe = '{}'
$global:KdsCommand = 'kds.cmd'
"manual-kds-command"
KDS gain
"manual-kds-wrap"
KDS -- cargo test
"native"
cargo run -- --help
"cargo-test-native"
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
"npm-run-test-fast"
npm run test-fast
"npm-run-deploy"
npm run deploy
"pnpm-run-build"
pnpm run build
"pnpm-run-build-local"
pnpm run build-local
"pnpm-run-deploy"
pnpm run deploy
"just-test"
just test
"just-test-fast"
just test-fast
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
            stdout.contains("cargo-test-native\r\n[test]\r\n[--]\r\n[--nocapture]")
                || stdout.contains("cargo-test-native\n[test]\n[--]\n[--nocapture]"),
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
            stdout.contains(
                "npm-run-test-fast\r\nkds:[--]\r\nkds:[npm]\r\nkds:[run]\r\nkds:[test-fast]"
            ) || stdout
                .contains("npm-run-test-fast\nkds:[--]\nkds:[npm]\nkds:[run]\nkds:[test-fast]"),
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
            stdout.contains(
                "pnpm-run-build-local\r\nkds:[--]\r\nkds:[pnpm]\r\nkds:[run]\r\nkds:[build-local]"
            ) || stdout.contains(
                "pnpm-run-build-local\nkds:[--]\nkds:[pnpm]\nkds:[run]\nkds:[build-local]"
            ),
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
            stdout.contains("just-test-fast\r\nkds:[--]\r\nkds:[just]\r\nkds:[test-fast]")
                || stdout.contains("just-test-fast\nkds:[--]\nkds:[just]\nkds:[test-fast]"),
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
    fn powershell_hook_wraps_builtin_profiles() {
        if !cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let fake_kds = dir.path().join("kds.cmd");
        std::fs::write(
            &fake_kds,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho kds:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();
        for name in [
            "yarn", "bun", "deno", "go", "mvn", "gradle", "dotnet", "composer", "phpunit",
            "bundle", "rails", "mix", "cmake", "make", "ninja", "ctest", "task", "mise", "jest",
        ] {
            std::fs::write(
                dir.path().join(format!("{name}.cmd")),
                "@echo off\r\necho native-%~n0:[%*]\r\n",
            )
            .unwrap();
        }

        let script_path = dir.path().join("profiles.ps1");
        let mut script = std::fs::File::create(&script_path).unwrap();
        write!(
            script,
            r#"
{}
$env:PATH = '{};' + $env:PATH
$global:KdsExe = '{}'
$global:KdsCommand = 'kds.cmd'
"yarn-test"
yarn test
"bun-run-lint"
bun run lint
"deno-task-test"
deno task test
"go-vet"
go vet ./...
"mvn-test"
mvn test
"gradle-check"
gradle check
"dotnet-build"
dotnet build
"composer-test"
composer test
"phpunit-direct"
phpunit
"bundle-rspec"
bundle exec rspec
"rails-test"
rails test
"mix-compile"
mix compile
"cmake-build"
cmake --build build
"make-test"
make test
"ninja-test"
ninja test
"ctest-direct"
ctest
"task-test"
task test
"mise-run-test"
mise run test
"jest-watch-native"
jest --watch
"make-deploy-native"
make deploy
"mvn-deploy-native"
mvn deploy
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
        for (marker, command) in [
            ("yarn-test", "yarn"),
            ("bun-run-lint", "bun"),
            ("deno-task-test", "deno"),
            ("go-vet", "go"),
            ("mvn-test", "mvn"),
            ("gradle-check", "gradle"),
            ("dotnet-build", "dotnet"),
            ("composer-test", "composer"),
            ("phpunit-direct", "phpunit"),
            ("bundle-rspec", "bundle"),
            ("rails-test", "rails"),
            ("mix-compile", "mix"),
            ("cmake-build", "cmake"),
            ("make-test", "make"),
            ("ninja-test", "ninja"),
            ("ctest-direct", "ctest"),
            ("task-test", "task"),
            ("mise-run-test", "mise"),
        ] {
            assert_sequence(&stdout, &[marker, "kds:[--]", &format!("kds:[{command}]")]);
        }
        assert_sequence(&stdout, &["jest-watch-native", "native-jest:[--watch]"]);
        assert_sequence(&stdout, &["make-deploy-native", "native-make:[deploy]"]);
        assert_sequence(&stdout, &["mvn-deploy-native", "native-mvn:[deploy]"]);
    }

    #[test]
    fn powershell_hook_survives_strict_child_scripts() {
        if !cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let fake_npm = dir.path().join("npm.cmd");
        let fake_kds = dir.path().join("kds.cmd");
        std::fs::write(&fake_npm, "@echo off\r\necho native-npm:[%*]\r\n").unwrap();
        std::fs::write(
            &fake_kds,
            "@echo off\r\n:loop\r\nif \"%~1\"==\"\" goto end\r\necho kds:[%~1]\r\nshift\r\ngoto loop\r\n:end\r\n",
        )
        .unwrap();

        let profile_path = dir.path().join("profile.ps1");
        std::fs::write(&profile_path, hook_block().unwrap()).unwrap();

        let child_path = dir.path().join("strict-child.ps1");
        std::fs::write(
            &child_path,
            r#"
Set-StrictMode -Version Latest
"strict-child"
npm test
"after-cargo"
"#,
        )
        .unwrap();

        let output = match Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-Command")
            .arg(format!(
                ". '{}'; $env:PATH = '{};' + $env:PATH; $global:KdsExe = '{}'; $global:KdsCommand = 'kds.cmd'; & '{}'",
                profile_path.display(),
                dir.path().display(),
                fake_kds.display(),
                child_path.display()
            ))
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
            stdout.contains("strict-child\r\nkds:[--]\r\nkds:[npm]\r\nkds:[test]\r\nafter-cargo")
                || stdout.contains("strict-child\nkds:[--]\nkds:[npm]\nkds:[test]\nafter-cargo"),
            "stdout:\n{stdout}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("KdsCommand"), "stderr:\n{stderr}");
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
$global:KdsExe = '{}'
$global:KdsCommand = 'kds.cmd'
"cargo-run-native"
cargo run
"cargo-check-native"
cargo check
"cargo-build-native"
cargo build
"cargo-clippy-native"
cargo clippy
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
            stdout.contains("cargo-check-native\r\nnative-cargo:[check]")
                || stdout.contains("cargo-check-native\nnative-cargo:[check]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("cargo-build-native\r\nnative-cargo:[build]")
                || stdout.contains("cargo-build-native\nnative-cargo:[build]"),
            "stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("cargo-clippy-native\r\nnative-cargo:[clippy]")
                || stdout.contains("cargo-clippy-native\nnative-cargo:[clippy]"),
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
