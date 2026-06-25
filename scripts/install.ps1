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

if ($Help) {
  @"
KDS Windows installer

Usage:
  ./scripts/install.ps1 [--dry-run] [--no-hook] [--help]

Behavior:
  - builds KDS from this repository
  - installs kds.exe to %LOCALAPPDATA%\CodexKD\bin
  - installs the automatic PowerShell hook by default unless --no-hook is set
  - does not silently edit PATH
  - does not modify Codex config
"@ | Write-Host
  exit 0
}

$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
$installDir = Join-Path $env:LOCALAPPDATA "CodexKD\bin"
$targetExe = Join-Path $installDir "kds.exe"
$builtExe = Join-Path $repo "target\release\kds.exe"

Write-Host "KDS install plan"
Write-Host "Repository: $repo"
Write-Host "Install directory: $installDir"
Write-Host "Binary: $targetExe"
if ($NoHook) {
  Write-Host "Automatic hook: skipped by --no-hook"
} else {
  Write-Host "Automatic hook: PowerShell profile managed by kds hook install powershell"
}

if ($DryRun) {
  Write-Host "Dry run: no binary copy, no hook/profile edit, no Codex config edit, no PATH edit."
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

if ($NoHook) {
  Write-Host "Skipped automatic PowerShell hook install."
  Write-Host "Install it later with:"
  Write-Host "  kds hook install powershell"
} else {
  & $targetExe hook install powershell
  if ($LASTEXITCODE -ne 0) {
    throw "kds hook install powershell failed with exit code $LASTEXITCODE"
  }
}

$pathEntries = [Environment]::GetEnvironmentVariable("Path", "User") -split ';'
$pathHasInstallDir = $pathEntries -contains $installDir
if (-not $pathHasInstallDir) {
  Write-Host "PATH note: $installDir is not in your user PATH."
  Write-Host "Add it manually if `kds` is not found in new terminals:"
  Write-Host "[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$installDir', 'User')"
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
}
Write-Host "  Install directory on user PATH: $pathHasInstallDir"
Write-Host "Next checks:"
Write-Host "  kds gain"
Write-Host "  kds doctor"
