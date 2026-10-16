# List all .rs files missing SPDX header
param(
    # 缺省: 本仓根
    [string]$EvoruleRoot = (Split-Path $PSScriptRoot -Parent),
    # 缺省: 按生态工作区兄弟目录约定解析（本仓根的兄弟目录）
    [string]$EvoAgentRoot = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'evo-agent')
)

$rsDirs = @(
    "$evoruleRoot\evorule-tcb\src",
    "$evoruleRoot\evorule-reactor\src",
    "$evoruleRoot\evorule-governance\src",
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
