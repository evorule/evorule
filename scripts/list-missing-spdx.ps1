# List all .rs files missing SPDX header
$evoruleRoot = "D:\evorule"
$evoAgentRoot = "D:\evo-agent"

$rsDirs = @(
    "$evoruleRoot\tier0-tcb\src",
    "$evoruleRoot\tier1-reactor\src",
    "$evoruleRoot\tier2-governance\src",
    "$evoAgentRoot\src"
)
foreach ($dir in $rsDirs) {
    if (-not (Test-Path $dir)) { continue }
    $rsFiles = Get-ChildItem -Path $dir -Filter "*.rs" -Recurse
    foreach ($f in $rsFiles) {
        $head = Get-Content $f.FullName -TotalCount 12 -ErrorAction SilentlyContinue
        if (-not ($head -join "`n" -match 'SPDX-License-Identifier:\s*AGPL-3.0')) {
            Write-Host $f.FullName
        }
    }
}
