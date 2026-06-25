#!/usr/bin/env pwsh

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$DryRun = $false
$Help = $false

foreach ($arg in $args) {
  switch ($arg) {
    "--dry-run" { $DryRun = $true }
    "-DryRun" { $DryRun = $true }
    "--help" { $Help = $true }
    "-h" { $Help = $true }
    "-Help" { $Help = $true }
    default {
      Write-Error "Unknown argument: $arg"
      exit 2
    }
  }
}

$archiveUrl = "https://github.com/ikhdark/KDS/archive/refs/heads/main.zip"

if ($Help) {
  @"
KDS bootstrap installer

Copy-paste install:
  irm https://raw.githubusercontent.com/ikhdark/KDS/main/scripts/bootstrap.ps1 | iex

Behavior:
  - downloads the KDS source archive
  - builds KDS with cargo
  - runs scripts/install.ps1 from the downloaded source
"@ | Write-Host
  exit 0
}

Write-Host "KDS bootstrap install"
Write-Host "Source archive: $archiveUrl"

if ($DryRun) {
  Write-Host "Dry run: no download, no extraction, no build, no install."
  exit 0
}

$cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $cargo) {
  Write-Error "Cargo was not found on PATH. Install Rust/Cargo, then rerun the KDS install command."
  exit 1
}

$workRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("kds-install-" + [System.Guid]::NewGuid().ToString("N"))
$zipPath = Join-Path $workRoot "kds-source.zip"

New-Item -ItemType Directory -Force -Path $workRoot | Out-Null

try {
  Write-Host "Downloading KDS source..."
  Invoke-WebRequest -Uri $archiveUrl -OutFile $zipPath -UseBasicParsing

  Write-Host "Extracting KDS source..."
  Expand-Archive -LiteralPath $zipPath -DestinationPath $workRoot -Force
  $sourceRoot = Get-ChildItem -LiteralPath $workRoot -Directory |
    Where-Object { $_.Name -like "KDS-*" } |
    Select-Object -First 1
  if (-not $sourceRoot) {
    throw "Downloaded archive did not contain a KDS source directory"
  }

  $installer = Join-Path $sourceRoot.FullName "scripts\install.ps1"
  if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    throw "Downloaded archive did not contain scripts\install.ps1"
  }

  Write-Host "Running KDS installer..."
  & $installer
  if ($LASTEXITCODE -ne 0) {
    throw "KDS installer failed with exit code $LASTEXITCODE"
  }
} finally {
  $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
  $resolvedWorkRoot = [System.IO.Path]::GetFullPath($workRoot)
  if ($resolvedWorkRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase) -and
      (Test-Path -LiteralPath $resolvedWorkRoot)) {
    Remove-Item -LiteralPath $resolvedWorkRoot -Recurse -Force
  }
}
