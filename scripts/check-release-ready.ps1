# 发版一键预检（O-217 方案①）：推送前本地拦截 CI 红灯返工源
# 串跑五项预检，任一 FAIL 即 exit 1（fail-closed，不吞错）：
#   1. cargo fmt --check          （Rust 仓自动探测；非 Rust 仓跳过）
#   2. 公开面矩阵扫描             （CI 同口径 --fail-on A1 --fail-on B，计入 0 项=PASS）
#   3. 推送密钥泄露扫描           （-EnvFile 提供时执行）
#   4. workspace 成员 req 对齐    （evorule-* registry req 主线与 workspace.version 一致；
#                                  0.7.0/0.9.0 两次发版踩雷的机器化拦截）
#   5. CHANGELOG 残留 [Unreleased]（发布定版时点应为 0 残留；WARNING 级）
# 用法：
#   pwsh -NoProfile -File D:\evorule\scripts\check-release-ready.ps1 -Repo D:\evorule-server
#   pwsh -NoProfile -File D:\evorule\scripts\check-release-ready.ps1 -Repo D:\evorule -EnvFile D:\evorule\.env
param(
    [Parameter(Mandatory = $true)]
    [string]$Repo,
    [string]$EnvFile = ''
)
$ErrorActionPreference = 'Stop'
$script:failed = 0
$script:warned = 0

function Add-Result([string]$name, [string]$status, [string]$note) {
    $icon = switch ($status) { 'PASS' { '[PASS]' } 'FAIL' { '[FAIL]' } 'WARN' { '[WARN]' } 'SKIP' { '[SKIP]' } }
    if ($status -eq 'FAIL') { $script:failed++ }
    if ($status -eq 'WARN') { $script:warned++ }
    Write-Output ("{0} {1} -- {2}" -f $icon, $name, $note)
}

if (-not (Test-Path $Repo)) { Write-Output "ERROR: repo path not found: $Repo"; exit 2 }
$repoName = Split-Path $Repo -Leaf
Write-Output "=== check-release-ready: $repoName ==="

# ── 1. cargo fmt --check（Rust 仓探测）────────────────────────────
$wsCargo = Join-Path $Repo 'Cargo.toml'
if (Test-Path $wsCargo) {
    $fmt = & cargo fmt --manifest-path $wsCargo --all -- --check 2>&1
    if ($LASTEXITCODE -ne 0) {
        Add-Result 'cargo fmt --check' 'FAIL' (($fmt | Select-Object -First 3) -join ' | ')
    } else {
        Add-Result 'cargo fmt --check' 'PASS' '0 diff'
    }
} else {
    Add-Result 'cargo fmt --check' 'SKIP' 'no Cargo.toml'
}

# ── 2. 公开面矩阵扫描（增量零容忍口径）──────────────────────────
# 存量容忍（用户裁定）：全仓命中不判死；只有「待推 diff 范围内文件」的 A1/B 命中 = FAIL。
# 注意：仅 evorule 主仓 CI 有公开面扫描 job；其他仓（如 server）CI 不跑——本地增量口径即终审。
$scanner = 'D:\evorule\scripts\scan_public_face.py'
if (Test-Path $scanner) {
    $out = & python $scanner --root $Repo --fail-on A1 --fail-on B 2>&1 | Out-String
    $hitFiles = @()
    foreach ($m in [regex]::Matches($out, '^\s*(\S+?):\d+\s+\[阻断\]', 'Multiline')) {
        $hitFiles += (Split-Path $m.Groups[1].Value -Leaf)
    }
    $hitFiles = $hitFiles | Select-Object -Unique
    # 待推范围：优先 gitee/main（主仓命名），回落 origin/main（server 仓命名）
    $base = $null
    foreach ($r in @('gitee', 'origin')) {
        if (& git -C $Repo remote | Select-String -SimpleMatch $r -Quiet) {
            $base = "$r/main"; break
        }
    }
    $changed = @()
    if ($base) {
        $changed = (& git -C $Repo diff --name-only "$base..HEAD" 2>$null) | ForEach-Object { Split-Path $_ -Leaf }
    }
    $incremental = @($hitFiles | Where-Object { $changed -contains $_ })
    if ($LASTEXITCODE -eq 0 -and $incremental.Count -eq 0) {
        $stockNote = if ($hitFiles.Count -gt 0) { "存量命中 $($hitFiles.Count) 文件（容忍）；待推增量 0 命中" } else { '全仓 0 命中' }
        Add-Result 'public-face scan (A1/B 增量口径)' 'PASS' $stockNote
    } else {
        Add-Result 'public-face scan (A1/B 增量口径)' 'FAIL' ("待推增量命中 {0} 文件: {1}" -f $incremental.Count, ($incremental -join ', '))
    }
} else {
    Add-Result 'public-face scan' 'SKIP' "scanner not found: $scanner"
}

