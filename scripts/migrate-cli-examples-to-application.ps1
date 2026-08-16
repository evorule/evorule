# Copyright 2026 EvoRule Project
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
<#
.SYNOPSIS
  Migrates the 2 bundled industry-rule templates (hospital + law-firm, 10 files)
  under evorule-cli/examples/ out of the evorule *core* framework repository,
  into the sibling evorule-application repository.

.DESCRIPTION
  Rationale (AGENTS.md boundary rule, "evorule-cli add rule templates" row):
  - The evorule core repo is a pure mechanism-layer framework. It must not ship
    business-level domain solutions.
  - The hospital / law-firm JSON rule sets are INDUSTRY-SPECIFIC BUSINESS CONTENT
    and therefore belong in evorule-application, never in the framework core.
  - This script is the companion tool for the 2026-07-30 boundary-cleanup action
    (performed after EXPLICIT PROJECT-SIDE CONFIRMATION per AGENTS.md Rule 2).

  Recommended usage: COPY this script into evorule-application/scripts/ and run it
  from there.  Alternatively, run it directly from the evorule core repo and pass
  -AppRepo to point at the sibling application repository.

  Recovery priority:
  1) Git HEAD snapshot of the core repo (via `git show HEAD:<path>`)
  2) Local backup directory (optional -FromBackup argument) when #1 has never
     been committed.

.PARAMETER CoreRepo
  Absolute path to the local evorule core repository.  Default: the parent
  directory of the script (assumes script lives in evorule/scripts/).

.PARAMETER AppRepo
  Absolute path to the local evorule-application repository.  Default:
  a SIBLING directory of CoreRepo literally named "evorule-application".

.PARAMETER FromBackup
  (Optional) Local backup directory.  Only consulted when `git show HEAD:<path>`
  returns nothing (file never committed).  Relative paths are resolved under
  $FromBackup (e.g. $FromBackup/evorule-cli/examples/hospital/...).

.PARAMETER TargetSubdir
  (Optional) Subdirectory inside AppRepo where templates are written.
  Default: "examples/evorule-cli" (matches the convention documented in AGENTS.md).

.PARAMETER DryRun
  Only PRINT the actions (mkdir + file writes + migration metadata).
  Nothing is actually written to disk.

.PARAMETER Overwrite
  Forcefully overwrite existing destination files.  Default behaviour:
  SKIP a file when the destination exists, and print a WARN-level message.

.EXAMPLE
  # Simplest: script lives in evorule/scripts/; sibling evorule-application is
  # already cloned next to evorule.
  .\scripts\migrate-cli-examples-to-application.ps1

