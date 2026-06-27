param(
    [int]$Iterations = 3,
    [int]$SuccessMegabytes = 10,
    [int]$StateRuns = 50,
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')
$KdsExe = Join-Path $RepoRoot 'target\debug\kds.exe'

if (-not $SkipBuild) {
    cargo build
    if ($LASTEXITCODE -ne 0) {
        throw 'cargo build failed'
    }
}

if (-not (Test-Path -LiteralPath $KdsExe)) {
    throw "missing debug binary: $KdsExe"
}

function New-TempDir {
    $path = Join-Path ([System.IO.Path]::GetTempPath()) ("kds-bench-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $path | Out-Null
    return $path
}

function ConvertTo-EncodedCommand {
    param([string]$Script)
    $bytes = [System.Text.Encoding]::Unicode.GetBytes($Script)
    return [Convert]::ToBase64String($bytes)
}

function Invoke-WithEnv {
    param(
        [hashtable]$Env,
        [scriptblock]$Body
    )
    $old = @{}
    foreach ($key in $Env.Keys) {
        $old[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
        [Environment]::SetEnvironmentVariable($key, [string]$Env[$key], 'Process')
    }
    try {
        & $Body
    }
    finally {
        foreach ($key in $Env.Keys) {
            [Environment]::SetEnvironmentVariable($key, $old[$key], 'Process')
        }
    }
}

function Measure-Case {
    param(
        [string]$Name,
        [scriptblock]$Body
    )
    $runs = New-Object System.Collections.Generic.List[double]
    for ($i = 0; $i -lt $Iterations; $i++) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        & $Body
        $sw.Stop()
        [void]$runs.Add($sw.Elapsed.TotalMilliseconds)
    }
    $avg = ($runs | Measure-Object -Average).Average
    $min = ($runs | Measure-Object -Minimum).Minimum
    $max = ($runs | Measure-Object -Maximum).Maximum
    [pscustomobject]@{
        Case = $Name
        Iterations = $Iterations
        AverageMs = [Math]::Round($avg, 1)
        MinMs = [Math]::Round($min, 1)
        MaxMs = [Math]::Round($max, 1)
    }
}

function Invoke-KdsGeneratedLog {
    param(
        [string[]]$KdsArgs,
        [string]$Script
    )
    $encoded = ConvertTo-EncodedCommand $Script
    $cmd = @($KdsExe) + $KdsArgs + @('--', 'pwsh', '-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded)
    $exe = $cmd[0]
    $args = $cmd[1..($cmd.Count - 1)]
    & $exe @args *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "benchmark command failed: $($cmd -join ' ')"
    }
}

$TempRoot = New-TempDir
$SuccessHome = Join-Path $TempRoot 'success-home'
$RawHome = Join-Path $TempRoot 'raw-home'
$StateHome = Join-Path $TempRoot 'state-home'
$HookProfile = Join-Path $TempRoot 'profile.ps1'
$FakeBin = Join-Path $TempRoot 'bin'
New-Item -ItemType Directory -Path $FakeBin | Out-Null

$line = 'ordinary successful output line without paths secrets or ansi markers 0123456789'
$successLineCount = [Math]::Ceiling(($SuccessMegabytes * 1MB) / ($line.Length + 2))
$successScript = @"
`$line = '$line'
for (`$i = 0; `$i -lt $successLineCount; `$i++) {
    [Console]::Out.WriteLine(`$line)
}
"@

$errorScript = @"
for (`$i = 0; `$i -lt 2000; `$i++) {
    [Console]::Error.WriteLine("src/main.rs:{0}:1: error: synthetic failure {0}" -f `$i)
}
exit 1
"@

$results = New-Object System.Collections.Generic.List[object]

[void]$results.Add((Measure-Case 'compact-success-log' {
    Invoke-WithEnv @{ KDS_HOME = $SuccessHome } {
        Invoke-KdsGeneratedLog @('run') $successScript
    }
}))

[void]$results.Add((Measure-Case 'compact-error-heavy-log' {
    Invoke-WithEnv @{ KDS_HOME = $SuccessHome } {
        $encoded = ConvertTo-EncodedCommand $errorScript
        & $KdsExe run -- pwsh -NoProfile -NonInteractive -EncodedCommand $encoded *> $null
        if ($LASTEXITCODE -eq 0) {
            throw 'expected error-heavy benchmark command to fail'
        }
    }
}))

[void]$results.Add((Measure-Case 'raw-tee-success-log' {
    Invoke-WithEnv @{ KDS_HOME = $RawHome } {
        Invoke-KdsGeneratedLog @('raw') $successScript
    }
}))

Invoke-WithEnv @{ KDS_HOME = $StateHome } {
    for ($i = 0; $i -lt $StateRuns; $i++) {
        & $KdsExe run --save-artifacts -- pwsh -NoProfile -NonInteractive -Command "Write-Output state-prime-$i" *> $null
        if ($LASTEXITCODE -ne 0) {
            throw "state priming failed at run $i"
        }
    }
}

[void]$results.Add((Measure-Case 'saved-artifact-existing-state' {
    Invoke-WithEnv @{ KDS_HOME = $StateHome } {
        & $KdsExe run --save-artifacts -- pwsh -NoProfile -NonInteractive -Command 'Write-Output measured-state-run' *> $null
        if ($LASTEXITCODE -ne 0) {
            throw 'saved-artifact benchmark failed'
        }
    }
}))

Set-Content -LiteralPath (Join-Path $FakeBin 'cargo.cmd') -Value "@echo off`r`necho cargo 1.0.0`r`n"
Invoke-WithEnv @{ KDS_POWERSHELL_PROFILE = $HookProfile } {
    & $KdsExe hook install powershell *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'hook install failed'
    }
}

$hookScript = @"
`$env:PATH = '$FakeBin;' + `$env:PATH
. '$HookProfile'
for (`$i = 0; `$i -lt 100; `$i++) {
    cargo --version > `$null
}
"@

[void]$results.Add((Measure-Case 'powershell-hook-passthrough' {
    $encoded = ConvertTo-EncodedCommand $hookScript
    & pwsh -NoProfile -NonInteractive -EncodedCommand $encoded *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'hook passthrough benchmark failed'
    }
}))

$results | Format-Table -AutoSize
[Console]::Out.WriteLine("Temp benchmark state: $TempRoot")
