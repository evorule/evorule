# find-md040.ps1 - Find remaining files with MD040 errors

$output = npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**" 2>&1 | Out-String

$files = @{}
foreach ($line in ($output -split "`n")) {
    if ($line -match '^(.+?):(\d+).*MD040') {
        $f = $Matches[1]
        if (-not $files.ContainsKey($f)) { $files[$f] = 0 }
        $files[$f]++
    }
}

$files.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object {
    Write-Host "$($_.Value)`t$($_.Name)"
}
Write-Host "---"
Write-Host "Total MD040: $(($files.Values | Measure-Object -Sum).Sum)"
Write-Host "Files: $($files.Count)"
