#!/usr/bin/env pwsh
# validate-version.ps1
# VERSION_STRATEGY.md 2.1, 3.2
# Check: SemVer format + cross-project MAJOR sync + mechanism MINOR <= dependent MINOR
# Exit code: 0 = pass, 1 = fail

[CmdletBinding()]
param(
    [switch]$Quiet
)

$ErrorActionPreference = "Stop"

# Project root: script lives in D:\evorule\scripts\, go up 1 level
$evoruleRoot = Split-Path -Parent $PSScriptRoot
$evoAgentRoot = "D:\evo-agent"

# SemVer 2.0: MAJOR.MINOR.PATCH(-pre.N)?
$semverPattern = '^(\d+)\.(\d+)\.(\d+)(?:-([a-z]+)\.(\d+))?$'

function Get-TomlVersion {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return $null }
    $line = Get-Content $Path | Where-Object { $_ -match '^\s*version\s*=\s*"' } | Select-Object -First 1
    if ($line -and $line -match '"([^"]+)"') { return $Matches[1] }
    return $null
}

function Get-JsonVersion {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return $null }
    $line = Get-Content $Path | Where-Object { $_ -match '"version"\s*:' } | Select-Object -First 1
    if ($line -and $line -match '"version"\s*:\s*"([^"]+)"') { return $Matches[1] }
    return $null
}

# Build project table without quoted keys (PowerShell 5.1 parsing safety)
$projects = [ordered]@{}
$projects['evorule-workspace'] = @{ Get = { Get-TomlVersion "$evoruleRoot\Cargo.toml" } }
$projects['tier0-tcb']         = @{ Get = { Get-TomlVersion "$evoruleRoot\tier0-tcb\Cargo.toml" } }
$projects['tier1-reactor']     = @{ Get = { Get-TomlVersion "$evoruleRoot\tier1-reactor\Cargo.toml" } }
$projects['tier2-governance']  = @{ Get = { Get-TomlVersion "$evoruleRoot\tier2-governance\Cargo.toml" } }
$projects['evo-agent']         = @{ Get = { Get-TomlVersion "$evoAgentRoot\Cargo.toml" } }
$projects['sdk-typescript']    = @{ Get = { Get-JsonVersion "$evoruleRoot\sdk\typescript\package.json" } }
$projects['sdk-python']        = @{ Get = { Get-TomlVersion "$evoruleRoot\sdk\python\pyproject.toml" } }

# 1. Extract + SemVer check
$versions = [ordered]@{}
$failed = $false

if (-not $Quiet) { Write-Host "`n=== Version Validation ===" -ForegroundColor Cyan }

foreach ($name in $projects.Keys) {
    $v = & $projects[$name].Get
    $versions[$name] = $v
    if ($null -eq $v) {
        Write-Host "[SKIP] $name : version not found" -ForegroundColor Yellow
        continue
    }
    if ($v -notmatch $semverPattern) {
        Write-Host "[FAIL] $name : '$v' is not valid SemVer 2.0" -ForegroundColor Red
        $failed = $true
    } else {
        Write-Host "[OK]   $name : $v" -ForegroundColor Green
    }
}

# 2. MAJOR consistency (3.2)
$parsedMajors = @()
foreach ($name in $versions.Keys) {
    $v = $versions[$name]
    if ($v -and $v -match $semverPattern) {
        $parsedMajors += [int]$Matches[1]
    }
}
$uniqueMajors = $parsedMajors | Sort-Object -Unique
if ($uniqueMajors.Count -gt 1) {
    Write-Host "`n[FAIL] MAJOR mismatch: $($uniqueMajors -join ', ')" -ForegroundColor Red
    $failed = $true
} elseif ($uniqueMajors.Count -eq 1) {
    Write-Host "`n[OK]   All projects share MAJOR = $($uniqueMajors[0])" -ForegroundColor Green
}

# 3. Mechanism MINOR <= application / SDK (3.2)
$mechMinor = $null
if ($versions['evorule-workspace'] -and $versions['evorule-workspace'] -match $semverPattern) {
    $mechMinor = [int]$Matches[2]
}
if ($null -ne $mechMinor) {
    Write-Host ""
    foreach ($name in @('evo-agent', 'sdk-typescript', 'sdk-python')) {
        $v = $versions[$name]
        if ($v -and $v -match $semverPattern) {
            $minor = [int]$Matches[2]
            if ($minor -lt $mechMinor) {
                Write-Host "[FAIL] $name MINOR ($minor) < evorule MINOR ($mechMinor)" -ForegroundColor Red
                $failed = $true
            } else {
                Write-Host "[OK]   $name MINOR ($minor) >= evorule MINOR ($mechMinor)" -ForegroundColor Green
            }
        }
    }
}

if ($failed) {
    Write-Host "`n[RESULT] FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
