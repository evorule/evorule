# SPDX-License-Identifier: AGPL-3.0-or-later
#!/usr/bin/env pwsh
# validate-release.ps1
# VERSION_STRATEGY.md 4.5, 10.1
# Release-time check: git tag matches + pre-release identifier + tag format
# 各仓独立发布:只校验本仓 git tag,不查兄弟仓
# Exit code: 0 = pass, 1 = fail
#
# 时序说明:
#   - 发布前就绪检查:用 -SkipTagCheck 跳过 tag 存在性检查(此时 tag 尚未创建)
#   - 发布后验证(默认):tag 必须存在,且与 Cargo.toml version 一致

[CmdletBinding()]
param(
    [switch]$SkipTagCheck
)

$ErrorActionPreference = "Stop"
$evoruleRoot = Split-Path -Parent $PSScriptRoot

$semverPattern = '^(\d+)\.(\d+)\.(\d+)(?:-([a-z]+)\.(\d+))?$'
$tagPattern = '^v\d+\.\d+\.\d+(-[a-z]+\.\d+)?$'

$failed = $false
Write-Host "`n=== Release Validation ===" -ForegroundColor Cyan

Push-Location $evoruleRoot
try {
    # 1. Current version
    $currentVersion = (Get-Content "Cargo.toml" | Where-Object { $_ -match '^version\s*=' } | Select-Object -First 1) -replace '.*"([^"]+)".*', '$1'
    Write-Host "[INFO] Current version: $currentVersion" -ForegroundColor Cyan

    if ($currentVersion -notmatch $semverPattern) {
        Write-Host "[FAIL] Current version '$currentVersion' is not valid SemVer" -ForegroundColor Red
        exit 1
    }
    $preRelease = $Matches[4]
    if ($preRelease) {
        Write-Host "[INFO] Pre-release identifier: $preRelease" -ForegroundColor Cyan
    }

    # 2. git tag match — tag 不存在 FAIL(默认);-SkipTagCheck 跳过(发布前就绪检查用)
    if (-not $SkipTagCheck) {
        $tagName = "v$currentVersion"
        $tagExists = git tag --list $tagName
        if ($tagExists) {
            Write-Host "[OK]   git tag '$tagName' exists" -ForegroundColor Green
        } else {
            Write-Host "[FAIL] git tag '$tagName' does not exist — 发布验证要求 tag 已创建(用 'git tag $tagName' 创建;发布前就绪检查用 -SkipTagCheck 跳过)" -ForegroundColor Red
            $failed = $true
        }

        # 新增:不应有比当前版本更大的已发布 tag(说明 Cargo.toml 版本号落后于已发布 tag)
        # 拦住"打了 v0.1.2 tag 但 Cargo.toml 还停在 0.1.1"的问题
        $cm = [int]$Matches[1]; $cn = [int]$Matches[2]; $cp = [int]$Matches[3]
        $allVersionTags = @(git tag --list 'v*' 2>$null | Where-Object { $_ -match '^v\d+\.\d+\.\d+$' })
        $futureTags = @()
        foreach ($t in $allVersionTags) {
            if ($t -match '^v(\d+)\.(\d+)\.(\d+)$') {
                $tm = [int]$Matches[1]; $tn = [int]$Matches[2]; $tp = [int]$Matches[3]
                $tagGreater = $false
                if ($tm -gt $cm) { $tagGreater = $true }
                elseif ($tm -eq $cm -and $tn -gt $cn) { $tagGreater = $true }
                elseif ($tm -eq $cm -and $tn -eq $cn -and $tp -gt $cp) { $tagGreater = $true }
                if ($tagGreater) { $futureTags += $t }
            }
        }
        if ($futureTags.Count -gt 0) {
            Write-Host "[FAIL] 存在比 Cargo.toml version ($cm.$cn.$cp) 更大的已发布 tag: $($futureTags -join ', ') — Cargo.toml 版本号落后于已发布 tag" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   无比当前版本更大的已发布 tag(版本号未落后)" -ForegroundColor Green
        }
    } else {
        Write-Host "[INFO] -SkipTagCheck set, skipping tag existence/consistency check (pre-release readiness)" -ForegroundColor DarkGray
    }

    # 3. Version tags format check (only check 'v'-prefixed tags)
    # Non-version tags (e.g. checkpoint-*) are development markers, not releases
    $allTags = @(git tag --list 2>$null)
    if ($allTags.Count -eq 0) {
        Write-Host "[INFO] No git tags yet" -ForegroundColor Cyan
    } else {
        $versionTags = @($allTags | Where-Object { $_ -match '^v' })
        $nonVersionTags = @($allTags | Where-Object { $_ -notmatch '^v' })
        if ($nonVersionTags.Count -gt 0) {
            Write-Host "[INFO] Non-version tags (ignored): $($nonVersionTags -join ', ')" -ForegroundColor DarkGray
        }
        if ($versionTags.Count -eq 0) {
            Write-Host "[INFO] No version tags (v*) yet" -ForegroundColor Cyan
        } else {
            $badTags = @($versionTags | Where-Object { $_ -notmatch $tagPattern })
            if ($badTags.Count -gt 0) {
                Write-Host "[FAIL] Bad version tag format: $($badTags -join ', ')" -ForegroundColor Red
                $failed = $true
            } else {
                Write-Host "[OK]   All $($versionTags.Count) version tags have 'vX.Y.Z' format" -ForegroundColor Green
            }
        }
    }

    # 4. Status
    if (-not $preRelease) {
        Write-Host "[INFO] Stable version, ready for release" -ForegroundColor Cyan
    } else {
        Write-Host "[INFO] Pre-release, do NOT publish to crates.io/npm/PyPI until stable" -ForegroundColor Yellow
    }

    # 5. GitHub workflow syntax lint (fail-closed)
    #    2026-10-13 发版实证: release.yml 的 build job 丢 runs-on 后 workflow
    #    深度解析失败, tag 触发的 Release run 静默缺位(无 run 无 suite), 而
    #    branch push 仍产 0-job phantom run 掩蔽损坏——仓内无 lint 时该缺陷
    #    潜伏至发版才显形。此处对全部 workflows 做解析 + 必填字段断言
    #    (每个 job 必须有 runs-on 且 steps 非空), 防同类手误再入 main。
    $wfDir = Join-Path $evoruleRoot ".github\workflows"
    $wfFiles = @()
    if (Test-Path $wfDir) {
        $wfFiles = @(Get-ChildItem $wfDir -Filter *.yml) + @(Get-ChildItem $wfDir -Filter *.yaml -ErrorAction SilentlyContinue)
    }
    if ($wfFiles.Count -eq 0) {
        Write-Host "[INFO] No workflow files found under .github/workflows" -ForegroundColor DarkGray
    } else {
        $pyCode = @'
import sys, yaml
bad = []
for p in sys.argv[1:]:
    try:
        with open(p, encoding="utf-8") as f:
            d = yaml.safe_load(f)
    except Exception as e:
        bad.append("%s: parse error: %s" % (p, e))
        continue
    if not isinstance(d, dict) or "jobs" not in d:
        bad.append("%s: missing jobs mapping" % p)
        continue
    for name, job in d["jobs"].items():
        if not isinstance(job, dict):
            bad.append("%s: job %s is not a mapping" % (p, name))
            continue
        if "runs-on" not in job:
            bad.append("%s: job %s: missing runs-on" % (p, name))
        steps = job.get("steps")
        if not isinstance(steps, list) or len(steps) == 0:
            bad.append("%s: job %s: missing/empty steps" % (p, name))
if bad:
    print("\n".join(bad))
    sys.exit(1)
print("all workflows parse OK; every job has runs-on and non-empty steps")
'@
        $pyOut = $pyCode | python - @($wfFiles.FullName) 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host "[FAIL] GitHub workflow lint:" -ForegroundColor Red
            $pyOut | ForEach-Object { Write-Host "       $_" -ForegroundColor Red }
            $failed = $true
        } else {
            Write-Host "[OK]   $($wfFiles.Count) workflow files lint clean (parse + runs-on + steps)" -ForegroundColor Green
        }
    }
} finally {
    Pop-Location
}

if ($failed) { Write-Host "`n[RESULT] FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