.EXAMPLE
  # Explicit repository paths; with a local backup fallback.
  .\scripts\migrate-cli-examples-to-application.ps1 `
      -CoreRepo D:\evorule `
      -AppRepo  D:\evorule-application `
      -FromBackup D:\backups\evorule-2026-07-29

.EXAMPLE
  # Preview only (no disk writes).
  .\scripts\migrate-cli-examples-to-application.ps1 -DryRun

.NOTES
  [Rule-2 audit trail] This migration is a boundary-cleanup action performed
  AFTER EXPLICIT PROJECT-SIDE CONFIRMATION.  Corresponding trace documents live in
  the evorule core repo:
    - CHANGELOG.md v0.1.0 section "走神 9 拆分（机制-应用边界清理）"
    - evorule-cli/README.md top banner "业务规则模板位置说明（边界合规 · 2026-07-30）"
    - evorule-cli/examples/README.md trailing "[留痕声明]" block
#>

param(
    [string]$CoreRepo    = "",
    [string]$AppRepo     = "",
    [string]$FromBackup  = "",
    [string]$TargetSubdir = "examples/evorule-cli",
    [switch]$DryRun,
    [switch]$Overwrite
)

$ErrorActionPreference = "Stop"

# ── Parameter resolution ───────────────────────────────────────────────────
if ([string]::IsNullOrEmpty($CoreRepo)) {
    $CoreRepo = Split-Path -Parent $PSScriptRoot
}
$CoreRepo = [System.IO.Path]::GetFullPath($CoreRepo)

if ([string]::IsNullOrEmpty($AppRepo)) {
    $AppRepo = Join-Path (Split-Path -Parent $CoreRepo) "evorule-application"
}
$AppRepo    = [System.IO.Path]::GetFullPath($AppRepo)
$TargetRoot = Join-Path $AppRepo $TargetSubdir

Write-Host "========================================================================"
Write-Host " evorule-cli business-rule template migration -> evorule-application"
Write-Host "========================================================================"
Write-Host (" CoreRepo     = {0}" -f $CoreRepo)
Write-Host (" AppRepo      = {0}" -f $AppRepo)
Write-Host (" TargetRoot   = {0}" -f $TargetRoot)
if (-not [string]::IsNullOrEmpty($FromBackup)) {
    Write-Host (" FromBackup   = {0}" -f [System.IO.Path]::GetFullPath($FromBackup))
}
Write-Host (" DryRun       = {0}" -f $DryRun)
Write-Host (" Overwrite    = {0}" -f $Overwrite)
Write-Host ""

# ── Pre-flight validation ──────────────────────────────────────────────────
if (-not (Test-Path (Join-Path $CoreRepo ".git"))) {
    throw ("CoreRepo is not a valid Git work tree: {0} (expected the evorule core repo root with a .git/ subdir)" -f $CoreRepo)
}
if (-not (Test-Path $AppRepo)) {
    throw ("AppRepo directory does not exist: {0}.  Clone the evorule-application repo next to evorule first." -f $AppRepo)
}

# ── 10-file migration manifest (core-relative src ; dst-relative tgt) ──────
$Manifest = @(
    @{ Src = "evorule-cli/examples/hospital/README.md"
       Dst = "hospital/README.md" }
    @{ Src = "evorule-cli/examples/hospital/payload.example.json"
       Dst = "hospital/payload.example.json" }
    @{ Src = "evorule-cli/examples/hospital/rules/01-access-audit.json"
       Dst = "hospital/rules/01-access-audit.json" }
    @{ Src = "evorule-cli/examples/hospital/rules/02-prescription-guard.json"
       Dst = "hospital/rules/02-prescription-guard.json" }
    @{ Src = "evorule-cli/examples/hospital/rules/03-privacy-redaction.json"
       Dst = "hospital/rules/03-privacy-redaction.json" }
    @{ Src = "evorule-cli/examples/law-firm/README.md"
       Dst = "law-firm/README.md" }
    @{ Src = "evorule-cli/examples/law-firm/payload.example.json"
       Dst = "law-firm/payload.example.json" }
    @{ Src = "evorule-cli/examples/law-firm/rules/01-file-access-audit.json"
       Dst = "law-firm/rules/01-file-access-audit.json" }
    @{ Src = "evorule-cli/examples/law-firm/rules/02-conflict-of-interest.json"
       Dst = "law-firm/rules/02-conflict-of-interest.json" }
    @{ Src = "evorule-cli/examples/law-firm/rules/03-deadline-tracker.json"
       Dst = "law-firm/rules/03-deadline-tracker.json" }
)

# ── Helper: restore a single file's content (returns hashtable or $null) ───
function Restore-File([string]$RelSrc) {
    # 1) Git HEAD first
    Push-Location $CoreRepo
    try {
        $lines  = & git show ("HEAD:{0}" -f $RelSrc) 2>$null
        if ($LASTEXITCODE -eq 0 -and $null -ne $lines) {
            return @{
                Source  = ("git HEAD:{0}" -f $RelSrc)
                Content = ($lines -join "`n")
            }
        }
    } finally {
        Pop-Location
    }

    # 2) Fallback backup directory
    if (-not [string]::IsNullOrEmpty($FromBackup)) {
        $BackupAbs = Join-Path ([System.IO.Path]::GetFullPath($FromBackup)) $RelSrc
        if (Test-Path $BackupAbs) {
            return @{
                Source  = ("backup {0}" -f $BackupAbs)
                Content = (Get-Content -Raw $BackupAbs)
            }
        }
    }

    return $null
}

# ── Prepare destination directories ────────────────────────────────────────
$DestDirs = @(
    (Join-Path $TargetRoot "hospital\rules"),
    (Join-Path $TargetRoot "law-firm\rules")
)
foreach ($dir in $DestDirs) {
    if ($DryRun) {
        Write-Host ("[DRY-RUN] mkdir {0}" -f $dir)
    } else {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }
}

# ── Per-file migration loop ────────────────────────────────────────────────
$Stats = @{ Ok = 0; Skipped = 0; Failed = 0 }
foreach ($Item in $Manifest) {
    $DstAbs = Join-Path $TargetRoot $Item.Dst
    $DstDir = Split-Path -Parent $DstAbs

    if ((Test-Path $DstAbs) -and -not $Overwrite) {
        Write-Host ("[SKIP ] Destination already exists: {0} (pass -Overwrite to replace)" -f $Item.Dst)
        $Stats.Skipped = $Stats.Skipped + 1
        continue
    }

    $Result = Restore-File $Item.Src
    if ($null -eq $Result) {
        Write-Host ("[FAIL ] Unable to restore {0} (absent from Git HEAD AND -FromBackup not set / missing)" -f $Item.Src) -ForegroundColor Red
        $Stats.Failed = $Stats.Failed + 1
        continue
    }

    if ($DryRun) {
        Write-Host ("[DRY  ] {0}  <-  {1}" -f $Item.Dst, $Result.Source)
    } else {
        if (-not (Test-Path $DstDir)) {
            New-Item -ItemType Directory -Force -Path $DstDir | Out-Null
        }
        Set-Content -Path $DstAbs -Value $Result.Content -NoNewline
        $Stats.Ok = $Stats.Ok + 1
        Write-Host ("[ OK  ] {0}  <-  {1}" -f $Item.Dst, $Result.Source) -ForegroundColor Green
    }
}

