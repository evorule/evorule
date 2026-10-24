# SPDX-License-Identifier: AGPL-3.0-or-later
#!/usr/bin/env pwsh
# check-dist-compliance.ps1
# Static distribution-compliance scanner.
# Verifies that each repo's distribution channel carries the required notice set:
#   binary/archive channels (zip/tar/installer): LICENSE + NOTICE.md + THIRD-PARTY-NOTICES.md
#                                                + LICENSES/ + SOURCE.md  (5-piece set)
#   crates.io publish channels:                  LICENSE piece present in release.yml
#                                                (cargo publish auto-includes LICENSE/NOTICE)
#   npm channels:                                LICENSE file + license field (part 3)
# Repo table SSOT: scripts/license-audit/repos.json
# Default: report-only (always exit 0). Pass -Strict to exit 1 on any gap (CI blocking mode).

[CmdletBinding()]
param([switch]$Strict)

$ErrorActionPreference = "Stop"
$evoruleRoot = Split-Path -Parent $PSScriptRoot
$reposMap = Get-Content "$evoruleRoot\scripts\license-audit\repos.json" -Raw | ConvertFrom-Json

# Distribution-channel classification per repo (2026-10-24 ecosystem license audit).
#   binary : ships archives/installers  -> full 5-piece notice set required in release.yml
#   crate  : crates.io publish only     -> LICENSE piece in release.yml (package auto-includes)
#   npm    : npm/JS distribution        -> part 3 file checks (release.yml not required)
#   none   : no external distribution artifacts -> informational only
$distClass = @{
    "evorule"                  = "binary"
    "evorule-server"           = "binary"
    "evorule-rule"             = "binary"
    "evorule-console-cloud"    = "binary"
    "evo-agent-desktop"        = "binary"
    "evorule-bundle"           = "crate"
    "evorule-hash"             = "crate"
    "evo-agent"                = "npm"
    "evorule-system-rules"     = "none"
    "evorule-integration-tests"= "none"
    "evorule-context-inspector"= "none"
    "evorule-wasm-demo"        = "none"
    "sync-sentinel"            = "none"
}

# 5-piece notice set -> detection substring. LICENSE uses a negative lookahead so the
# "SPDX-License-Identifier" workflow header does not count as carrying a LICENSE file.
$piecesFull = [ordered]@{
    "LICENSE"                = 'LICENSE(?!-Identifier)'
    "NOTICE.md"              = "NOTICE.md"
    "THIRD-PARTY-NOTICES.md" = "THIRD-PARTY-NOTICES"
    "LICENSES/"              = "LICENSES"
    "SOURCE.md"              = "SOURCE.md"
}

# Crates published to crates.io from this repo (embedded targets excluded).
$publishedCrates = @(
    "evorule-tcb", "evorule-reactor", "evorule-governance", "evorule-cli",
    "evorule-discipline", "evorule-eval"
)

$gapTotal = 0
Write-Host "`n=== Distribution Compliance Check ===" -ForegroundColor Cyan

# --- Part 1: distribution channels -------------------------------------------
foreach ($prop in $reposMap.PSObject.Properties) {
    $name = $prop.Name
    $repoPath = $prop.Value
    $class = $distClass[$name]
    if ($class -eq "none") {
        Write-Host "[INFO] $name : no external distribution channel — skipped" -ForegroundColor DarkGray
        continue
    }
    $rel = Join-Path $repoPath ".github\workflows\release.yml"
    if ($class -eq "crate") {
        # crates.io publish: cargo auto-includes LICENSE/NOTICE from the crate dir,
        # so the file-level predicate is LICENSE presence at the crate root.
        if (Test-Path (Join-Path $repoPath "LICENSE")) {
            Write-Host "[OK]   $name : crate channel — LICENSE at repo root (cargo publish auto-includes)" -ForegroundColor Green
        } else {
            Write-Host "[GAP]  $name : crate channel — LICENSE missing at repo root" -ForegroundColor Red
            $gapTotal++
        }
        continue
    }
    if (-not (Test-Path $rel)) {
        if ($class -eq "npm") {
            Write-Host "[INFO] $name : no release.yml — npm publishing is manual, file-level checks in part 3" -ForegroundColor DarkGray
        } else {
            Write-Host "[GAP]  $name : distribution channel ($class) has no release.yml — packaging not CI-tracked" -ForegroundColor Red
            $gapTotal++
        }
        continue
    }
    $yml = Get-Content $rel -Raw
    $missing = @()
    foreach ($piece in $piecesFull.Keys) {
        if ($yml -notmatch $piecesFull[$piece]) { $missing += $piece }
    }
    if ($missing.Count -eq 0) {
        Write-Host "[OK]   $name : release packaging carries required notice set ($class channel)" -ForegroundColor Green
    } else {
        Write-Host "[GAP]  $name : release.yml missing -> $($missing -join ', ')" -ForegroundColor Red
        $gapTotal += $missing.Count
    }
}

# --- Part 2: published crate directories (this repo) --------------------------
Write-Host ""
foreach ($crate in $publishedCrates) {
    $crateDir = Join-Path $evoruleRoot $crate
    if (-not (Test-Path $crateDir)) {
        Write-Host "[GAP] crate $crate : directory not found" -ForegroundColor Red
        $gapTotal++
        continue
    }
    if (Test-Path (Join-Path $crateDir "LICENSE")) {
        Write-Host "[OK]  crate $crate : LICENSE present" -ForegroundColor Green
    } else {
        Write-Host "[GAP] crate $crate : LICENSE missing (cargo package would ship without license text)" -ForegroundColor Red
        $gapTotal++
    }
}

# --- Part 3: npm / workspace packages ------------------------------------------
Write-Host ""
$npmChecks = @(
    @{ Name = "evo-agent sdk/nodejs (npm evo-agent-node)"; Dir = "$($reposMap.'evo-agent')\sdk\nodejs" },
    @{ Name = "evo-agent desktop workspace (npm evo-agent-desktop)"; Dir = "$($reposMap.'evo-agent')\desktop" }
)
foreach ($c in $npmChecks) {
    $pkg = Join-Path $c.Dir "package.json"
    $lic = Join-Path $c.Dir "LICENSE"
    $problems = @()
    if (-not (Test-Path $lic)) { $problems += "LICENSE file missing" }
    if (Test-Path $pkg) {
        $json = Get-Content $pkg -Raw | ConvertFrom-Json
        if (-not $json.license) { $problems += "package.json license field missing" }
    } else {
        $problems += "package.json missing"
    }
    if ($problems.Count -eq 0) {
        Write-Host "[OK]  npm $($c.Name) : LICENSE + license field present" -ForegroundColor Green
    } else {
        Write-Host "[GAP] npm $($c.Name) : $($problems -join '; ')" -ForegroundColor Red
        $gapTotal += $problems.Count
    }
}

# --- Summary --------------------------------------------------------------------
Write-Host ""
if ($gapTotal -gt 0) {
    Write-Host "[RESULT] $gapTotal gap(s) found" -ForegroundColor $(if ($Strict) { "Red" } else { "Yellow" })
    if ($Strict) { exit 1 }
    exit 0
}
Write-Host "[RESULT] PASSED — distribution compliance complete" -ForegroundColor Green
exit 0
