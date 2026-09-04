# SPDX-License-Identifier: AGPL-3.0-or-later
param(
    [switch]$Quiet
)
$ErrorActionPreference = 'Stop'
trap {
    Write-Host ("TRAP EXCEPTION: " + $_.Exception.GetType().FullName + " :: " + $_.Exception.Message)
    if ($_.InvocationInfo) {
        Write-Host ("TRAP INVOCATION: Line=" + $_.InvocationInfo.ScriptLineNumber + " :: " + $_.InvocationInfo.Line)
    }
    Write-Host ("TRAP STACK: " + $_.ScriptStackTrace)
    exit 99
}

$evoruleRoot = Split-Path -Parent $PSScriptRoot
$semverPattern = '^(\d+)\.(\d+)\.(\d+)(?:-([a-z]+)\.(\d+))?$'

function Get-TomlVersion {
    param([Parameter(Mandatory=$false)][AllowNull()][string]$Path)
    if ([string]::IsNullOrEmpty($Path)) { return $null }
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $lines = Get-Content -LiteralPath $Path
    # 优先识别显式 version = "X.Y.Z"
    $line = $lines | Where-Object { $_ -match '^\s*version\s*=\s*"' } | Select-Object -First 1
    if ($line -and $line -match '"([^"]+)"') { return $Matches[1] }
    # 识别 version.workspace = true(继承 workspace 版本,视为与 workspace 一致)
    $wsLine = $lines | Where-Object { $_ -match '^\s*version\.workspace\s*=\s*true' } | Select-Object -First 1
    if ($wsLine) { return '::workspace::' }
    return $null
}
function Get-JsonVersion {
    param([Parameter(Mandatory=$false)][AllowNull()][string]$Path)
    if ([string]::IsNullOrEmpty($Path)) { return $null }
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $line = Get-Content -LiteralPath $Path | Where-Object { $_ -match '"version"\s*:' } | Select-Object -First 1
    if ($line -and $line -match '"version"\s*:\s*"([^"]+)"') { return $Matches[1] }
    return $null
}

# 各仓独立发布:仅校验本仓(evorule)版本源,不查兄弟仓
$projects = [ordered]@{}
$projects['evorule-workspace']  = (Join-Path $evoruleRoot "Cargo.toml")
$projects['evorule-tcb']        = (Join-Path $evoruleRoot "evorule-tcb\Cargo.toml")
$projects['evorule-reactor']    = (Join-Path $evoruleRoot "evorule-reactor\Cargo.toml")
$projects['evorule-governance'] = (Join-Path $evoruleRoot "evorule-governance\Cargo.toml")
$projects['evorule-cli']        = (Join-Path $evoruleRoot "evorule-cli\Cargo.toml")
# 注:SDK 已物理分离到独立仓(evorule-sdk),本仓不再校验 sdk-typescript / sdk-python 版本

$versions = [ordered]@{}
$failed = $false
if (-not $Quiet) { Write-Host "`n=== Version Validation ===" -ForegroundColor Cyan }
foreach ($name in $projects.Keys) {
    $p = $projects[$name]
    if ($name -like '*sdk-typescript*' -or $name -like '*sdk-python*') {
        $v = Get-JsonVersion $p
    } else {
        $v = Get-TomlVersion $p
    }
    # 注:SDK 已分离,上述 sdk 分支保留仅为兼容,本仓 projects 已无 sdk 条目
    $versions[$name] = $v
    if ($null -eq $v) {
        Write-Host "[SKIP] $name : version not found (file may not exist)" -ForegroundColor Yellow
        continue
    }
    if ($v -eq '::workspace::') {
        Write-Host "[OK]   $name : workspace inherited (version.workspace = true)" -ForegroundColor Green
        continue
    }
    if ($v -notmatch $semverPattern) {
        Write-Host "[FAIL] $name : '$v' is not valid SemVer 2.0" -ForegroundColor Red
        $failed = $true
    } else {
        Write-Host "[OK]   $name : $v" -ForegroundColor Green
    }
}

$canonicalVersion = $versions['evorule-workspace']

