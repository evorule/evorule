$path = "D:\evorule\evorule-reactor\src\ffi.rs"
$bytes = [System.IO.File]::ReadAllBytes($path)

$spdx = [System.Text.Encoding]::UTF8.GetBytes("// SPDX-License-Identifier: AGPL-3.0-or-later`n// Copyright (C) 2026 EvoRule Project`n// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.`n`n")

# Find BOM (EF BB BF) at start
$hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
$contentStart = if ($hasBom) { 3 } else { 0 }

# Check if SPDX already present
$headerSlice = [System.Text.Encoding]::UTF8.GetString($bytes[$contentStart..($contentStart + 50)])
if ($headerSlice -match 'SPDX-License-Identifier: AGPL-3.0') {
    Write-Host "Already has SPDX, skipping"
    exit 0
}

$newBytes = New-Object byte[] ($bytes.Length + $spdx.Length)
if ($hasBom) {
    [Array]::Copy($bytes, 0, $newBytes, 0, 3)
    [Array]::Copy($spdx, 0, $newBytes, 3, $spdx.Length)
    [Array]::Copy($bytes, 3, $newBytes, 3 + $spdx.Length, $bytes.Length - 3)
} else {
    [Array]::Copy($spdx, 0, $newBytes, 0, $spdx.Length)
    [Array]::Copy($bytes, 0, $newBytes, $spdx.Length, $bytes.Length)
}

[System.IO.File]::WriteAllBytes($path, $newBytes)
Write-Host "Added SPDX to $path"
