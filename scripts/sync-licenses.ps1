# sync-licenses.ps1 — 许可文件七件套同步器（-Check 核验 / -Sync 分发）
# 权威源＝本仓（evorule 主仓）根目录七件套；副本仓经仓名归一化后须与主仓一致。
# 仓名定制规则：副本仓文本中 gitee.com/evorule/<repo> 等本仓名 token，归一化
# （repo 名 → evorule）后与主仓文本逐行比对，一致即合规（NOTICE 末行 URL、
# COMMERCIAL 等仓名替换版均按此规则核验）。
# 特例：evorule-sdk 维持 Apache-2.0 自治体系（仓级许可差异设计），仅核验
# LICENSE/NOTICE 存在性，不做内容对比。
# 用法：powershell -File scripts/sync-licenses.ps1 -Check   （默认）
#       powershell -File scripts/sync-licenses.ps1 -Sync

param(
  [switch]$Check,
  [switch]$Sync
)

$ErrorActionPreference = 'Stop'
$mainRoot = Split-Path -Parent $PSScriptRoot
$files = @('CLA-corporate.md','CLA-individual.md','COMMERCIAL_LICENSE.md','DUAL_LICENSE.md','FREE_COMMERCIAL_LICENSE.md','LICENSE','NOTICE.md')
# 仓根目录名（与本仓同级）→ 是否套用内容对比
$repos = @(
  'evo-agent','evorule-rule','evorule-server','evorule-system-rules',
  'evorule-console-cloud','evorule-console','evorule-hash','evorule-bundle',
  'evorule-sdk','evorule-dsh-skill','evorule-experience-pack'
)
# Apache 自治仓：仅核验存在性（sdk 按设计维持 Apache-2.0）
$apacheRepos = @('evorule-sdk')
# Apache 自治仓裁定不补的五件（存在性也不核验）
$filesSkipApache = @('CLA-corporate.md','CLA-individual.md','COMMERCIAL_LICENSE.md','DUAL_LICENSE.md','FREE_COMMERCIAL_LICENSE.md')

$mainText = @{}
foreach ($f in $files) {
  $p = Join-Path $mainRoot $f
  if (-not (Test-Path $p)) { Write-Output "FAIL 主仓缺 $f"; exit 1 }
  $mainText[$f] = [IO.File]::ReadAllText($p)
}

$fail = 0; $synced = 0
foreach ($repo in $repos) {
  $repoRoot = Join-Path (Split-Path -Parent $mainRoot) $repo
  if (-not (Test-Path $repoRoot)) { Write-Output "FAIL 仓不存在 $repoRoot"; $fail++; continue }
  $isApache = $apacheRepos -contains $repo
  foreach ($f in $files) {
    $dst = Join-Path $repoRoot $f
    if ($isApache) {
      if ($filesSkipApache -contains $f) { Write-Output "SKIP $repo/$f (Apache 自治仓，裁定不适用)"; continue }
      if (Test-Path $dst) { Write-Output "OK   $repo/$f (存在性核验)" }
      else { Write-Output "FAIL $repo/$f 缺失"; $fail++ }
      continue
    }
    if (-not (Test-Path $dst)) {
      if ($isApache -and $filesSkipApache -contains $f) { Write-Output "SKIP $repo/$f (Apache 自治仓，裁定不适用)"; continue }
      Write-Output "MISS $repo/$f 缺失"
      if ($Sync) { Copy-Item (Join-Path $mainRoot $f) $dst -Force; Write-Output "SYNC $repo/$f <- 主仓覆盖"; $synced++ } else { $fail++ }
      continue
    }
    if ($isApache -and $filesSkipApache -contains $f) { Write-Output "SKIP $repo/$f (Apache 自治仓，裁定不适用)"; continue }
    $cur = [IO.File]::ReadAllText($dst)
    $main = $mainText[$f]
    # 双边同态归一化：两侧同做 repo→evorule 替换（主仓文本含其他仓仓名列表条目时不误伤）
    $norm = $cur.Replace($repo, 'evorule')
    $mainNorm = $main.Replace($repo, 'evorule')
    # 行尾归一后逐行比较（行尾差异不计入，字节级一致性由 git 侧维护）
    $same = ((($norm -replace "`r`n", "`n")) -eq (($mainNorm -replace "`r`n", "`n")))
    if ($same) { Write-Output "OK   $repo/$f" }
    else {
      Write-Output "DIFF $repo/$f"
      if ($Sync) {
        if ($f -eq 'NOTICE.md') {
          $new = $main.Replace('gitee.com/evorule/evorule', "gitee.com/evorule/$repo")
          [IO.File]::WriteAllText($dst, $new)
        } else {
          Copy-Item (Join-Path $mainRoot $f) $dst -Force
        }
        Write-Output "SYNC $repo/$f <- 已重写（NOTICE=模板+仓名注入，其余=主仓原文）"
        $synced++
      } else { $fail++ }
    }
  }
}
Write-Output ('----')
if ($Sync) { Write-Output "SYNC 完成：重写 $synced 件；复验请再跑 -Check" }
if ($fail -gt 0) { Write-Output "CHECK-FAIL: $fail 项不一致"; exit 1 }
Write-Output 'CHECK-PASS: 十一仓许可文件全部合规'
