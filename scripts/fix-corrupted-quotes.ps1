# fix-corrupted-quotes.ps1
# Second-pass fix: replace 0x3F (?) with 0x22 (") when it's at a string boundary
# (after non-ASCII bytes, followed by . , ) ; (typical Rust end-of-expression markers)

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

$replacements = 0
$endMarkers = @(0x2E, 0x2C, 0x29, 0x3B)  # . , ) ;

foreach ($path in $targets) {
    if (-not (Test-Path $path)) { continue }
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $modified = $false

    for ($i = 1; $i -lt $bytes.Length - 1; $i++) {
        if ($bytes[$i] -ne 0x3F) { continue }
        $prev = $bytes[$i - 1]
        $next = $bytes[$i + 1]
        # Pattern: non-ASCII byte before ?, ASCII punctuation after ?
        if (($prev -band 0x80) -ne 0 -and $endMarkers -contains $next) {
            $bytes[$i] = 0x22  # Replace ? with "
            $modified = $true
            $replacements++
        }
    }

    if ($modified) {
        [System.IO.File]::WriteAllBytes($path, $bytes)
        Write-Host "FIXED: $path" -ForegroundColor Green
    } else {
        Write-Host "OK:    $path" -ForegroundColor Cyan
    }
}

Write-Host "`nTotal replacements: $replacements" -ForegroundColor Cyan
