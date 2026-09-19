# sync-msg-hook.ps1 - 将本仓 .git/hooks/commit-msg 红线钩子同步到全部公开仓本地 clone
#
# 背景: commit message 是双端推送面, 红线扫描前移到提交期 (commit-msg hook)。
# 钩子为本地工具, 不随仓库公开; 各仓本地 clone 独立存在, 用本脚本保持
# 词表一致 (SHA256 对比验证)。词表含: AI 身份 / AI 署名 / 内部主键编号 /
# 内部批次编号 / 决策序号 / 私有仓名 六类红线 (词表拼接防自指)。
#
# 用法:
#   powershell -File scripts/sync-msg-hook.ps1           # 同步 + 验证
#   powershell -File scripts/sync-msg-hook.ps1 -Check    # 仅验证哈希一致
param([switch]$Check)

$ErrorActionPreference = 'Stop'
$src = Join-Path $PSScriptRoot '..\.git\hooks\commit-msg'
if (-not (Test-Path $src)) { Write-Error "源钩子不存在: $src" }
$srcHash = (Get-FileHash $src -Algorithm SHA256).Hash
Write-Host "源: $src"
Write-Host "SHA256: $srcHash"

$repos = @(
    'D:\evorule',
    'D:\evo-agent',
    'D:\evorule-server',
    'D:\evorule-rule',
    'D:\evorule-system-rules',
    'D:\evorule-console-cloud',
    'D:\evorule-console',
    'D:\evorule-hash',
    'D:\evorule-bundle',
    'D:\evorule-sdk',
    'D:\evorule-dsh-skill',
    'D:\evorule-experience-pack'
)

$fail = 0
foreach ($repo in $repos) {
    $dst = Join-Path $repo '.git\hooks\commit-msg'
    if (-not (Test-Path (Join-Path $repo '.git'))) {
        Write-Host "SKIP (非 git 仓或不存在): $repo" -ForegroundColor Yellow
        $fail++; continue
    }
    $inSync = (Test-Path $dst) -and ((Get-FileHash $dst -Algorithm SHA256).Hash -eq $srcHash)
    if ($inSync) {
        Write-Host "OK   (一致): $repo"
    } elseif ($Check) {
        Write-Host "DRIFT (漂移): $repo" -ForegroundColor Red
        $fail++
    } else {
        Copy-Item $src $dst -Force
        $after = (Get-FileHash $dst -Algorithm SHA256).Hash
        if ($after -eq $srcHash) { Write-Host "SYNC (已同步): $repo" -ForegroundColor Green }
        else { Write-Host "FAIL (同步后哈希不符): $repo" -ForegroundColor Red; $fail++ }
    }
}
if ($fail -gt 0) { Write-Error "存在 $fail 个未一致项" }
Write-Host '全部公开仓钩子一致。'
