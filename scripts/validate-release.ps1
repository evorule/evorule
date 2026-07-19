#!/usr/bin/env pwsh
# validate-release.ps1
# VERSION_STRATEGY.md 4.5, 10.1
# Release-time check: git tag matches + pre-release identifier + tag format
# Exit code: 0 = pass, 1 = fail

[CmdletBinding()]
param(
    [switch]$SkipTagCheck
)

$ErrorActionPreference = "Stop"
$evoruleRoot = Split-Path -Parent $PSScriptRoot

$semverPattern = '^(\d+)\.(\d+)\.(\d+)(?:-([a-z]+)\.(\d+))?$'
$tagPattern = '^v\d+\.\d+\.\d+(-[a-z]+\.\d+)?$'

$failed = $false
Write-Host "`n=== Release Validation ===" -ForegroundColor Cyan

Push-Location $evoruleRoot
try {
    # 1. Current version
    $currentVersion = (Get-Content "Cargo.toml" | Where-Object { $_ -match '^version\s*=' } | Select-Object -First 1) -replace '.*"([^"]+)".*', '$1'
    Write-Host "[INFO] Current version: $currentVersion" -ForegroundColor Cyan

    if ($currentVersion -notmatch $semverPattern) {
        Write-Host "[FAIL] Current version '$currentVersion' is not valid SemVer" -ForegroundColor Red
        exit 1
    }
    $preRelease = $Matches[4]
    if ($preRelease) {
        Write-Host "[INFO] Pre-release identifier: $preRelease" -ForegroundColor Cyan
    }

    # 2. git tag match
    if (-not $SkipTagCheck) {
        $tagName = "v$currentVersion"
        $tagExists = git tag --list $tagName
        if ($tagExists) {
            Write-Host "[OK]   git tag '$tagName' exists" -ForegroundColor Green
        } else {
            Write-Host "[WARN] git tag '$tagName' does not exist (use 'git tag $tagName' to create)" -ForegroundColor Yellow
        }
    }

    # 3. All tags format check
    $allTags = git tag --list
    if ($allTags.Count -eq 0) {
        Write-Host "[INFO] No git tags yet" -ForegroundColor Cyan
    } else {
        $badTags = @($allTags | Where-Object { $_ -notmatch $tagPattern })
        if ($badTags.Count -gt 0) {
            Write-Host "[FAIL] Bad tag format: $($badTags -join ', ')" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   All $($allTags.Count) tags have 'vX.Y.Z' format" -ForegroundColor Green
        }
    }

    # 4. Status
    if (-not $preRelease) {
        Write-Host "[INFO] Stable version, ready for release" -ForegroundColor Cyan
    } else {
        Write-Host "[INFO] Pre-release, do NOT publish to crates.io/npm/PyPI until stable" -ForegroundColor Yellow
    }
} finally {
    Pop-Location
}

if ($failed) { Write-Host "`n[RESULT] FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