# 本仓内部 crate 必须与 workspace 完全一致(FULL version 比较,含 PATCH)
if ($canonicalVersion -and $canonicalVersion -match $semverPattern) {
    Write-Host ""
    foreach ($name in @('evorule-tcb','evorule-reactor','evorule-governance','evorule-cli')) {
        $v = $versions[$name]
        if ($v) {
            if ($v -eq '::workspace::') {
                Write-Host "[OK]   $name : workspace inherited (== workspace $canonicalVersion)" -ForegroundColor Green
            } elseif ($v -ne $canonicalVersion) {
                Write-Host "[FAIL] $name ($v) != workspace ($canonicalVersion) — 本仓内部 crate 必须与 workspace 版本完全一致(含 PATCH)" -ForegroundColor Red
                $failed = $true
            } else {
                Write-Host "[OK]   $name ($v) == workspace" -ForegroundColor Green
            }
        }
    }
}

# MAJOR 一致性(本仓所有项目)
$parsedMajors = @()
foreach ($name in $versions.Keys) {
    $v = $versions[$name]
    if ($v -and $v -match $semverPattern) { $parsedMajors += [int]$Matches[1] }
}
$uniqueMajors = $parsedMajors | Sort-Object -Unique
if ($uniqueMajors.Count -gt 1) {
    Write-Host "`n[FAIL] MAJOR mismatch: $($uniqueMajors -join ', ')" -ForegroundColor Red
    $failed = $true
} elseif ($uniqueMajors.Count -eq 1) {
    Write-Host "`n[OK]   All projects share MAJOR = $($uniqueMajors[0])" -ForegroundColor Green
}

# npm lockfile
$npmLockfile = Join-Path $evoruleRoot "sdk\typescript\package-lock.json"
if (Test-Path -LiteralPath $npmLockfile) {
    $lockVersion = Get-JsonVersion $npmLockfile
    $pkgVersion = $versions['sdk-typescript']
    if ($lockVersion -and $pkgVersion -and $lockVersion -ne $pkgVersion) {
        Write-Host "[FAIL] sdk-typescript lockfile: version '$lockVersion' != package.json '$pkgVersion'" -ForegroundColor Red
        $failed = $true
    } elseif ($lockVersion -and $pkgVersion) {
        Write-Host "[OK]   sdk-typescript lockfile: $lockVersion (matches package.json)" -ForegroundColor Green
    }
}

# retired patterns (v6.x / v7.0 历史废弃版本号)
$retiredPatterns = @('v6\.0','v6\.1','v6\.2','v7\.0','6\.0\.0')
$docFiles = @(
    @{ Path = (Join-Path $evoruleRoot "README.md"); Name = 'README.md' },
    @{ Path = (Join-Path $evoruleRoot "CONTRIBUTING.md"); Name = 'CONTRIBUTING.md' },
    @{ Path = (Join-Path $evoruleRoot "VERSION_STRATEGY.md"); Name = 'VERSION_STRATEGY.md' }
)
$retiredFound = $false
foreach ($doc in $docFiles) {
    if ([string]::IsNullOrEmpty($doc.Path) -or -not (Test-Path -LiteralPath $doc.Path)) { continue }
    $lines = Get-Content -LiteralPath $doc.Path -Encoding UTF8
    for ($i = 0; $i -lt $lines.Count; $i++) {
        # v0.4.1 完善门禁: 跳过描述退休扫描本身的自引用行
        if ($lines[$i] -match '历史废弃模式扫描|等已退役模式') { continue }
        foreach ($pattern in $retiredPatterns) {
            if ($lines[$i] -match $pattern) {
                Write-Host "[FAIL] $($doc.Name):$($i+1) contains retired version pattern '$pattern': $($lines[$i].Trim())" -ForegroundColor Red
                $failed = $true; $retiredFound = $true
            }
        }
    }
}
if (-not $retiredFound) { Write-Host "[OK]   No retired version references (v6.x/v7.0) in docs" -ForegroundColor Green }

