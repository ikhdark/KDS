#!/usr/bin/env pwsh

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$DryRun = $false
$Help = $false
$Version = "v0.1.0"

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

$archiveUrl = "https://github.com/ikhdark/KDS/releases/download/$Version/KDS-$Version-source.zip"
$checksumUrl = "$archiveUrl.sha256"
$latestReleaseApi = "https://api.github.com/repos/ikhdark/KDS/releases/latest"
$releasesUrl = "https://github.com/ikhdark/KDS/releases"

function Get-KdsFileSha256 {
  param([string]$Path)
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Read-KdsExpectedSha256 {
  param([string]$Path)
  $text = Get-Content -LiteralPath $Path -Raw
  $match = [regex]::Match($text, '(?i)\b[0-9a-f]{64}\b')
  if (-not $match.Success) {
    throw "Checksum file did not contain a SHA-256 digest"
  }
  return $match.Value.ToLowerInvariant()
}

function Get-KdsInstalledVersion {
  $installed = Get-Command kds.exe,kds -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $installed) {
    return "not installed"
  }
  try {
    $output = & $installed.Source --version 2>$null
    if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($output)) {
      return [string]$output
    }
  } catch {
  }
  return "installed, version unavailable"
}

function Get-KdsLatestReleaseVersion {
  try {
    $headers = @{
      "User-Agent" = "KDS-bootstrap/$Version"
      "Accept" = "application/vnd.github+json"
    }
    $release = Invoke-RestMethod -Uri $latestReleaseApi -Headers $headers -UseBasicParsing
    if ($release -and -not [string]::IsNullOrWhiteSpace([string]$release.tag_name)) {
      return [string]$release.tag_name
    }
  } catch {
    Write-Warning "Update check failed: $($_.Exception.Message)"
  }
  return "unknown"
}

if ($Help) {
  @"
KDS bootstrap installer

Copy-paste install:
  irm https://raw.githubusercontent.com/ikhdark/KDS/$Version/scripts/bootstrap.ps1 | iex

Behavior:
  - prints installed and latest release versions before installing
  - downloads the versioned KDS release source archive
  - verifies the archive SHA-256 checksum from the matching release asset
  - requires Rust/Cargo to already be available on PATH
  - never downloads or installs Rust/Cargo
  - builds KDS with cargo
  - runs scripts/install.ps1 from the downloaded source
"@ | Write-Host
  exit 0
}

Write-Host "KDS bootstrap install"
Write-Host "Version: $Version"
Write-Host "Source archive: $archiveUrl"
Write-Host "Checksum: $checksumUrl"
Write-Host "Releases: $releasesUrl"

if ($DryRun) {
  Write-Host "Update check: skipped in dry run"
  Write-Host "Dry run: no download, no checksum verification, no extraction, no Rust/Cargo install, no build, no install."
  exit 0
}

$installedVersion = Get-KdsInstalledVersion
$latestVersion = Get-KdsLatestReleaseVersion
Write-Host "Installed version: $installedVersion"
Write-Host "Latest release: $latestVersion"
if ($latestVersion -ne "unknown" -and $latestVersion -ne $Version) {
  Write-Host "Bootstrap target: $Version"
  Write-Host "A different latest release is available. To install it, use the bootstrap URL for $latestVersion from $releasesUrl."
}

$cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $cargo) {
  Write-Error "Cargo was not found on PATH. KDS does not download or install Rust/Cargo. Install Rust/Cargo separately, then rerun the KDS install command."
  exit 1
}

$workRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("kds-install-" + [System.Guid]::NewGuid().ToString("N"))
$zipPath = Join-Path $workRoot "kds-source.zip"
$checksumPath = Join-Path $workRoot "kds-source.zip.sha256"

New-Item -ItemType Directory -Force -Path $workRoot | Out-Null

try {
  Write-Host "Downloading KDS source checksum..."
  Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath -UseBasicParsing
  $expectedHash = Read-KdsExpectedSha256 $checksumPath

  Write-Host "Downloading KDS source..."
  Invoke-WebRequest -Uri $archiveUrl -OutFile $zipPath -UseBasicParsing
  $actualHash = Get-KdsFileSha256 $zipPath
  if ($actualHash -ne $expectedHash) {
    throw "KDS source checksum mismatch. Expected $expectedHash but downloaded $actualHash"
  }
  Write-Host "Verified KDS source checksum: sha256:$actualHash"

  Write-Host "Extracting KDS source..."
  Expand-Archive -LiteralPath $zipPath -DestinationPath $workRoot -Force
  $sourceRoot = @((Get-Item -LiteralPath $workRoot)) + @(Get-ChildItem -LiteralPath $workRoot -Directory) |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "scripts\install.ps1") -PathType Leaf } |
    Select-Object -First 1
  if (-not $sourceRoot) {
    throw "Downloaded archive did not contain scripts\install.ps1"
  }

  $installer = Join-Path $sourceRoot.FullName "scripts\install.ps1"

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
