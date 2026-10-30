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
# Single-repo CI mode: -Repo <name> -RepoPath <caller checkout>. Scans only that repo's
# channel(s) against the caller's working tree (repos.json absolute paths are a local
# convenience and are overridden in this mode). Part 2 runs for -Repo evorule, part 3
# for -Repo evo-agent only; the full-ecosystem pass stays a local release discipline.

[CmdletBinding()]
param(
    [switch]$Strict,
    [string]$Repo = "",
    [string]$RepoPath = "."
)

$ErrorActionPreference = "Stop"
$evoruleRoot = Split-Path -Parent $PSScriptRoot
$reposMap = Get-Content "$evoruleRoot/scripts/license-audit/repos.json" -Raw | ConvertFrom-Json

# Distribution-channel classification per repo (2026-10-24 ecosystem license audit).
#   binary : ships archives/installers  -> full 5-piece notice set required in release.yml
#   crate  : crates.io publish only     -> LICENSE piece in release.yml (package auto-includes)
#   npm    : npm/JS distribution        -> CI publish workflow (sdk-nodejs-release.yml
#                                            must run `npm publish`) + part 3 file checks
#   none   : no external distribution artifacts -> informational only
#   evo-agent-desktop override: the installer is packaged in the evo-agent repo
#   (desktop-release.yml + desktop\build-installer.ps1), so the scan surface moves there.
$distClass = @{
    "evorule"                  = "binary"
    "evorule-server"           = "binary"
    "evorule-wasm-host"        = "binary"
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

# Crates published to crates.io from this repo. Mirrors the release.yml publish
# list plus evorule-eval (historically published via manual channel, not CI).
# evorule-mcu ships as a library crate; embedded board targets are not published.
$publishedCrates = @(
    "evorule-tcb", "evorule-audit-chain", "evid-verify-core", "evid-verify-wasm",
    "evorule-reactor", "evorule-mcu", "evorule-governance", "evorule-discipline",
    "evorule-cli", "evorule-eval"
)

$gapTotal = 0
Write-Host "`n=== Distribution Compliance Check ===" -ForegroundColor Cyan

# --- Single-repo CI mode: override the repo table to the caller checkout ------
if ($Repo -ne "") {
    if (-not $distClass.ContainsKey($Repo)) { Write-Error "unknown repo: $Repo"; exit 2 }
    if ($Repo -in @("evo-agent", "evo-agent-desktop")) {
        # both names resolve to the evo-agent repo checkout (desktop packaging lives there)
        $reposMap = [pscustomobject]@{ "evo-agent" = $RepoPath; "evo-agent-desktop" = $RepoPath }
    } else {
        $reposMap = [pscustomobject]@{ $Repo = $RepoPath }
    }
}

# --- Part 1: distribution channels -------------------------------------------
foreach ($prop in $reposMap.PSObject.Properties) {
    $name = $prop.Name
    if ($Repo -ne "" -and $name -ne $Repo) { continue }
    $repoPath = $prop.Value
    $class = $distClass[$name]
    if ($class -eq "none") {
        Write-Host "[INFO] $name : no external distribution channel — skipped" -ForegroundColor DarkGray
        continue
    }
    $rel = Join-Path $repoPath ".github/workflows/release.yml"
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
    if ($class -eq "npm") {
        # npm publish channel: CI workflow must exist and actually run `npm publish`
        $npmYml = Join-Path $repoPath ".github\workflows\sdk-nodejs-release.yml"
        if ((Test-Path $npmYml) -and ((Get-Content $npmYml -Raw) -match "npm publish")) {
            Write-Host "[OK]   $name : npm publish CI-tracked (sdk-nodejs-release.yml) — file checks in part 3" -ForegroundColor Green
        } elseif (Test-Path $npmYml) {
            Write-Host "[GAP]  $name : sdk-nodejs-release.yml present but does not run npm publish" -ForegroundColor Red
            $gapTotal++
        } else {
            Write-Host "[GAP]  $name : no sdk-nodejs-release.yml — npm publishing not CI-tracked" -ForegroundColor Red
            $gapTotal++
        }
        continue
    }
    # binary channels: scan release.yml; evo-agent-desktop packaging lives in the
    # evo-agent repo (build workflow + installer script carry the 5-piece set).
    $scanPaths = @()
    if (Test-Path $rel) { $scanPaths += $rel }
    if ($name -eq "evo-agent-desktop") {
        $dYml = Join-Path $reposMap.'evo-agent' ".github/workflows/desktop-release.yml"
        $dPs1 = Join-Path $reposMap.'evo-agent' "desktop/build-installer.ps1"
        if (Test-Path $dYml) { $scanPaths += $dYml } else {
            Write-Host "[GAP]  $name : desktop-release.yml missing in evo-agent repo — packaging not CI-tracked" -ForegroundColor Red
            $gapTotal++
        }
        if (Test-Path $dPs1) { $scanPaths += $dPs1 } else {
            Write-Host "[GAP]  $name : desktop\build-installer.ps1 missing in evo-agent repo" -ForegroundColor Red
            $gapTotal++
        }
    }
    if ($scanPaths.Count -eq 0) {
        Write-Host "[GAP]  $name : distribution channel ($class) has no release.yml — packaging not CI-tracked" -ForegroundColor Red
        $gapTotal++
        continue
    }
    $yml = ($scanPaths | ForEach-Object { Get-Content $_ -Raw }) -join "`n"
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
if ($Repo -eq "" -or $Repo -eq "evorule") {
# Full-ecosystem mode scans the main-repo root ($evoruleRoot); single-repo CI
# mode must scan the caller checkout (-RepoPath) — in CI $evoruleRoot resolves
# to the sparse tooling checkout where crate directories do not exist.
$crateBase = if ($Repo -ne "") { $RepoPath } else { $evoruleRoot }
foreach ($crate in $publishedCrates) {
    $crateDir = Join-Path $crateBase $crate
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
}

# --- Part 3: npm / workspace packages ------------------------------------------
Write-Host ""
if ($Repo -eq "" -or $Repo -eq "evo-agent") {
$npmChecks = @(
    @{ Name = "evo-agent sdk/nodejs (npm evo-agent-node)"; Dir = "$($reposMap.'evo-agent')/sdk/nodejs" },
    @{ Name = "evo-agent desktop workspace (npm evo-agent-desktop)"; Dir = "$($reposMap.'evo-agent')/desktop" }
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