# === 通用 L1 文档版本号扫描(替代硬编码 4e~4h)===
# 扫描所有 L1 .md 中的 v\d+\.\d+\.\d+ 字面量,与 Cargo.toml canonical 不一致即 FAIL
# 白名单:
#   - CHANGELOG.md(历史段含多个版本)
#   - 废弃文档(顶部 [已废弃] 横幅)
#   - 审计/威胁模型文档(文件名含 AUDIT/THREAT_MODEL,审计版本与代码版本独立)
#   - 未来版本(>canonical,如路线图 0.2.0/1.0.0)
#   - canonical 自身
# 注:\b 保证 _v0.1.0.md(文件名引用)不被误匹配
if ($canonicalVersion -and $canonicalVersion -match '^(\d+)\.(\d+)\.(\d+)$') {
    $cm = [int]$Matches[1]; $cn = [int]$Matches[2]; $cp = [int]$Matches[3]
    if (-not $Quiet) { Write-Host "`n=== L1 Document Version Scan (canonical = v$canonicalVersion) ===" -ForegroundColor Cyan }

    $l1Files = @()
    $l1Files += Get-ChildItem -LiteralPath $evoruleRoot -Filter *.md -File -ErrorAction SilentlyContinue
    $docsDir = Join-Path $evoruleRoot "docs"
    if (Test-Path -LiteralPath $docsDir) {
        $l1Files += Get-ChildItem -LiteralPath $docsDir -Recurse -Filter *.md -File -ErrorAction SilentlyContinue
    }
    foreach ($crate in @('evorule-tcb','evorule-reactor','evorule-governance','evorule-cli')) {
        $crateDir = Join-Path $evoruleRoot $crate
        if (Test-Path -LiteralPath $crateDir) {
            $l1Files += Get-ChildItem -LiteralPath $crateDir -Filter *.md -File -ErrorAction SilentlyContinue
        }
    }

    # 匹配 v\d+\.\d+\.\d+ 但排除文件名引用(如 SECURITY_AUDIT_v0.1.0.md 中的 _v0.1.0)
    # 负向后行断言: v 前面不能是字母或下划线(否则是文件名/标识符的一部分)
    # v1.9 补盲区:除 v 前缀(v0.2.2)外,也匹配无 v 前缀的 version = "X.Y.Z" 写法
    # (如 DOCS_INDEX 的 `version = "0.2.0"`),避免此类"写死版本号"逃过门禁
    $versionLiteralPattern = '(?<![a-zA-Z_])v(\d+\.\d+\.\d+)\b|version\s*=\s*"(\d+\.\d+\.\d+)"'
    # === v0.2.2 引入: 历史性引用白名单 ===
    # 1) 文件名匹配 MIGRATION_v*.md / RELEASE_PROCESS_v*.md → 整个文件跳过(文档本身讲特定版本迁移/发布流程)
    # 2) 文档版本表行 → 行内匹配 "基于 evorule-core-backup" 或 "| X.Y | YYYY-MM-DD |" 表格行格式
    # 3) 历史性描述行 → 行内同时含 v\d.\d.\d 和以下关键词之一: 重构/下沉/已移除/未实现/迁移/达标条件/边界再调整/从 governance/已废弃/已发布/迁移指南/破坏性变更/路线图规划
    $docVersionTableRowPattern = '\|\s*\d+\.\d+\s*\|\s*\d{4}-\d{2}-\d{2}\s*\|'
    $historyKeywordPattern = '重构|下沉|已移除|未实现|迁移|达标条件|边界再调整|从 governance|已废弃|已发布|迁移指南|破坏性变更|路线图规划|初版|自\s*v\d+\.\d+\.\d+\s*起|新增|撤销|已删除|移除|修正|收紧|规范化|升级|落地|补入|合并|回滚|基础仓|在 vault|审计治理|历史说明|审计版|新设计|旧版|早期规划|性能基准|文档系统|验证设计|格式说明|重放契约|当前实现|当前状态|宪法|命名约定|仅保留|代码量目标|是历史对比'
    $scanFailed = $false
    foreach ($f in $l1Files) {
        $relName = $f.FullName.Substring($evoruleRoot.Length + 1)
        # CHANGELOG 白名单:根 CHANGELOG.md + 子 crate CHANGELOG.md(历史段含多版本,合法)
        if ($relName -match 'CHANGELOG\.md$') { continue }
        $content = [System.IO.File]::ReadAllText($f.FullName, [System.Text.Encoding]::UTF8)
        # 废弃文档跳过
        $headLen = [Math]::Min(2000, $content.Length)
        if ($content.Substring(0, $headLen) -match '\[已废弃\]') { continue }
        # 审计/威胁模型文档跳过(版本绑定审计批次)
        # SECURITY.md 含版本支持表(历史边界声明如 < v0.1.0-alpha.1,合法)
        if ($relName -match 'AUDIT|THREAT_MODEL|^SECURITY\.md$') { continue }
        # === v0.2.2 新增:迁移指南/发布流程文档 → 整文件跳过 ===
        # MIGRATION_v0.2.0.md 讲 v0.1.x → v0.2.0 迁移, RELEASE_PROCESS_v0.1.1.md 讲 v0.2.0 发布流程示例
        if ($relName -match 'MIGRATION_v\d+\.\d+\.\d+\.md$|RELEASE_PROCESS_v\d+\.\d+\.\d+\.md$|CHANGE_REQUEST\.md$|DETERMINISM_REPORT\.md$|PERFORMANCE_BASELINE_[Vv]\d+\.\d+\.\d+\.md$') { continue }

        $lines = $content -split "`r?`n"
        # v0.4.1 完善门禁: 预计算代码围栏行集合(围栏内 version="X.Y.Z" 为示例/用户脚手架内容)
        $fenceLines = @{}
        $inFence = $false
        for ($fi = 0; $fi -lt $lines.Count; $fi++) {
            if ($lines[$fi] -match '^\s*```') { $inFence = -not $inFence }
            $fenceLines[$fi] = $inFence
        }
        $seen = @{}
        foreach ($m in [regex]::Matches($content, $versionLiteralPattern)) {
            # v1.9:交替模式两组捕获(v 前缀 / version="X.Y.Z"),取非空者
            $ver = $m.Groups[1].Value
            if (-not $ver) { $ver = $m.Groups[2].Value }
            if ($seen.ContainsKey($ver)) { continue }
            $seen[$ver] = $true
            if ($ver -eq $canonicalVersion) { continue }
            # 未来版本允许(路线图/版本语义表)
            if ($ver -match '^(\d+)\.(\d+)\.(\d+)$') {
                $fm = [int]$Matches[1]; $fn = [int]$Matches[2]; $fp = [int]$Matches[3]
                # 未来版本允许(路线图/版本语义表/回滚流程的下一个版本)
                # FULL version 比较:任何 > canonical 的版本都算未来版本(含 PATCH 递增,如 0.1.1→0.1.2)
                $isFuture = ($fm -gt $cm) -or
                            ($fm -eq $cm -and $fn -gt $cn) -or
                            ($fm -eq $cm -and $fn -eq $cn -and $fp -gt $cp)
                if ($isFuture) { continue }
            }
            # === v0.2.2 新增:历史性引用白名单(按行上下文判断) ===
            # 定位匹配所在行,检查该行是否为文档版本表行或历史性描述行
            $matchStart = $m.Index
            $lineStart = $content.LastIndexOf("`n", $matchStart)
            if ($lineStart -lt 0) { $lineStart = 0 } else { $lineStart++ }
            $lineEnd = $content.IndexOf("`n", $matchStart)
            if ($lineEnd -lt 0) { $lineEnd = $content.Length }
            $lineText = $content.Substring($lineStart, $lineEnd - $lineStart)
            # 文档版本表行: "| 1.0 | 2026-07-19 | 初版,基于 evorule-core-backup v0.2.0-beta ..."
            if ($lineText -match $docVersionTableRowPattern -or $lineText -match '基于 evorule-core-backup') { continue }
            # 历史性描述行: 同行同时含 v0.2.X 和历史关键词
            if ($lineText -match $historyKeywordPattern) { continue }
            # === v0.4.1 完善门禁: 历史锚点白名单 ===
            # 版本章节标题/演进日志(如 "## v0.3.2 更新")
            if ($lineText -match '^\s*#{1,6}\s+v\d+\.\d+\.\d+') { continue }
            # doc 自身版本头(点态注解): "- **版本**:v0.3.2" / "> **版本**: v0.3.2"
            if ($lineText -match '^\s*[>\-\s*]*\*\*版本\*\*\s*[:：]\s*v?\d+\.\d+\.\d+') { continue }
            # 括号或破折号内地版本戳(溯源标注): "(v0.3.1)" / "（v0.3.1新增）" / "— v0.3.1"
            $esc = [regex]::Escape($ver)
            if ($lineText -match '[（(]v?' + $esc + '[）)：:]' -or $lineText -match '—\s*v?' + $esc + '\b') { continue }
            # v0.4.1 完善门禁: 代码围栏内的 version="X.Y.Z" 是示例内容(如教程用户脚手架 crate),跳过
            if ($m.Groups[2].Success) {
                $mLineIdx = [regex]::Matches($content.Substring(0, $m.Index), "`n").Count
                if ($fenceLines[$mLineIdx]) { continue }
            }


            Write-Host "[FAIL] $relName contains 'v$ver' (expected v$canonicalVersion or future version)" -ForegroundColor Red
            $failed = $true; $scanFailed = $true
        }
    }
    if (-not $scanFailed) {
        Write-Host "[OK]   L1 docs contain no stale version literals (CHANGELOG/废弃/审计/未来版本 白名单)" -ForegroundColor Green
    }
}

if ($failed) {
    Write-Host "`n[RESULT] FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
