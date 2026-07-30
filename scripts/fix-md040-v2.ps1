# fix-md040-v2.ps1 - Fix MD040 by adding language to fenced code blocks
# Version 2: more robust parsing

param(
    [string]$RootDir = "d:\evorule"
)

$ErrorActionPreference = "Stop"

function Detect-CodeLanguage {
    param([string[]]$codeLines)
    $text = ($codeLines -join "`n").Trim()
    if ($text.Length -eq 0) { return "text" }

    $firstNonEmpty = ""
    foreach ($l in $codeLines) {
        if ($l.Trim().Length -gt 0) { $firstNonEmpty = $l.Trim(); break }
    }
    $allText = $codeLines -join " "

    # JSON: starts with { or [
    if ($firstNonEmpty -match '^[\{\[]') { return "json" }

    # Rust
    if ($allText -match '\bfn\s+\w+\s*\(') { return "rust" }
    if ($allText -match '\blet\s+mut\b') { return "rust" }
    if ($allText -match '\bimpl\s+\w+') { return "rust" }
    if ($allText -match '\buse\s+std::') { return "rust" }

    # Shell
    if ($firstNonEmpty -match '^\$\s+') { return "bash" }
    if ($allText -match '\bcargo\s+(run|test|build|check|clippy)') { return "bash" }
    if ($firstNonEmpty -match '^cargo\s') { return "bash" }

    # Python
    if ($allText -match '\bdef\s+\w+\s*\(') { return "python" }
    if ($allText -match 'print\(') { return "python" }

    # TypeScript/JS
    if ($allText -match '\bconst\s+\w+\s*=') { return "typescript" }
    if ($allText -match '\bfunction\s+\w+\s*\(') { return "typescript" }

    # YAML
    if ($firstNonEmpty -match '^\w[\w-]*\s*:\s*' -and $allText -match "`n\s+\w[\w-]*\s*:") { return "yaml" }

    # TOML
    if ($allText -match '^\[[\w\.]+\]$' -and $allText -match '\w+\s*=\s*"') { return "toml" }

    # ASCII diagram
    if ($allText -match '[┌─┐└┘│├┤┬┴╔╗╚╝║═]') { return "text" }
    if ($allText -match '^\s+[├└]──') { return "text" }

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

    $result = New-Object System.Collections.Generic.List[string]
    $inCode = $false
    $codeStart = -1
    $blockCount = 0

    for ($i = 0; $i -lt $lines.Length; $i++) {
        $line = $lines[$i]

        # Check for fenced code block start (only ``` with no language)
        if (-not $inCode -and $line -match '^(\s*)```\s*$') {
            $inCode = $true
            $codeStart = $result.Count
            $result.Add($line) | Out-Null
            continue
        }

        # Check for fenced code block end
        if ($inCode -and $line -match '^\s*```\s*$') {
            $inCode = $false
            $endIdx = $result.Count - 1
            if ($endIdx -gt $codeStart) {
                $codeBlockLines = $result.GetRange($codeStart + 1, $endIdx - $codeStart).ToArray()
            } else {
                $codeBlockLines = @()
            }
            $lang = Detect-CodeLanguage $codeBlockLines
            $indent = if ($result[$codeStart] -match '^(\s*)```') { $Matches[1] } else { "" }
            $result[$codeStart] = "$indent```$lang"
            $result.Add($line) | Out-Null
            $blockCount++
            continue
        }

        $result.Add($line) | Out-Null
    }

    # Handle unclosed code block
    if ($inCode) {
        # Just leave it as is
    }

    if ($blockCount -gt 0) {
        $newContent = $result -join "`n"
        [System.IO.File]::WriteAllBytes($filePath, [System.Text.Encoding]::UTF8.GetBytes($newContent))
        Write-Host "UPDATED: $(Split-Path $filePath -Leaf) ($blockCount blocks)"
    }
    return $blockCount
}

# Find all .md files with MD040 errors by running markdownlint
Write-Host "Scanning for MD040 errors..."
$lintOutput = npx --yes markdownlint-cli "$RootDir\docs\**\*.md" "$RootDir\*.md" "$RootDir\evorule-tcb\**\*.md" "$RootDir\evorule-reactor\**\*.md" "$RootDir\evorule-governance\**\*.md" "$RootDir\evorule-cli\**\*.md" "$RootDir\.gitee\*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**" 2>&1 | Out-String

$filesToFix = @{}
foreach ($line in ($lintOutput -split "`n")) {
    if ($line -match '^(.+?):\d+.*MD040') {
        $f = $Matches[1]
        if (-not $filesToFix.ContainsKey($f)) { $filesToFix[$f] = $true }
    }
}

Write-Host "Found $($filesToFix.Count) files with MD040 errors"
$totalFixed = 0

foreach ($f in $filesToFix.Keys) {
    if (Test-Path $f) {
        $totalFixed += Process-File $f
    }
}

Write-Host "---"
Write-Host "Total code blocks fixed: $totalFixed"
