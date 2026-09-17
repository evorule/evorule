# EvoRule 文档门禁一键串跑（本地全量校验入口）
#
# 串跑全部纯文本文档门禁，任何代码修改/文档生成/更新提交前后均可即时校验，
# 不依赖事后专项检查。CI 中各检查由 .github/workflows/ci.yml 独立 job 执行；
# 提交期的增量快查由本地 pre-commit hook 承接（--files 模式）。
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File scripts/run_doc_gates.ps1
# 退出码:
#   0 = 全部通过; 1 = 存在违规（以最后一个失败项为准）

$ErrorActionPreference = "Continue"
$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

# 提升中文输出兼容性
try {
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    $OutputEncoding = [System.Text.Encoding]::UTF8
} catch {}

$checks = @(
    @{ Name = "文档安全合规 (R 系, check_doc_safety)";           Script = "check_doc_safety.py" },
    @{ Name = "验证状态同步 (S 系, check_status_sync)";          Script = "check_status_sync.py" },
    @{ Name = "ASSURANCE 规范合规 (T 系, check_assurance_compliance)"; Script = "check_assurance_compliance.py" },
    @{ Name = "docs 中英双语完整性 (check_docs_bilingual)";      Script = "check_docs_bilingual.py" }
)

# L1-1 错误码 i18n 对照需要兄弟仓快照（CI 由 workflow checkout；本地存在时才跑）
$serverRs = "sibling/evorule-server/evorule-server/src/api/server.rs"
$consoleDir = "sibling/evorule-console-cloud/src/lib/locale"
if ((Test-Path $serverRs) -and (Test-Path $consoleDir)) {
    $checks += @{ Name = "错误码 i18n 对照 (L1-1, check_error_code_i18n)";
        Script = "check_error_code_i18n.py";
        Args = @("--server-rs", $serverRs, "--console-dir", $consoleDir) }
} else {
    Write-Host "[skip] 错误码 i18n 对照: 未发现 sibling/ 兄弟仓快照 (CI 会跑)" -ForegroundColor DarkGray
}

$fail = 0
foreach ($c in $checks) {
    Write-Host ""
    Write-Host "===== 文档门禁: $($c.Name) =====" -ForegroundColor Cyan
    $script = Join-Path (Join-Path $repoRoot "scripts") $c.Script
    $cArgs = @()
    if ($c.ContainsKey("Args")) { $cArgs = $c.Args }
    python $script @cArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $($c.Name)" -ForegroundColor Red
        $fail = 1
    }
}

Write-Host ""
if ($fail -ne 0) {
    Write-Host "文档门禁存在违规, 提交前请修复 (小项直接修 / 规范性偏离登记 STATUS.md §3.2 DEV-x)。" -ForegroundColor Red
} else {
    Write-Host "文档门禁全部通过。" -ForegroundColor Green
}
exit $fail