# ── 3. 推送密钥泄露扫描 ──────────────────────────────────────────
$secretScan = 'D:\evorule\scripts\check-push-secret-safety.ps1'
if (Test-Path $secretScan) {
    if ($EnvFile -and (Test-Path $EnvFile)) {
        & pwsh -NoProfile -File $secretScan -Repo $Repo -EnvFile $EnvFile *> $null
        $sec = $LASTEXITCODE
    } else {
        & pwsh -NoProfile -File $secretScan -Repo $Repo *> $null
        $sec = $LASTEXITCODE
    }
    if ($sec -eq 0) { Add-Result 'secret-safety scan' 'PASS' 'exit 0' }
    else { Add-Result 'secret-safety scan' 'FAIL' "exit $sec" }
} else {
    Add-Result 'secret-safety scan' 'SKIP' "script not found: $secretScan"
}

# ── 4. workspace 成员 req 对齐（Rust workspace 仓）───────────────
if (Test-Path $wsCargo) {
    $wsVer = $null
    foreach ($line in Get-Content $wsCargo) {
        if ($line -match '^\s*version\s*=\s*"(\d+\.\d+)\.\d+"') { $wsVer = $Matches[1]; break }
    }
    if ($wsVer) {
        $bad = @()
        foreach ($cargo in (Get-ChildItem $Repo -Recurse -Filter Cargo.toml)) {
            if ($cargo.FullName -match '\\target\\|\\fuzz\\') { continue }
            foreach ($line in Get-Content $cargo.FullName) {
                if ($line -match '^\s*evorule-(tcb|reactor|governance|discipline)\s*=\s*\{\s*version\s*=\s*"(\d+)\.') {
                    if ($Matches[2] -ne ($wsVer -split '\.')[0]) {
                        $bad += ('{0}: {1}' -f $cargo.FullName.Substring($Repo.Length + 1), $line.Trim())
                    }
                }
            }
        }
        if ($bad.Count -eq 0) {
            Add-Result 'workspace member req alignment' 'PASS' "evorule-* req 主线 = $((($wsVer -split '\.')[0])).x"
        } else {
            Add-Result 'workspace member req alignment' 'FAIL' ("workspace={0} 但 {1} 处 req 漂移: {2}" -f $wsVer, $bad.Count, ($bad -join ' | '))
        }
    } else {
        Add-Result 'workspace member req alignment' 'SKIP' 'workspace version not found'
    }
} else {
    Add-Result 'workspace member req alignment' 'SKIP' 'no Cargo.toml'
}

# ── 5. CHANGELOG [Unreleased] 残留（发布定版时点）────────────────
$cl = Join-Path $Repo 'CHANGELOG.md'
if (Test-Path $cl) {
    $unrel = Select-String -Path $cl -Pattern '^##\s*\[(Unreleased|未发布)\]' -ErrorAction SilentlyContinue
    if ($unrel) { Add-Result 'CHANGELOG [Unreleased]' 'WARN' '存在 [Unreleased] 段——发布定版时应转正版本段' }
    else { Add-Result 'CHANGELOG [Unreleased]' 'PASS' '零残留' }
} else {
    Add-Result 'CHANGELOG [Unreleased]' 'SKIP' 'no CHANGELOG.md'
}

# ── 汇总 ─────────────────────────────────────────────────────────
Write-Output ('=== 结果: FAIL={0} WARN={1} ===' -f $script:failed, $script:warned)
if ($script:failed -gt 0) { exit 1 }
exit 0
