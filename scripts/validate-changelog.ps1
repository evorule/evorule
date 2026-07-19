#!/usr/bin/env pwsh
# validate-changelog.ps1
# VERSION_STRATEGY.md 4.5
# Check: each project CHANGELOG has section for current version + release-mode has no [Unreleased]
# Exit code: 0 = pass, 1 = fail

[CmdletBinding()]
param(
    [switch]$AllowUnreleased
)

$ErrorActionPreference = "Stop"
$evoruleRoot = Split-Path -Parent $PSScriptRoot
$evoAgentRoot = "D:\evo-agent"

# Build projects table (no quoted keys for PowerShell 5.1 safety)
$projects = [ordered]@{}
$projects['evorule']        = @{ Version = "$evoruleRoot\Cargo.toml";                   Changelog = "$evoruleRoot\CHANGELOG.md" }
$projects['evo-agent']      = @{ Version = "$evoAgentRoot\Cargo.toml";                  Changelog = "$evoAgentRoot\CHANGELOG.md" }
$projects['sdk-typescript'] = @{ Version = "$evoruleRoot\sdk\typescript\package.json";  Changelog = "$evoruleRoot\sdk\typescript\CHANGELOG.md" }
$projects['sdk-python']     = @{ Version = "$evoruleRoot\sdk\python\pyproject.toml";    Changelog = "$evoruleRoot\sdk\python\CHANGELOG.md" }

function Read-Version {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return $null }
    $content = Get-Content $Path -Raw
    # Multiline-aware: find first version = "..." or "version": "..." on any line
    if ($content -match '(?m)^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
    if ($content -match '(?m)"version"\s*:\s*"([^"]+)"') { return $Matches[1] }
    return $null
}

$failed = $false
Write-Host "`n=== Changelog Validation ===" -ForegroundColor Cyan

foreach ($name in $projects.Keys) {
    $p = $projects[$name]
    $version = Read-Version $p.Version
    if (-not $version) {
        Write-Host "[SKIP] $name : version not found at $($p.Version)" -ForegroundColor Yellow
        continue
    }
    if (-not (Test-Path $p.Changelog)) {
        Write-Host "[FAIL] $name : CHANGELOG not found at $($p.Changelog)" -ForegroundColor Red
        $failed = $true
        continue
    }

    $content = Get-Content $p.Changelog -Raw

    # 4.5: every version has its own section
    $sectionPattern = "##\s*\[$([regex]::Escape($version))\]"
    if ($content -match $sectionPattern) {
        Write-Host "[OK]   $name : CHANGELOG has '## [$version]'" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $name : CHANGELOG missing '## [$version]'" -ForegroundColor Red
        $failed = $true
    }

    # 4.5: release-mode forbids [Unreleased]
    if (-not $AllowUnreleased) {
        if ($content -match '##\s*\[Unreleased\]') {
            Write-Host "[FAIL] $name : CHANGELOG has [Unreleased] (release-mode forbidden)" -ForegroundColor Red
            $failed = $true
        }
    }
}

if ($failed) { Write-Host "`n[RESULT] FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
