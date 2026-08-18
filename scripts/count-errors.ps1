# SPDX-License-Identifier: AGPL-3.0-or-later
﻿# count-errors.ps1 - Count markdownlint errors by type

$output = npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**" 2>&1 | Out-String

$lines = $output -split "`n"
$errors = @{}
$total = 0
foreach ($line in $lines) {
    if ($line -match 'error (\w+[\/\w-]*)') {
        $type = $Matches[1]
        if ($errors.ContainsKey($type)) { $errors[$type]++ } else { $errors[$type] = 1 }
        $total++
    }
}

$errors.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object {
    Write-Host "$($_.Value)`t$($_.Name)"
}
Write-Host "---"
Write-Host "Total errors: $total"
