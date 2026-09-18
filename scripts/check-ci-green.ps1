# SPDX-License-Identifier: AGPL-3.0-or-later
# check-ci-green.ps1 — 推送后 CI 绿灯检查（推送远端后 CI 绿灯纪律的机械化工具）
#
# 纪律（2026-09-18 建立）：任何推送至公开远端后，必须检查远端 CI 是否全绿；
# 存在红灯必须修复至全绿，该任务方算完成。
#
# 用法：
#   .\scripts\check-ci-green.ps1 -Sha a3d728fbd5f4cb311a4032a36886faec35e7c5e7
#   .\scripts\check-ci-green.ps1 -Sha <full-or-7+-char-sha> -Repo evorule/evorule -TimeoutMin 15
#
# 判定：head_sha 匹配的全部 workflow runs 至终态：
#   - 全部 success / skipped（paths 过滤未跑，非红）→ RC=0 输出 PASS
#   - 任一 failure / cancelled / startup_failure / timed_out → RC=1 输出 FAIL + 失败 job 清单
#   - 超时未终态 → RC=2 输出 INCONCLUSIVE
param(
    [Parameter(Mandatory = $true)]
    [string]$Sha,
    [string]$Repo = 'evorule/evorule',
    [int]$TimeoutMin = 15,
    [int]$PollSec = 20
)

$ErrorActionPreference = 'Stop'
$api = "https://api.github.com/repos/$Repo/actions/runs"
$deadline = (Get-Date).AddMinutes($TimeoutMin)
$headers = @{ 'User-Agent' = 'evorule-ci-green-check'; 'X-GitHub-Api-Version' = '2022-11-28' }

function Get-Runs {
    $r = Invoke-RestMethod -Uri "$api`?head_sha=$Sha&per_page=100" -Headers $headers
    @($r.workflow_runs)
}

# 阶段 1：等待至少一个 run 注册（push 后 runs 有秒级延迟）
$runs = @()
while ($runs.Count -eq 0) {
    if ((Get-Date) -gt $deadline) {
        Write-Host "INCONCLUSIVE: $TimeoutMin 分钟内未见任何 run 注册（head_sha=$Sha）"
        exit 2
    }
    Start-Sleep -Seconds $PollSec
    $runs = Get-Runs
}

# 阶段 2：轮询至全部终态
while ($true) {
    $pending = @($runs | Where-Object { $_.status -ne 'completed' })
    if ($pending.Count -eq 0) { break }
    if ((Get-Date) -gt $deadline) {
        Write-Host "INCONCLUSIVE: $TimeoutMin 分钟内未全部终态，仍在跑:"
        $pending | ForEach-Object { Write-Host "  $($_.name): $($_.status)" }
        exit 2
    }
    Start-Sleep -Seconds $PollSec
    $runs = Get-Runs
}

# 阶段 3：判定
$failed = @($runs | Where-Object { $_.conclusion -notin @('success', 'skipped', 'neutral') })
$lines = $runs | ForEach-Object { "$($_.name): $($_.conclusion)" }
$lines | Sort-Object | ForEach-Object { Write-Host $_ }

if ($failed.Count -gt 0) {
    Write-Host "FAIL: $($failed.Count)/$($runs.Count) 个 workflow 非 success（head_sha=$Sha）"
    foreach ($f in $failed) {
        Write-Host "  失败 job 明细: https://api.github.com/repos/$Repo/actions/runs/$($f.id)/jobs"
    }
    exit 1
}
Write-Host "PASS: $($runs.Count) 个 workflow 全绿（head_sha=$Sha）"
exit 0