#!/usr/bin/env pwsh

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$DryRun = $false
$NoHook = $false
$Help = $false

foreach ($arg in $args) {
  switch ($arg) {
    "--dry-run" { $DryRun = $true }
    "-DryRun" { $DryRun = $true }
    "--no-hook" { $NoHook = $true }
    "--help" { $Help = $true }
    "-h" { $Help = $true }
    "-Help" { $Help = $true }
    default {
      Write-Error "Unknown argument: $arg"
      exit 2
    }
  }
}

function Split-KdsPathList {
  param([AllowNull()][string]$Value)
  if ([string]::IsNullOrWhiteSpace($Value)) {
    return @()
  }
  return @($Value -split ';' | ForEach-Object { $_.Trim() } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Normalize-KdsPathEntry {
  param([AllowNull()][string]$Value)
  if ([string]::IsNullOrWhiteSpace($Value)) {
    return ""
  }
  return (($Value.Trim()) -replace '[\\/]+$', '')
}

function Test-KdsPathListContains {
  param(
    [string[]]$Entries,
    [string]$Candidate
  )
  $needle = Normalize-KdsPathEntry $Candidate
  foreach ($entry in $Entries) {
    if ((Normalize-KdsPathEntry $entry) -ieq $needle) {
      return $true
    }
  }
  return $false
}

function Add-KdsUserPath {
  param([string]$InstallDir)
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $userPathEntries = Split-KdsPathList $userPath
  $userPathHasInstallDir = Test-KdsPathListContains $userPathEntries $InstallDir
  if (-not $userPathHasInstallDir) {
    $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) {
      $InstallDir
    } else {
      "$($userPath.TrimEnd(';'));$InstallDir"
    }
    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    Write-Host "Updated user PATH with: $InstallDir"
  } else {
    Write-Host "User PATH already includes: $InstallDir"
  }

  $processPathEntries = Split-KdsPathList $env:PATH
  if (-not (Test-KdsPathListContains $processPathEntries $InstallDir)) {
    $env:PATH = "$InstallDir;$env:PATH"
    Write-Host "Updated current session PATH with: $InstallDir"
  }
  return $true
}

function Get-KdsTimestamp {
  return (Get-Date).ToString("yyyyMMddTHHmmss")
}

function Backup-KdsFile {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    return $null
  }
  $backup = "$Path.kds-backup-$(Get-KdsTimestamp)"
  Copy-Item -LiteralPath $Path -Destination $backup -Force
  Write-Host "Backed up: $backup"
  return $backup
}

function Set-KdsFileContentIfChanged {
  param(
    [string]$Path,
    [string]$Content
  )
  $current = $null
  if (Test-Path -LiteralPath $Path -PathType Leaf) {
    $current = Get-Content -LiteralPath $Path -Raw
  }
  if ($current -eq $Content) {
    Write-Host "Already current: $Path"
    return $false
  }
  [void](Backup-KdsFile $Path)
  Set-Content -LiteralPath $Path -Value $Content -Encoding UTF8 -NoNewline
  Write-Host "Wrote: $Path"
  return $true
}

function Get-KdsSha256ForString {
  param([string]$Value)
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($Value)
  $sha = [System.Security.Cryptography.SHA256]::Create()
  try {
    $hashBytes = $sha.ComputeHash($bytes)
  } finally {
    $sha.Dispose()
  }
  $hex = ($hashBytes | ForEach-Object { $_.ToString("x2") }) -join ""
  return "sha256:$hex"
}

function Get-KdsCommandHookHash {
  param(
    [string]$Matcher,
    [string]$Command,
    [int]$Timeout,
    [string]$StatusMessage
  )
  $identity = [ordered]@{
    event_name = "pre_tool_use"
    hooks = @(
      [ordered]@{
        async = $false
        command = $Command
        statusMessage = $StatusMessage
        timeout = $Timeout
        type = "command"
      }
    )
    matcher = $Matcher
  }
  $json = $identity | ConvertTo-Json -Depth 10 -Compress
  return Get-KdsSha256ForString $json
}

function Get-KdsJsonProperty {
  param(
    [AllowNull()][object]$Object,
    [string]$Name
  )
  if ($null -eq $Object) {
    return $null
  }
  $property = $Object.PSObject.Properties[$Name]
  if ($null -eq $property) {
    return $null
  }
  return $property.Value
}

