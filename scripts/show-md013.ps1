# SPDX-License-Identifier: AGPL-3.0-or-later
﻿# show-md013.ps1 - Show MD013 errors with line content

$output = npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**" 2>&1 | Out-String

foreach ($line in ($output -split "`n")) {
    if ($line -match 'MD013') {
        Write-Host $line
    }
}
