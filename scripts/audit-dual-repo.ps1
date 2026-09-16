# audit-dual-repo.ps1 — EvoRule 双仓健康门禁（P2 #12）
# 用法:
#   powershell -File audit-dual-repo.ps1           # 全量巡检, 输出 PASS/FAIL, 退出码 0/1
#   powershell -File audit-dual-repo.ps1 -Quiet    # 仅输出 FAIL 与汇总
# 建议: 每周一 09:00 运行(任务计划) 或结合 doubao 每周巡检提醒。
# 检查项: 1) 本地工作区干净  2) 两端 HEAD 一致  3) remote 约定  4) mirror.yml 存在
#         5) 默认分支 = main  6) 备份链最近备份存在
param([switch]$Quiet)

$repos = @(
  @{ Name="evorule";        Gitee="gitee";  Github="github" },
  @{ Name="evorule-server"; Gitee="origin"; Github="github" },
  @{ Name="evorule-rule";   Gitee="origin"; Github="github" },
  @{ Name="evorule-system-rules"; Gitee="origin"; Github="github" },
  @{ Name="evorule-console-cloud"; Gitee="origin"; Github="github" },
  @{ Name="evorule-console"; Gitee="origin"; Github="github" },
  @{ Name="evo-agent";      Gitee="origin"; Github="github" },
  @{ Name="evorule-hash";   Gitee="origin"; Github="github" },
  @{ Name="evorule-bundle"; Gitee="origin"; Github="github" },
  @{ Name="evorule-sdk";    Gitee="origin"; Github="github" },
  @{ Name="evorule-dsh-skill"; Gitee="origin"; Github="github" }
)
$privateRepos = @("evorule-agent")  # 私有仓: 仅 Gitee(SSH 可达) + 本地; GitHub 镜像豁免

$issues = 0
function Report($ok, $msg) {
  if ($ok) { if (-not $Quiet) { Write-Host ("  [PASS] " + $msg) -ForegroundColor Green } }
  else     { Write-Host ("  [FAIL] " + $msg) -ForegroundColor Red; $script:issues++ }
}

Write-Host "===== EvoRule 双仓健康巡检 $(Get-Date -Format 'yyyy-MM-dd HH:mm') ====="

foreach ($r in $repos) {
  $path = "D:\$($r.Name)"
  Write-Host "--- $($r.Name) ---"
  if (-not (Test-Path $path)) { Report $false "本地目录缺失: $path"; continue }

  # 1) 工作区干净
  $dirty = git -C $path status --porcelain
  Report ([string]::IsNullOrEmpty($dirty)) "工作区干净 (dirty=$([string]::IsNullOrEmpty($dirty)))"

  # 2) 两端 HEAD 一致
  $local = git -C $path rev-parse main 2>$null
  $ge = git -C $path ls-remote $($r.Gitee) main 2>$null | ForEach-Object { ($_ -split "\s+")[0] }
  $gh = git -C $path ls-remote $($r.Github) main 2>$null | ForEach-Object { ($_ -split "\s+")[0] }
  Report (($local -and $ge -and $gh -and $local -eq $ge -and $ge -eq $gh)) "两端 HEAD 一致 (local=$local ge=$ge gh=$gh)"

  # 3) mirror.yml 存在
  Report (Test-Path "$path\.github\workflows\mirror.yml") "mirror.yml 存在"

  # 4) 默认分支(远端 HEAD 指向)
  $headRef = git -C $path ls-remote --symref $($r.Gitee) HEAD 2>$null | Select-String "ref: refs/heads/"
  Report (($headRef -and $headRef.ToString().Contains("refs/heads/main"))) "Gitee 默认分支=main"
}

# 私有仓: 本地 + Gitee HEAD 一致(SSH)
Write-Host "--- evorule-agent(私有) ---"
$privPath = "D:\evorule-agent"
if (Test-Path $privPath) {
  $dirty = git -C $privPath status --porcelain
  Report ([string]::IsNullOrEmpty($dirty)) "工作区干净"
  $local = git -C $privPath rev-parse main 2>$null
  $ge = git -C $privPath ls-remote gitee main 2>$null | ForEach-Object { ($_ -split "\s+")[0] }
  Report (($local -and $ge -and $local -eq $ge)) "Gitee HEAD 一致 (local=$local ge=$ge)"
} else { Report $false "本地目录缺失: $privPath" }

# 5) 备份链最近备份存在且覆盖 12 仓
Write-Host "--- 备份链 ---"
$latest = Get-ChildItem "D:\evorule-backup\golden" -Directory -ErrorAction SilentlyContinue |
  Sort-Object Name -Descending | Select-Object -First 1
if ($latest) {
  $cnt = (Get-ChildItem $latest.FullName -Directory).Count
  Report ($cnt -ge 12) "最近备份 $($latest.Name) 覆盖 $cnt/12 仓"
} else { Report $false "无备份(备份链未运行)" }

Write-Host "===== 巡检结束: $issues 项 FAIL ====="
exit $(if ($issues -gt 0) { 1 } else { 0 })
