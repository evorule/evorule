# scripts/install-hooks.ps1
# 门禁自检: 校验本地公开面门禁 hook 在场 + core.hooksPath 未被改指
# (本脚本不安装任何 hook, 只做自检; 钩子本体为 .git/hooks/pre-push 本地工具,
#  不随仓库公开。推前门禁三道门: 公开面 A1/B + 文档安全 + 密钥泄露扫描。)
# 用法: powershell -File scripts/install-hooks.ps1

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

Write-Host "==> evorule 公开面门禁自检..."
$fail = $false

# 1. core.hooksPath 必须未设置(.git/hooks 门禁生效的前提)
$hooksPath = git config --get core.hooksPath 2>$null
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($hooksPath)) {
    Write-Host "    [OK] core.hooksPath 未设置(.git/hooks 门禁生效)"
} else {
    Write-Host "    [FAIL] core.hooksPath = $hooksPath(会旁路 .git/hooks 门禁)" -ForegroundColor Red
    Write-Host "           修复: git config --unset core.hooksPath"
    $fail = $true
}

# 2. .git/hooks/pre-push 门禁必须在场
$prePush = Join-Path '.git/hooks' 'pre-push'
if (Test-Path $prePush) {
    Write-Host "    [OK] $prePush 在场"
} else {
    Write-Host "    [FAIL] $prePush 缺失(门禁休眠)" -ForegroundColor Red
    $fail = $true
}

# 3. 确认无 post-commit 自动推送残留(push 一律人工手动执行)
foreach ($p in @('.git/hooks/post-commit', 'scripts/hooks/post-commit')) {
    if (Test-Path $p) {
        Write-Host "    [FAIL] $p 在场(自动推送应不存在, 推送一律人工)" -ForegroundColor Red
        $fail = $true
    } else {
        Write-Host "    [OK] $p 不存在"
    }
}

# 4. 门禁依赖脚本在场
foreach ($s in @('scripts/scan_public_face.py', 'scripts/check_doc_safety.py', 'scripts/check-push-secret-safety.ps1')) {
    if (Test-Path $s) {
        Write-Host "    [OK] $s 在场"
    } else {
        Write-Host "    [FAIL] $s 缺失(门禁依赖不全)" -ForegroundColor Red
        $fail = $true
    }
}

Write-Host ""
if ($fail) {
    Write-Host "==> 自检未通过, 按上述提示修复。" -ForegroundColor Red
    exit 1
}
Write-Host "==> 自检通过: 门禁在场, 违规内容推前硬拦, 推送保持人工手动。"
