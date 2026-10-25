# 构建单文件 HTML 验证器：wasm release 构建 -> base64 -> 注入模板 -> dist/index.html
# 用法: powershell -ExecutionPolicy Bypass -File build-dist.ps1  （在本 crate 目录执行）
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

cargo build -p evid-verify-wasm --target wasm32-unknown-unknown --release
if ($LASTEXITCODE -ne 0) { throw "wasm 构建失败" }

$wasmPath = Join-Path $PSScriptRoot "..\target\wasm32-unknown-unknown\release\evid_verify_wasm.wasm"
if (-not (Test-Path $wasmPath)) { throw "未找到 wasm 产物: $wasmPath" }
$b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($wasmPath))

$templatePath = Join-Path $PSScriptRoot "assets\verifier.html"
$html = [IO.File]::ReadAllText($templatePath, [Text.Encoding]::UTF8)
if (-not $html.Contains("__WASM_BASE64__")) { throw "模板缺少占位符 __WASM_BASE64__" }
$html = $html.Replace("__WASM_BASE64__", $b64)

$outDir = Join-Path $PSScriptRoot "dist"
New-Item -ItemType Directory -Force $outDir | Out-Null
$outPath = Join-Path $outDir "index.html"
# 双参 WriteAllText：.NET 默认即 UTF-8 无 BOM
[IO.File]::WriteAllText($outPath, $html)
if (-not (Test-Path $outPath)) { throw "dist/index.html 写入失败" }
"OK: $outPath ($([math]::Round((Get-Item $outPath).Length/1KB)) KB)"
