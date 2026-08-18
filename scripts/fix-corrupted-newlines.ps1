# SPDX-License-Identifier: AGPL-3.0-or-later
﻿# fix-corrupted-newlines.ps1
# Third-pass fix: replace 0x3F (?) with 0x0A (\n) when followed by Rust line-start tokens
# (like #[, pub, fn, impl, use, let, etc.)

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

# Common Rust line-start tokens after indentation
# Pattern: optional whitespace then keyword/attribute
$replacements = 0

foreach ($path in $targets) {
    if (-not (Test-Path $path)) { continue }
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $modified = $false

    for ($i = 1; $i -lt $bytes.Length - 3; $i++) {
        if ($bytes[$i] -ne 0x3F) { continue }
        $prev = $bytes[$i - 1]
        if (($prev -band 0x80) -eq 0) { continue }  # previous must be non-ASCII
        $next = $bytes[$i + 1]

        # Case 1: followed by # (could be #[derive] or #![attribute])
        if ($next -eq 0x23) {
            $bytes[$i] = 0x0A
            $modified = $true
            $replacements++
            continue
        }

        # Case 2: followed by whitespace then identifier
        if ($next -eq 0x20 -or $next -eq 0x09) {
            # Look ahead for non-whitespace ASCII letter (start of keyword)
            $j = $i + 1
            while ($j -lt $bytes.Length -and ($bytes[$j] -eq 0x20 -or $bytes[$j] -eq 0x09)) { $j++ }
            if ($j -lt $bytes.Length) {
                $first = $bytes[$j]
                # Common Rust keyword starts
                if ($first -in 0x61..0x7B) {  # a-z
                    $bytes[$i] = 0x0A
                    $modified = $true
                    $replacements++
                }
            }
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