# ── Write migration-audit metadata README ──────────────────────────────────
$MetaPath = Join-Path $TargetRoot "README_MIGRATION.md"
$MetaContent = @"
<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule (evorule-application application-layer assets),
  licensed under GNU Affero General Public License v3 or later.
-->

# evorule-cli Business-Rule Template Migration Metadata

> The two industry rule sets under `hospital/` and `law-firm/` in this directory
> were **migrated OUT of the evorule core framework repository on 2026-07-30**
> as a mechanism/application boundary-cleanup action.  Corresponding boundary
> rule in the core repo's AGENTS.md:
> **"Adding rule templates to evorule-cli = business content, NOT mechanism,
> move to evorule-application (REJECT)"**

## Basic information

| Field | Value |
|---|---|
| Migration completion date | 2026-07-30 |
| Original location (evorule core) | `evorule-cli/examples/hospital/` + `evorule-cli/examples/law-firm/` |
| New location (evorule-application) | `examples/evorule-cli/hospital/` + `examples/evorule-cli/law-firm/` |
| Total files migrated | 10 (hospital 5 + law-firm 5: 2 README, 2 payload, 6 JSON rules) |
| Original license | AGPL-3.0-or-later (same as evorule core repo) |
| Recovery method used | `git show HEAD:evorule-cli/examples/...` (core HEAD snapshot) |

## Corresponding audit-trail entries (inside the evorule core repo)

1. CHANGELOG.md v0.1.0 section "走神 9 拆分（机制-应用边界清理）"
2. evorule-cli/README.md top banner "业务规则模板位置说明（边界合规 · 2026-07-30）"
3. evorule-cli/examples/README.md trailing "[留痕声明]" block

## AGENTS.md Rule 2 (Warning + Confirmation)

Before performing this migration the Rule-2 warning-and-confirmation flow was
completed and the decision was explicitly confirmed by the project side
(2026-07-30 decision):
- WARNING: This violates the evorule core "mechanism-only" purity rule.
- WARNING: Negative consequences
    1) Breaks core purity, future refactor cost rises.
    2) Blurs the mechanism vs. strategy boundary, maintenance complexity grows.
    3) Business-rule changes would otherwise drag the core release cadence.
    4) Duplicates / overlaps with the solution set in evorule-application.
- EXPLICIT PROJECT-SIDE CONFIRMATION: YES (2026-07-30, decision text: "业务规则迁移").
- [x] Rule-2 "record and retain trail" — this file is the retained trail.

## Maintenance guidelines (going forward)

- Any and all BUSINESS changes to these two rule sets (or any new industry
  template added later) are committed **ONLY HERE in evorule-application**.
  **Never move them back into evorule core.**
- New industry templates (AML, government data-classification, etc.) go
  directly under `examples/evorule-cli/<industry-name>/` following the same
  three-file layout used by hospital/ and law-firm/.
"@

if ($DryRun) {
    Write-Host ("[DRY  ] Write migration metadata {0}" -f $MetaPath)
} else {
    if ((Test-Path $MetaPath) -and -not $Overwrite) {
        Write-Host "[SKIP ] Migration metadata README_MIGRATION.md already exists (pass -Overwrite to rewrite)"
    } else {
        Set-Content -Path $MetaPath -Value $MetaContent -NoNewline
        Write-Host "[ OK  ] Wrote migration metadata README_MIGRATION.md" -ForegroundColor Green
    }
}

# ── Final report ───────────────────────────────────────────────────────────
Write-Host ""
Write-Host "─────────────────────────────────────────────────────────────────────────"
Write-Host ("Migration summary : OK={0}  SKIPPED={1}  FAILED={2} (total {3} business-rule files)" -f $Stats.Ok, $Stats.Skipped, $Stats.Failed, $Manifest.Count)
Write-Host ("Destination dir   : {0}" -f $TargetRoot)
Write-Host "─────────────────────────────────────────────────────────────────────────"
Write-Host ""
Write-Host "Recommended next steps:"
Write-Host ("  1) cd {0}" -f $AppRepo)
Write-Host "     then inspect:  git status --short examples/evorule-cli/"
Write-Host "  2) Commit ONLY inside evorule-application (do NOT mix with any evorule-core commit)."
Write-Host "  3) OPTIONAL: copy this script into evorule-application/scripts/ for long-term maintenance."

if ($Stats.Failed -gt 0) { exit 1 }
exit 0
