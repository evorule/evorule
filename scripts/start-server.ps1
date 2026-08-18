# SPDX-License-Identifier: AGPL-3.0-or-later
﻿Set-Location "d:\evorule"
Write-Host "Starting server from: $(Get-Location)"
python -m http.server 8766 --bind 127.0.0.1
