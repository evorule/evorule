# SPDX-License-Identifier: AGPL-3.0-or-later
# sync-doc-safety.ps1 — check_doc_safety.py 多仓副本同步器（同步真相源机制）
#
# 母本 = 本仓 scripts/check_doc_safety.py（evorule 主仓）；各公开仓副本
# （scripts/check_doc_safety.py）以母本字节级一致为对齐标准。
#
# 用法（在 evorule 主仓执行）：
#   .\scripts\sync-doc-safety.ps1            # 默认 = -Check，只报告各副本与母本的漂移
#   .\scripts\sync-doc-safety.ps1 -Check     # 同上
#   .\scripts\sync-doc-safety.ps1 -Sync      # 复制母本到各副本并逐文件哈希校验（只复制，不删除）
#
# 纪律：-Sync 后各仓产生未提交变更，须逐仓提交并推送双远端（Gitee 权威仓 + GitHub 镜像）；
#       推送后按 CI 绿灯纪律检查远端 CI 全绿。词表/豁免内容的变更须先修订内部术语表再改母本。
param(
    [switch]$Check,
    [switch]$Sync,
    [string[]]$Repos = @('evorule-server', 'evorule-console-cloud', 'evorule-console', 'evorule-rule', 'evorule-system-rules', 'evo-agent', 'evorule-sdk'),
    # 默认假定各仓与母本仓同级（母本仓父目录下）；布局不同时用 -Base 显式指定仓根父目录
    [string]$Base = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent)
)
$ErrorActionPreference = 'Stop'
$master = Join-Path $PSScriptRoot 'check_doc_safety.py'
if (-not (Test-Path $master)) { Write-Host "FAIL: 母本缺失: $master"; exit 2 }
$masterHash = (Get-FileHash $master -Algorithm SHA256).Hash
Write-Host "母本: $master"
Write-Host "  SHA256 = $($masterHash.Substring(0, 16))..."

$fail = 0
foreach ($r in $Repos) {
    $target = Join-Path (Join-Path $Base $r) 'scripts/check_doc_safety.py'
    if (-not (Test-Path $target)) { Write-Host "  $r : 副本缺失（$target）"; $fail++; continue }
    $tHash = (Get-FileHash $target -Algorithm SHA256).Hash
    if ($tHash -eq $masterHash) { Write-Host "  $r : 一致"; continue }
    if (-not $Sync) { Write-Host "  $r : 漂移（副本 $($tHash.Substring(0, 16))... 不等于母本）"; $fail++; continue }
    # -Sync：只复制不删除；复制后逐文件校验（文件安全纪律）
    Copy-Item $master $target -Force
    $vHash = (Get-FileHash $target -Algorithm SHA256).Hash
    if ($vHash -eq $masterHash) { Write-Host "  $r : 已同步并校验一致" }
    else { Write-Host "  $r : 复制后校验失败（$target）"; $fail++ }
}
if ($fail -gt 0) { Write-Host "FAIL: $fail 项未对齐"; exit 1 }
Write-Host "PASS: 全部副本与母本一致"
exit 0
