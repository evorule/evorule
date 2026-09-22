# check-dep-anchors.ps1 — evorule 生态依赖锚点结构不变量核验
# 依据：《专项-evorule生态仓全景基线-20260921.md》§6/§9（工作约束绑定，2026-09-21 项目方令）
# 层级规则（设计/实施/核验三阶段共用的机器化表达）：
#   A1 evo-agent 零主仓 evorule-* 依赖（O-044；独立仓治理组件走显式 allowlist）
#   A2 evorule-hash 纯原语（零家族依赖、零 I/O 框架）
#   A3 evorule-bundle 集装箱纯度（家族仅许 hash、零 I/O 框架）
#   A4 非主仓活仓禁 path 指向主仓布局、禁 git 直连主仓仓 URL（registry 化退役原则恒久化）
#   A5 server 家族锁（tcb/reactor/governance 同仓单版本且互等）
#   A6 跨仓 evorule-bundle 版本一致（两域契约）
#   A7 console-cloud 运行时单依赖（@noble/hashes）
#   W1 server→rule git pin 与 rule 本地 HEAD 漂移提示（WARN，不 FAIL）
# 冻结豁免：evorule-agent（gitee evorulelab/evorule-agent，私有姊妹项目）2026-09-21 项目方令冻结，
#   暂不发展、不入核验范围——其 tcb/reactor path 依赖为冻结时历史原貌，不作为违反项。
# 用法：pwsh -NoProfile -File scripts\check-dep-anchors.ps1 [-Root D:\]
# 退出码：0=PASS，1=FAIL。主仓自身（workspace 自治）与周边 4 仓（sdk/application/dsh-skill/experience-pack，待纳管）不在扫描范围。

