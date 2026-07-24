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

# 4. Document version consistency (VERSION_STRATEGY §10)
# Cargo.toml is the "source of truth"; docs must not reference retired versions.
if (-not $Quiet) { Write-Host "`n=== Document Version Consistency ===" -ForegroundColor Cyan }

$canonicalVersion = $versions['evorule-workspace']  # e.g. "0.1.0"

# 4a. README.md bibtex version = {X} must match canonical
$readmePath = "$evoruleRoot\README.md"
if (Test-Path $readmePath) {
    $readmeContent = Get-Content $readmePath -Raw -Encoding UTF8
    if ($readmeContent -match 'version\s*=\s*\{([^}]+)\}') {
        $bibtexVersion = $Matches[1].Trim()
        if ($bibtexVersion -notlike "$canonicalVersion*") {
            Write-Host "[FAIL] README.md bibtex version '$bibtexVersion' != Cargo.toml '$canonicalVersion'" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   README.md bibtex version = $bibtexVersion" -ForegroundColor Green
        }
    }
}

# 4b. CONTRIBUTING.md Version: X must match canonical
$contribPath = "$evoruleRoot\CONTRIBUTING.md"
if (Test-Path $contribPath) {
    $contribContent = Get-Content $contribPath -Raw -Encoding UTF8
    if ($contribContent -match '\*\*Version\*\*:\s*(.+)') {
        $contribVersion = $Matches[1].Trim()
        if ($contribVersion -notlike "$canonicalVersion*") {
            Write-Host "[FAIL] CONTRIBUTING.md Version '$contribVersion' != Cargo.toml '$canonicalVersion'" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   CONTRIBUTING.md Version = $contribVersion" -ForegroundColor Green
        }
    }
}

# 4c. CHANGELOG.md first entry version must be >= canonical
# The first "## [...] - X.Y.Z" line in CHANGELOG is the latest/unreleased version
# For initial release, first entry == canonical is OK; otherwise first entry > canonical
$changelogPath = "$evoruleRoot\CHANGELOG.md"
if (Test-Path $changelogPath) {
    $changelogContent = [System.IO.File]::ReadAllText($changelogPath, [System.Text.Encoding]::UTF8)
    # Match first "## [something] - version" heading (avoids Chinese encoding issues)
    if ($changelogContent -match '##\s*\[[^\]]+\]\s*-\s*(\d+\.\d+\.\d+(?:-[a-z]+\.\d+)?)') {
        $unreleasedVersion = $Matches[1].Trim()
        if ($unreleasedVersion -match '^(\d+)\.(\d+)') {
            $unMajor = [int]$Matches[1]
            $unMinor = [int]$Matches[2]
            if ($canonicalVersion -match '^(\d+)\.(\d+)') {
                $canMajor = [int]$Matches[1]
                $canMinor = [int]$Matches[2]
                if ($unMajor -lt $canMajor -or ($unMajor -eq $canMajor -and $unMinor -lt $canMinor)) {
                    Write-Host "[FAIL] CHANGELOG first entry '$unreleasedVersion' < current '$canonicalVersion'" -ForegroundColor Red
                    $failed = $true
                } elseif ($unMajor -eq $canMajor -and $unMinor -eq $canMinor) {
                    Write-Host "[OK]   CHANGELOG first entry = $unreleasedVersion (initial release)" -ForegroundColor Green
                } else {
                    Write-Host "[OK]   CHANGELOG first entry = $unreleasedVersion (> $canonicalVersion)" -ForegroundColor Green
                }
            }
        }
    }
}

# 4d. Retired version scan — no v6.x / v7.0 in non-historical docs
# CHANGELOG historical sections (## [6.0.0] etc.) are exempt
$retiredPatterns = @('v6\.0', 'v6\.1', 'v6\.2', 'v7\.0', '6\.0\.0')
$docFiles = @(
    @{ Path = "$evoruleRoot\README.md"; Name = 'README.md' }
    @{ Path = "$evoruleRoot\CONTRIBUTING.md"; Name = 'CONTRIBUTING.md' }
    @{ Path = "$evoruleRoot\ROADMAP.md"; Name = 'ROADMAP.md' }
    @{ Path = "$evoruleRoot\VERSION_STRATEGY.md"; Name = 'VERSION_STRATEGY.md' }
)
$retiredFound = $false
foreach ($doc in $docFiles) {
    if (-not (Test-Path $doc.Path)) { continue }
    $lines = Get-Content $doc.Path -Encoding UTF8
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        foreach ($pattern in $retiredPatterns) {
            if ($line -match $pattern) {
                Write-Host "[FAIL] $($doc.Name):$($i+1) contains retired version pattern '$pattern': $($line.Trim())" -ForegroundColor Red
                $failed = $true
                $retiredFound = $true
            }
        }
    }
}
if (-not $retiredFound) {
    Write-Host "[OK]   No retired version references (v6.x/v7.0) in docs" -ForegroundColor Green
}

if ($failed) {
    Write-Host "`n[RESULT] FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