function Set-KdsJsonProperty {
  param(
    [object]$Object,
    [string]$Name,
    [AllowNull()][object]$Value
  )
  $property = $Object.PSObject.Properties[$Name]
  if ($null -eq $property) {
    $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value
  } else {
    $property.Value = $Value
  }
}

function Get-KdsDesktopHookScript {
  return @'
$ErrorActionPreference = 'Stop'

$inputJson = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($inputJson)) {
  exit 0
}

try {
  $event = $inputJson | ConvertFrom-Json
} catch {
  exit 0
}

if ($event.hook_event_name -ne 'PreToolUse') {
  exit 0
}

if ($event.tool_name -ne 'Bash') {
  exit 0
}

$command = [string]$event.tool_input.command
if ([string]::IsNullOrWhiteSpace($command)) {
  exit 0
}

$trimmed = $command.Trim()
if ($trimmed -match '(?i)^kds(\.exe)?\b') {
  exit 0
}

# KDS executes argv directly, so only rewrite simple commands with no shell control.
if ($trimmed -match '[\r\n]|&&|\|\||[|<>;&`]') {
  exit 0
}

$shouldWrap = $false
switch -Regex ($trimmed) {
  '(?i)^cargo\s+(check|test|build|clippy)(\s|$)' { $shouldWrap = $true; break }
  '(?i)^just\s+(test|build|check|lint|typecheck|ci|clippy)(\s|$)' { $shouldWrap = $true; break }
  '(?i)^npm\s+test(\s|$)' { $shouldWrap = $true; break }
  '(?i)^npm\s+run\s+(test|build|check|lint|typecheck|ci|clippy)(\s|$)' { $shouldWrap = $true; break }
  '(?i)^pnpm\s+test(\s|$)' { $shouldWrap = $true; break }
  '(?i)^pnpm\s+run\s+(test|build|check|lint|typecheck|ci|clippy)(\s|$)' { $shouldWrap = $true; break }
  '(?i)^pytest(\s|$)' { $shouldWrap = $true; break }
  '(?i)^python\s+-m\s+(pytest|unittest)(\s|$)' { $shouldWrap = $true; break }
  '(?i)^py\s+-m\s+(pytest|unittest)(\s|$)' { $shouldWrap = $true; break }
}

if (-not $shouldWrap) {
  exit 0
}

$kdsExe = Join-Path $env:LOCALAPPDATA 'CodexKD\bin\kds.exe'
if (-not (Test-Path -LiteralPath $kdsExe -PathType Leaf)) {
  $resolved = Get-Command kds.exe,kds -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $resolved) {
    exit 0
  }
  $kdsExe = $resolved.Source
}

$kdsDir = Split-Path -Parent $kdsExe
if ($kdsDir -and -not (($env:PATH -split ';') -contains $kdsDir)) {
  $env:PATH = "$kdsDir;$env:PATH"
}

$updatedCommand = "kds.exe -- $trimmed"

$response = [ordered]@{
  hookSpecificOutput = [ordered]@{
    hookEventName = 'PreToolUse'
    permissionDecision = 'allow'
    updatedInput = [ordered]@{
      command = $updatedCommand
    }
  }
}

$response | ConvertTo-Json -Depth 10 -Compress
'@
}

function Get-KdsCodexHomeCandidates {
  $candidates = [System.Collections.Generic.List[string]]::new()
  if (-not [string]::IsNullOrWhiteSpace($env:KDS_INSTALL_CODEX_HOME)) {
    [void]$candidates.Add($env:KDS_INSTALL_CODEX_HOME)
    return @(Get-KdsExistingUniquePaths $candidates)
  }
  if (-not [string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
    [void]$candidates.Add($env:CODEX_HOME)
  }
  if (-not [string]::IsNullOrWhiteSpace($HOME)) {
    [void]$candidates.Add((Join-Path $HOME ".codex"))
  }
  if (-not [string]::IsNullOrWhiteSpace($env:APPDATA)) {
    [void]$candidates.Add((Join-Path $env:APPDATA "Codex"))
  }
  if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    [void]$candidates.Add((Join-Path $env:LOCALAPPDATA "Codex"))
  }
  $desktop = [Environment]::GetFolderPath([Environment+SpecialFolder]::Desktop)
  if (-not [string]::IsNullOrWhiteSpace($desktop)) {
    [void]$candidates.Add((Join-Path $desktop "LOCAL-KD"))
  }
  return @(Get-KdsExistingUniquePaths $candidates)
}

