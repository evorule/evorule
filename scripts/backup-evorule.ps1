# backup-evorule.ps1 — EvoRule 黄金备份链 v2（单人版去单点）
# 用法:
#   powershell -File backup-evorule.ps1            # 全量备份
#   powershell -File backup-evorule.ps1 -Verify    # 备份后从裸仓恢复演练(抽查前 2 仓)
# 建议: Windows 任务计划每周日 02:00 运行(见 README 末尾或 REPO_REGISTRY 维护规则)
# 备份源: Gitee 主仓(权威源)。GitHub 镜像无需备份(其存在本身即灾备)。
# 覆盖: 11 公开仓(HTTPS) + evorule-agent 私有仓(SSH, 无 GitHub 灾备, 必须备份)。
param([switch]$Verify)

$ErrorActionPreference = 'Stop'

$public = @(
  "evorule","evorule-server","evorule-rule","evorule-system-rules",
  "evorule-console-cloud","evorule-console","evo-agent","evorule-hash",
  "evorule-bundle","evorule-sdk","evorule-dsh-skill"
)
$private = @{ "evorule-agent" = "git@gitee.com:evorulelab/evorule-agent.git" }

$base   = "D:\evorule-backup\golden"
$stamp  = Get-Date -Format "yyyyMMdd"
$dest   = Join-Path $base $stamp
New-Item -ItemType Directory -Force -Path $dest | Out-Null

$failed = 0
foreach ($r in $public) {
  Write-Host "备份: $r"
  git clone --mirror "https://gitee.com/evorule/$r.git" (Join-Path $dest "$r.git") 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) { Write-Warning "备份失败: $r"; $failed++ }
}
foreach ($k in $private.Keys) {
  Write-Host "备份(私有): $k"
  git clone --mirror $private[$k] (Join-Path $dest "$k.git") 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) { Write-Warning "备份失败: $k"; $failed++ }
}

# 保留最近 4 份
Get-ChildItem $base -Directory | Sort-Object Name -Descending | Select-Object -Skip 4 |
  Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "备份完成: $stamp ($($public.Count + $private.Count) 仓, 失败 $failed)"

if ($Verify) {
  Write-Host "===== 恢复演练(从裸仓抽查) ====="
  $probe = "D:\evorule-backup\_restore_probe"
  if (Test-Path $probe) { Remove-Item -Recurse -Force $probe }
  New-Item -ItemType Directory -Force -Path $probe | Out-Null
  $probeRepos = @("evorule","evorule-agent")
  foreach ($r in $probeRepos) {
    $mirror = Join-Path $dest "$r.git"
    if (-not (Test-Path $mirror)) { Write-Warning "演练跳过(无备份): $r"; continue }
    Write-Host "演练: 从裸仓恢复 $r"
    git clone $mirror (Join-Path $probe $r) 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Warning "恢复失败: $r"; continue }
    $head = git -C (Join-Path $probe $r) rev-parse --short HEAD
    Write-Host "  恢复成功: $r @ $head"
  }
  Remove-Item -Recurse -Force $probe
}
