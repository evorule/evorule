# SPDX-License-Identifier: AGPL-3.0-or-later
﻿# fix-md040.ps1 - Auto-detect and add language to fenced code blocks

param(
    [string]$RootDir = "d:\evorule"
)

$ErrorActionPreference = "Stop"
$nl = [Environment]::NewLine

function Detect-CodeLanguage {
    param([string[]]$codeLines)
    $text = ($codeLines -join "`n").Trim()
    if ($text.Length -eq 0) { return "text" }

    $firstLine = $codeLines[0].Trim()
    $allText = $codeLines -join " "

    # JSON: starts with { or [ and has "key":
    if ($firstLine -match '^[\{\[]' -and $allText -match '"[^"]+"\s*:') { return "json" }

    # Rust: fn / let mut / impl / use std::
    if ($allText -match '\bfn\s+\w+\s*\(') { return "rust" }
    if ($allText -match '\blet\s+mut\b') { return "rust" }
    if ($allText -match '\bimpl\s+\w+') { return "rust" }
    if ($allText -match '\buse\s+std::') { return "rust" }

    # Shell: $ prompt, cargo commands
    if ($firstLine -match '^\$\s+') { return "bash" }
    if ($allText -match '\bcargo\s+(run|test|build|check|clippy)') { return "bash" }

    # Python: def / import / print(
    if ($allText -match '\bdef\s+\w+\s*\(') { return "python" }
    if ($allText -match '\bimport\s+\w+') { return "python" }
    if ($allText -match 'print\(') { return "python" }

    # TypeScript/JS: const / function / =>
    if ($allText -match '\bconst\s+\w+\s*=') { return "typescript" }
    if ($allText -match '\bfunction\s+\w+\s*\(') { return "typescript" }
    if ($allText -match '=>\s*\{?') { return "typescript" }

    # YAML: key: value with indentation
    if ($firstLine -match '^\w[\w-]*\s*:\s*' -and $allText -match "\n\s+\w[\w-]*\s*:") { return "yaml" }

    # ASCII diagram: box-drawing chars
    if ($allText -match '[┌─┐└┘│├┤┬┴╔╗╚╝║═]') { return "text" }
    if ($allText -match '^\s+[├└]──') { return "text" }

    # Lots of pipes = table/diagram
    $pipeCount = 0
    foreach ($c in $allText.ToCharArray()) { if ($c -eq '|') { $pipeCount++ } }
    if ($pipeCount -gt 5) { return "text" }

    return "text"
}

function Process-File {
    param([string]$filePath)
    $bytes = [System.IO.File]::ReadAllBytes($filePath)
    $content = [System.Text.Encoding]::UTF8.GetString($bytes)
    $lines = $content -split "`n"

    $inCode = $false
    $codeStart = 0
    $blockCount = 0
    $newLines = New-Object System.Collections.Generic.List[string]

    for ($i = 0; $i -lt $lines.Length; $i++) {
        $line = $lines[$i]
        if ($line -match '^\s*```\s*$' -and -not $inCode) {
            $inCode = $true
            $codeStart = $i
            $newLines.Add($line) | Out-Null
        } elseif ($line -match '^\s*```' -and $inCode) {
            $inCode = $false
            $endIdx = $i - 1
            if ($endIdx -ge $codeStart + 1) {
                $codeBlockLines = $lines[($codeStart + 1)..$endIdx]
            } else {
                $codeBlockLines = @()
            }
            $lang = Detect-CodeLanguage $codeBlockLines
            $newLines[$codeStart] = "```$lang"
            $newLines.Add($line) | Out-Null
            $blockCount++
        } else {
            $newLines.Add($line) | Out-Null
        }
    }

    if ($blockCount -gt 0) {
        $newContent = $newLines -join "`n"
        [System.IO.File]::WriteAllBytes($filePath, [System.Text.Encoding]::UTF8.GetBytes($newContent))
        Write-Host "UPDATED: $(Split-Path $filePath -Leaf) ($blockCount blocks)"
    }
    return $blockCount
}

$totalFixed = 0

$targetFiles = @(
    "evorule-tcb/tla/TLC_VERIFICATION_REPORT.md",
    "docs/security/SECURITY_AUDIT_v1.0.0.md",
    "docs/security/SECURITY_AUDIT_v0.1.0.md",
    "docs/benchmarks/EXP_1.5.md",
    "docs/security/DEPENDENCY_AUDIT_v1.0.0.md",
    "CONTRIBUTING_ZH.md",
    "docs/security/DEPENDENCY_AUDIT_v0.1.0.md",
    "VERSION_STRATEGY.md",
    "docs/security/THREAT_MODEL.md",
    "CONTRIBUTING.md",
    "README.md",
    "docs/benchmarks/EXP_1.4.md",
    "evorule-cli/README.md",
    "docs/benchmarks/EXP_1.2.md",
    "docs/benchmarks/EXP_1.3.md",
    "evorule-tcb/README.md",
    "evorule-governance/README.md"
)

foreach ($relPath in $targetFiles) {
    $full = Join-Path $RootDir $relPath
    if (Test-Path $full) {
        $totalFixed += Process-File $full
    } else {
        Write-Host "MISSING: $relPath"
    }
}

Write-Host "---"
Write-Host "Total code blocks fixed: $totalFixed"
