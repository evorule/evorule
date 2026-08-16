# fix-corrupted-lines.ps1
# Recovery: the fix-missing-spdx.ps1 script corrupted evo-agent files
# because Get-Content -Raw decoded UTF-8 files (no BOM) as GBK,
# which replaced \n bytes between multi-byte Chinese and ASCII with 0x3F (?).
#
# This script replaces the corruption marker 0x3F with 0x0A (\n) wherever
# it appears between non-ASCII bytes and ASCII whitespace -- a heuristic
# that recovers valid Rust syntax. The garbled Chinese text in doc comments
# remains (cannot be recovered without a backup).

$ErrorActionPreference = "Stop"

$targets = @(
    "D:\evo-agent\src\io_dispatcher.rs",
    "D:\evo-agent\src\io_handler.rs",
    "D:\evo-agent\src\json_convert.rs",
    "D:\evo-agent\src\lib.rs",
    "D:\evo-agent\src\agent\definition.rs",
    "D:\evo-agent\src\agent\delegate.rs",
    "D:\evo-agent\src\agent\memory.rs",
    "D:\evo-agent\src\agent\mod.rs",
    "D:\evo-agent\src\agent\runner.rs",
    "D:\evo-agent\src\agent\tool_registry.rs",
    "D:\evo-agent\src\agent\translator.rs",
    "D:\evo-agent\src\api\agent_api.rs",
    "D:\evo-agent\src\api\evorule_client.rs",
    "D:\evo-agent\src\api\mod.rs",
    "D:\evo-agent\src\io_handlers\llm_handler.rs",
    "D:\evo-agent\src\io_handlers\mod.rs",
    "D:\evo-agent\src\io_handlers\tool_handler.rs"
)

$utf8 = [System.Text.UTF8Encoding]::new($false)
$replacements = 0

foreach ($path in $targets) {
    if (-not (Test-Path $path)) { continue }
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $modified = $false

    for ($i = 1; $i -lt $bytes.Length - 2; $i++) {
        # Look for pattern: non-ASCII byte (high bit set) followed by 0x3F followed by 0x20 (space)
        if ($bytes[$i] -eq 0x3F -and ($bytes[$i-1] -band 0x80) -ne 0 -and $bytes[$i+1] -eq 0x20) {
            $bytes[$i] = 0x0A  # Replace ? with \n
            $modified = $true
            $replacements++
        }
    }

    if ($modified) {
        [System.IO.File]::WriteAllBytes($path, $bytes)
        Write-Host "FIXED: $path" -ForegroundColor Green
    } else {
        Write-Host "OK:    $path (no corruption markers)" -ForegroundColor Cyan
    }
}

Write-Host "`nTotal replacements: $replacements" -ForegroundColor Cyan
