# show-md040.ps1 - Show specific MD040 errors with context

$output = npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**" 2>&1 | Out-String

foreach ($line in ($output -split "`n")) {
    if ($line -match 'MD040') {
        Write-Host $line
    }
}