function Get-KdsExistingUniquePaths {
  param([System.Collections.Generic.List[string]]$Candidates)
  $seen = @{}
  $homes = @()
  foreach ($candidate in $Candidates) {
    if ([string]::IsNullOrWhiteSpace($candidate)) {
      continue
    }
    if (-not (Test-Path -LiteralPath $candidate -PathType Container)) {
      continue
    }
    $fullPath = [System.IO.Path]::GetFullPath($candidate)
    $key = $fullPath.ToLowerInvariant()
    if ($seen.ContainsKey($key)) {
      continue
    }
    $seen[$key] = $true
    $homes += $fullPath
  }
  return $homes
}

function New-KdsDesktopHookEntry {
  param([string]$HookCommand)
  return [pscustomobject]@{
    matcher = "^Bash$"
    hooks = @(
      [pscustomobject]@{
        type = "command"
        command = $HookCommand
        commandWindows = $HookCommand
        timeout = 5
        statusMessage = "Routing allowlisted commands through KDS"
      }
    )
  }
}

function Set-KdsDesktopHookRecord {
  param(
    [object]$Hook,
    [string]$HookCommand
  )
  Set-KdsJsonProperty $Hook "type" "command"
  Set-KdsJsonProperty $Hook "command" $HookCommand
  Set-KdsJsonProperty $Hook "commandWindows" $HookCommand
  Set-KdsJsonProperty $Hook "timeout" 5
  Set-KdsJsonProperty $Hook "statusMessage" "Routing allowlisted commands through KDS"
}

function Update-KdsDesktopHooksConfig {
  param(
    [string]$ConfigPath,
    [string]$HookScriptPath
  )
  $hookCommand = "pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File `"$HookScriptPath`""
  $config = $null
  if (Test-Path -LiteralPath $ConfigPath -PathType Leaf) {
    try {
      $config = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
    } catch {
      Write-Warning "Existing hooks.json could not be parsed and will be replaced after backup: $ConfigPath"
    }
  }
  if ($null -eq $config) {
    $config = [pscustomobject]@{}
  }

  $hooks = Get-KdsJsonProperty $config "hooks"
  if ($null -eq $hooks) {
    $hooks = [pscustomobject]@{}
    Set-KdsJsonProperty $config "hooks" $hooks
  }

  $preToolUse = @(Get-KdsJsonProperty $hooks "PreToolUse")
  if ($preToolUse.Count -eq 1 -and $null -eq $preToolUse[0]) {
    $preToolUse = @()
  }

  $foundKdsHook = $false
  foreach ($entry in $preToolUse) {
    if ($null -eq $entry) {
      continue
    }
    $entryHooks = @(Get-KdsJsonProperty $entry "hooks")
    if ($entryHooks.Count -eq 1 -and $null -eq $entryHooks[0]) {
      continue
    }
    foreach ($hook in $entryHooks) {
      $command = [string](Get-KdsJsonProperty $hook "command")
      $commandWindows = [string](Get-KdsJsonProperty $hook "commandWindows")
      if ($command -like "*kds-pre-tool-use.ps1*" -or $commandWindows -like "*kds-pre-tool-use.ps1*") {
        Set-KdsDesktopHookRecord $hook $hookCommand
        $foundKdsHook = $true
      }
    }
  }

  if (-not $foundKdsHook) {
    $preToolUse += (New-KdsDesktopHookEntry $hookCommand)
  }
  Set-KdsJsonProperty $hooks "PreToolUse" @($preToolUse)
  return ($config | ConvertTo-Json -Depth 64)
}

function Get-KdsDesktopHookTrustEntries {
  param([string]$ConfigPath)
  if (-not (Test-Path -LiteralPath $ConfigPath -PathType Leaf)) {
    return @()
  }
  $config = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
  $hooks = Get-KdsJsonProperty $config "hooks"
  $preToolUse = @(Get-KdsJsonProperty $hooks "PreToolUse")
  if ($preToolUse.Count -eq 1 -and $null -eq $preToolUse[0]) {
    return @()
  }

  $entries = @()
  for ($groupIndex = 0; $groupIndex -lt $preToolUse.Count; $groupIndex += 1) {
    $group = $preToolUse[$groupIndex]
    if ($null -eq $group) {
      continue
    }
    $matcher = [string](Get-KdsJsonProperty $group "matcher")
    if ([string]::IsNullOrWhiteSpace($matcher)) {
      $matcher = ".*"
    }
    $handlers = @(Get-KdsJsonProperty $group "hooks")
    if ($handlers.Count -eq 1 -and $null -eq $handlers[0]) {
      continue
    }
    for ($handlerIndex = 0; $handlerIndex -lt $handlers.Count; $handlerIndex += 1) {
      $handler = $handlers[$handlerIndex]
      $command = [string](Get-KdsJsonProperty $handler "command")
      $commandWindows = [string](Get-KdsJsonProperty $handler "commandWindows")
      if (-not ($command -like "*kds-pre-tool-use.ps1*" -or $commandWindows -like "*kds-pre-tool-use.ps1*")) {
        continue
      }
      $effectiveCommand = if (-not [string]::IsNullOrWhiteSpace($commandWindows)) {
        $commandWindows
      } else {
        $command
      }
      $timeout = Get-KdsJsonProperty $handler "timeout"
      if ($null -eq $timeout) {
        $timeout = 600
      }
      $statusMessage = [string](Get-KdsJsonProperty $handler "statusMessage")
      $hash = Get-KdsCommandHookHash $matcher $effectiveCommand ([int]$timeout) $statusMessage
      $key = "$ConfigPath`:pre_tool_use`:$groupIndex`:$handlerIndex"
      $entries += [pscustomobject]@{
        Key = $key
        TrustedHash = $hash
      }
    }
  }
  return $entries
}

