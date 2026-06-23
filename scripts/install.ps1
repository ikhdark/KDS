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
} finally {
  Pop-Location
}

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -Force -Path $builtExe -Destination $targetExe
Write-Host "Wrote: $targetExe"

if ($NoHook) {
  Write-Host "Skipped automatic PowerShell hook install."
  Write-Host "Install it later with:"
  Write-Host "  kds hook install powershell"
} else {
  & $targetExe hook install powershell
}

$pathEntries = [Environment]::GetEnvironmentVariable("Path", "User") -split ';'
if ($pathEntries -notcontains $installDir) {
  Write-Host "PATH note: $installDir is not in your user PATH."
  Write-Host "Add it manually if `kds` is not found in new terminals:"
  Write-Host "[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$installDir', 'User')"
}

Write-Host "Verification:"
Write-Host "  kds --version"
Write-Host "  kds gain"
Write-Host "  kds doctor"
& $targetExe --version