param([string]$Root = "D:\")

$ErrorActionPreference = 'Stop'
$findings = New-Object System.Collections.Generic.List[string]
$warns = New-Object System.Collections.Generic.List[string]
function Fail($m) { $script:findings.Add($m) }
function Warn($m) { $script:warns.Add($m) }

$repos = @{
    'agent'   = Join-Path $Root 'evo-agent'
    'bundle'  = Join-Path $Root 'evorule-bundle'
    'hash'    = Join-Path $Root 'evorule-hash'
    'server'  = Join-Path $Root 'evorule-server'
    'rule'    = Join-Path $Root 'evorule-rule'
    'console' = Join-Path $Root 'evorule-console'
    'cloud'   = Join-Path $Root 'evorule-console-cloud'
    'sysrule' = Join-Path $Root 'evorule-system-rules'
}

function Get-Manifests([string]$dir) {
    if (-not (Test-Path $dir)) { return @() }
    return @(Get-ChildItem $dir -Recurse -Filter Cargo.toml -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\\target\\|\\\.git\\|\\node_modules\\' })
}

# ---- A1 evo-agent 零主仓家族依赖 ----
# O-044 (2026-09-20): evo-agent 与主仓 decoupled，数据面仅 HTTP；ban 对象=主仓
# crates（path OR version 依赖均禁）。独立仓治理组件走显式 allowlist（2026-09-22
# 宪法 crate 收编精确化，与 evo-agent verify.ps1 [4/4] 同口径——两处 allowlist
# 须同步维护）：evorule-constitution（宪法仓 crates/，schema 校验，0.2.0 起
# 编译期内嵌；git 依赖 + rev 钉版消费）。
$agentDepAllowlist = @('evorule-constitution')
$agentManifests = Get-Manifests $repos['agent']
foreach ($f in $agentManifests) {
    foreach ($ln in (Get-Content $f.FullName)) {
        if ($ln -match '^\s*(evorule-[A-Za-z0-9_-]+)\s*=' -and $agentDepAllowlist -notcontains $Matches[1]) {
            Fail ("A1 evo-agent 主仓家族依赖: {0}: {1}" -f $f.FullName, $ln.Trim())
        }
    }
}
$agentLock = Join-Path $repos['agent'] 'Cargo.lock'
if (Test-Path $agentLock) {
    foreach ($hit in (Select-String -Path $agentLock -Pattern '^name = "(evorule-[A-Za-z0-9_-]+)"')) {
        if ($agentDepAllowlist -notcontains $hit.Matches[0].Groups[1].Value) {
            Fail ("A1 evo-agent Cargo.lock 家族包: " + $hit.Line)
        }
    }
}

# ---- A2/A3 hash 纯原语 + bundle 集装箱纯度 ----
foreach ($f in (Get-Manifests $repos['hash'])) {
    foreach ($ln in (Get-Content $f.FullName)) {
        if ($ln -match '^\s*evorule-[A-Za-z0-9_-]+\s*=') { Fail ("A2 hash 家族依赖: {0}: {1}" -f $f.FullName, $ln.Trim()) }
        if ($ln -match '^\s*(tokio|axum|reqwest|hyper|warp|sqlx|rusqlite|ureq)\s*=') { Fail ("A2 hash I/O 框架依赖: {0}: {1}" -f $f.FullName, $ln.Trim()) }
    }
}
foreach ($f in (Get-Manifests $repos['bundle'])) {
    foreach ($ln in (Get-Content $f.FullName)) {
        if ($ln -match '^\s*evorule-(?!hash\b)[A-Za-z0-9_-]+\s*=') { Fail ("A3 bundle 越界家族依赖（仅许 evorule-hash）: {0}: {1}" -f $f.FullName, $ln.Trim()) }
        if ($ln -match '^\s*(tokio|axum|reqwest|hyper|warp|sqlx|rusqlite|ureq)\s*=') { Fail ("A3 bundle I/O 框架依赖: {0}: {1}" -f $f.FullName, $ln.Trim()) }
    }
}

# ---- A4 非主仓禁指主仓（path 布局 / git 主仓 URL）----
foreach ($key in @('agent', 'bundle', 'hash', 'server', 'rule', 'console', 'cloud', 'sysrule')) {
    foreach ($f in (Get-Manifests $repos[$key])) {
        foreach ($ln in (Get-Content $f.FullName)) {
            if ($ln -match 'path\s*=\s*"[^"]*(\.\./evorule/|[A-Za-z]:[\\/]+evorule)') {
                Fail ("A4 path 依赖指向主仓布局: {0}: {1}" -f $f.FullName, $ln.Trim())
            }
            if ($ln -match 'git\s*=\s*"[^"]*(gitee\.com[:/]evorule/evorule(?:\.git)?["'']|github\.com[:/]evorule/evorule(?:\.git)?["''])') {
                Fail ("A4 git 依赖直连主仓仓 URL（应走 crates.io）: {0}: {1}" -f $f.FullName, $ln.Trim())
            }
        }
    }
}

# ---- A5 server 家族锁：tcb/reactor/governance 单版本且互等 ----
$famVersions = @{}
foreach ($f in (Get-Manifests $repos['server'])) {
    foreach ($hit in (Select-String -Path $f.FullName -Pattern '^\s*evorule-(tcb|reactor|governance)\b.*?version\s*=\s*"([^"]+)"')) {
        $crate = $hit.Matches[0].Groups[1].Value
        $ver = $hit.Matches[0].Groups[2].Value
        if (-not $famVersions.ContainsKey($crate)) { $famVersions[$crate] = New-Object System.Collections.Generic.HashSet[string] }
        [void]$famVersions[$crate].Add($ver)
    }
}
if ($famVersions.Count -eq 0) {
    Warn "A5 server 未解析到家族版本锚（manifest 格式漂移？人工核对）"
}
else {
    foreach ($k in $famVersions.Keys) {
        if ($famVersions[$k].Count -gt 1) { Fail ("A5 server {0} 同仓多版本: {1}" -f $k, ($famVersions[$k] -join ' vs ')) }
    }
    $distinct = ($famVersions.Values | ForEach-Object { $_ } | Sort-Object -Unique)
    if ($distinct.Count -gt 1) { Fail ("A5 server 家族版本不互等: " + ($distinct -join ' vs ')) }
}

# ---- A6 跨仓 evorule-bundle 版本一致（server vs rule）----
$bundleVers = New-Object System.Collections.Generic.HashSet[string]
foreach ($key in @('server', 'rule')) {
    foreach ($f in (Get-Manifests $repos[$key])) {
        foreach ($hit in (Select-String -Path $f.FullName -Pattern '^\s*evorule-bundle\b.*?version\s*=\s*"([^"]+)"')) {
            [void]$bundleVers.Add($hit.Matches[0].Groups[1].Value)
        }
    }
}
if ($bundleVers.Count -gt 1) { Fail ("A6 跨仓 evorule-bundle 版本不一致: " + ($bundleVers -join ' vs ')) }

# ---- A7 console-cloud 运行时单依赖 ----
$pkgPath = Join-Path $repos['cloud'] 'package.json'
if (Test-Path $pkgPath) {
    $pkg = Get-Content $pkgPath -Raw | ConvertFrom-Json
    $depNames = @()
    if ($pkg.dependencies) { $depNames = @($pkg.dependencies.PSObject.Properties.Name) }
    if ($depNames.Count -ne 1 -or $depNames[0] -ne '@noble/hashes') {
        Fail ("A7 console-cloud 运行时依赖偏离单依赖纪律(@noble/hashes): " + ($depNames -join ', '))
    }
}
else { Warn "A7 console-cloud package.json 未找到" }

# ---- W1 server→rule git pin 漂移提示 ----
$serverMain = Join-Path $repos['server'] 'evorule-server\Cargo.toml'
$ruleHead = git -C $repos['rule'] rev-parse HEAD 2>$null
if ((Test-Path $serverMain) -and $ruleHead) {
    foreach ($hit in (Select-String -Path $serverMain -Pattern 'evorule-rule\b.*?rev\s*=\s*"([0-9a-f]{40})"')) {
        $pin = $hit.Matches[0].Groups[1].Value
        if (-not $ruleHead.StartsWith($pin.Substring(0, 12))) {
            Warn ("W1 server→rule git pin {0} 落后 rule 本地 HEAD {1}（跨仓 e2e 通道对账提示）" -f $pin.Substring(0, 12), $ruleHead.Substring(0, 12))
        }
    }
}

# ---- 输出 ----
Write-Host "=== check-dep-anchors（依据 全景基线-20260921 §6/§9）==="
foreach ($w in $warns) { Write-Host ("WARN: " + $w) }
if ($findings.Count -gt 0) {
    foreach ($m in $findings) { Write-Host ("FAIL: " + $m) }
    Write-Host ("RESULT: FAIL ({0} 项违反依赖锚点结构不变量)" -f $findings.Count)
    exit 1
}
Write-Host "RESULT: PASS — 8 仓结构不变量全部成立"
exit 0