function ConvertTo-KdsTomlQuotedKey {
  param([string]$Value)
  if (-not $Value.Contains("'")) {
    return "'" + $Value + "'"
  }
  $escaped = $Value.Replace('\', '\\').Replace('"', '\"')
  $escaped = $escaped.Replace("`r", "\r").Replace("`n", "\n").Replace("`t", "\t")
  return '"' + $escaped + '"'
}

function Update-KdsCodexConfigHookTrust {
  param(
    [string]$CodexHome,
    [string]$HooksConfigPath
  )
  $trustEntries = @(Get-KdsDesktopHookTrustEntries $HooksConfigPath)
  if ($trustEntries.Count -eq 0) {
    return 0
  }

  $configPath = Join-Path $CodexHome "config.toml"
  $current = if (Test-Path -LiteralPath $configPath -PathType Leaf) {
    Get-Content -LiteralPath $configPath -Raw
  } else {
    ""
  }
  $updated = $current
  foreach ($entry in $trustEntries) {
    $quotedKey = ConvertTo-KdsTomlQuotedKey $entry.Key
    $sectionHeader = "[hooks.state.$quotedKey]"
    $sectionPattern = "(?ms)^\[hooks\.state\.$([regex]::Escape($quotedKey))\]\s*(.*?)(?=^\[|\z)"
    $replacement = "$sectionHeader`r`ntrusted_hash = `"$($entry.TrustedHash)`"`r`n`r`n"
    if ($updated -match $sectionPattern) {
      $updated = [regex]::Replace($updated, $sectionPattern, $replacement, 1)
    } else {
      if (-not $updated.EndsWith("`n") -and $updated.Length -gt 0) {
        $updated += "`r`n"
      }
      $updated += "`r`n$replacement"
    }
  }

  if ($updated -eq $current) {
    Write-Host "Codex Desktop hook trust already current: $configPath"
    return $trustEntries.Count
  }
  [void](Backup-KdsFile $configPath)
  Set-Content -LiteralPath $configPath -Value $updated -Encoding UTF8 -NoNewline
  Write-Host "Updated Codex Desktop hook trust: $configPath"
  return $trustEntries.Count
}

function Install-KdsCodexDesktopHooks {
  param([bool]$DryRun)
  $homes = @(Get-KdsCodexHomeCandidates)
  if ($homes.Count -eq 0) {
    Write-Host "Codex Desktop hook: no existing Codex home found"
    return 0
  }

  foreach ($codexHome in $homes) {
    $hooksDir = Join-Path $codexHome "hooks"
    $hookScriptPath = Join-Path $hooksDir "kds-pre-tool-use.ps1"
    $hooksConfigPath = Join-Path $codexHome "hooks.json"
    if ($DryRun) {
      Write-Host "Codex Desktop hook: would install/update $hookScriptPath"
      Write-Host "Codex Desktop hooks config: would install/update $hooksConfigPath"
      continue
    }

    New-Item -ItemType Directory -Force -Path $hooksDir | Out-Null
    [void](Set-KdsFileContentIfChanged $hookScriptPath (Get-KdsDesktopHookScript))
    $updatedConfig = Update-KdsDesktopHooksConfig $hooksConfigPath $hookScriptPath
    [void](Set-KdsFileContentIfChanged $hooksConfigPath $updatedConfig)
    [void](Update-KdsCodexConfigHookTrust $codexHome $hooksConfigPath)
    Write-Host "Installed Codex Desktop hook for: $codexHome"
  }
  return $homes.Count
}

