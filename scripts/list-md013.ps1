# list-md013.ps1 - List MD013 errors by file

$output = npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**" 2>&1 | Out-String

$files = @{}
foreach ($line in ($output -split "`n")) {
    if ($line -match 'MD013') {
        if ($line -match '^(.+?):\d+') {
            $file = $Matches[1]
            if ($files.ContainsKey($file)) { $files[$file]++ } else { $files[$file] = 1 }
        }
    }
}

$files.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object {
    Write-Host "$($_.Value)`t$($_.Name)"
}
Write-Host "---"
$total = ($files.Values | Measure-Object -Sum).Sum
Write-Host "Total MD013: $total"