if ($Help) {
  @"
KDS Windows installer

Usage:
  ./scripts/install.ps1 [--dry-run] [--no-hook] [--help]

Behavior:
  - builds KDS from this repository
  - installs kds.exe to %LOCALAPPDATA%\CodexKD\bin
  - adds the install directory to the user PATH when missing
  - installs the automatic PowerShell hook by default unless --no-hook is set
  - installs or updates a Codex Desktop PreToolUse hook for detected Codex homes
"@ | Write-Host
  exit 0
}

$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
$installDir = Join-Path $env:LOCALAPPDATA "CodexKD\bin"
$targetExe = Join-Path $installDir "kds.exe"
$builtExe = Join-Path $repo "target\release\kds.exe"
$userPathEntries = Split-KdsPathList ([Environment]::GetEnvironmentVariable("Path", "User"))
$pathHasInstallDir = Test-KdsPathListContains $userPathEntries $installDir

Write-Host "KDS install plan"
Write-Host "Repository: $repo"
Write-Host "Install directory: $installDir"
Write-Host "Binary: $targetExe"
Write-Host "User PATH: $(if ($pathHasInstallDir) { 'already includes install directory' } else { 'will add install directory' })"
if ($NoHook) {
  Write-Host "Automatic hooks: skipped by --no-hook"
} else {
  Write-Host "Automatic PowerShell hook: profile managed by kds hook install powershell"
  Write-Host "Automatic Codex Desktop hook: install/update for detected Codex homes"
}

if ($DryRun) {
  if (-not $NoHook) {
    [void](Install-KdsCodexDesktopHooks $true)
  }
  Write-Host "Dry run: no source build, no binary copy, no PATH edit, and no hook/profile/Codex Desktop edit."
  exit 0
}

Push-Location $repo
try {
  cargo build --release
  if ($LASTEXITCODE -ne 0) {
    throw "cargo build --release failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location
}

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -Force -Path $builtExe -Destination $targetExe
Write-Host "Wrote: $targetExe"
if (-not (Test-Path -LiteralPath $targetExe -PathType Leaf)) {
  Write-Error "Validation failed: installed binary was not found at $targetExe"
  exit 1
}

$pathHasInstallDir = Add-KdsUserPath $installDir

if ($NoHook) {
  Write-Host "Skipped automatic hook installs."
  Write-Host "Install PowerShell activation later with:"
  Write-Host "  kds hook install powershell"
} else {
  & $targetExe hook install powershell
  if ($LASTEXITCODE -ne 0) {
    throw "kds hook install powershell failed with exit code $LASTEXITCODE"
  }
  $desktopHookCount = Install-KdsCodexDesktopHooks $false
}

Write-Host "Verification:"
Write-Host "  Binary present: yes"
$versionOutput = & $targetExe --version
if ($LASTEXITCODE -ne 0) {
  throw "kds --version failed with exit code $LASTEXITCODE"
}
Write-Host "  Version: $versionOutput"
if ($NoHook) {
  Write-Host "  PowerShell hook: skipped"
  Write-Host "  Codex Desktop hooks updated: skipped"
} else {
  $hookStatus = & $targetExe hook status
  if ($LASTEXITCODE -ne 0) {
    throw "kds hook status failed with exit code $LASTEXITCODE"
  }
  $hookInstalled = ($hookStatus -join "`n") -match "Installed:\s+true"
  Write-Host "  PowerShell hook installed: $hookInstalled"
  if (-not $hookInstalled) {
    Write-Error "Validation failed: PowerShell hook was not reported as installed"
    exit 1
  }
  Write-Host "  Codex Desktop hooks updated: $desktopHookCount"
}
Write-Host "  Install directory on user PATH: $pathHasInstallDir"
Write-Host "Next checks:"
Write-Host "  kds gain"
Write-Host "  kds doctor"
