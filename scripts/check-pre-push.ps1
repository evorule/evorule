# check-pre-push.ps1 - Unified pre-push gate: secret scan + doc safety + public face.
#
# Local equivalent of the CI gate classes; any FAIL blocks the push so red never
# leaves the machine. Scanners are repo-local (each repo's scripts/ dir holds its
# own check_doc_safety.py / scan_public_face.py), so this gate works for any
# repo that carries them.
#
# Usage:
#   powershell -File check-pre-push.ps1 -Repo D:\evorule
#   powershell -File check-pre-push.ps1 -Repo D:\evo-agent -FailOn A1,B
#
# Exit codes: 0 = PASS (push allowed), 1 = BLOCKED (findings), 3 = config/usage error.

param(
    [Parameter(Mandatory = $true)][string]$Repo,
    [string[]]$EnvFile = @(),
    [string[]]$FailOn = @('A1', 'B')
)

$ErrorActionPreference = 'Continue'
$Repo = (Resolve-Path $Repo).Path
if (-not (Test-Path (Join-Path $Repo '.git'))) {
    Write-Output "ERROR: not a git repo: $Repo"
    exit 3
}
$scripts = Join-Path $Repo 'scripts'
$py = 'python'
$failed = $false

Write-Output "=== pre-push gate: $Repo ==="

# 1) secret scan over the outgoing diff (reuses check-push-secret-safety.ps1 next to this script)
$secret = Join-Path $PSScriptRoot 'check-push-secret-safety.ps1'
if (-not $EnvFile -and (Test-Path (Join-Path $Repo '.env'))) { $EnvFile = @((Join-Path $Repo '.env')) }
$secretArgs = @('-Repo', $Repo)
if ($EnvFile) { $secretArgs += @('-EnvFile', ($EnvFile -join ',')) }
& powershell -NoProfile -ExecutionPolicy Bypass -File $secret @secretArgs
if ($LASTEXITCODE -ne 0) { $failed = $true; Write-Output "[GATE] secret-scan: FAIL (exit=$LASTEXITCODE)" }
else { Write-Output "[GATE] secret-scan: PASS" }

# 2) doc safety over the whole tree (default checks; exit != 0 blocks)
$env:PYTHONUTF8 = '1'
& $py (Join-Path $scripts 'check_doc_safety.py') --cwd $Repo
if ($LASTEXITCODE -ne 0) { $failed = $true; Write-Output "[GATE] doc-safety: FAIL" }
else { Write-Output "[GATE] doc-safety: PASS" }

# 3) public face over the whole tree (blocking classes only; review-class findings stay advisory)
$sfArgs = @((Join-Path $scripts 'scan_public_face.py'), '--root', $Repo)
foreach ($f in $FailOn) { $sfArgs += @('--fail-on', $f) }
$sfOut = (& $py @sfArgs 2>&1 | Out-String).Trim()
$sfCode = $LASTEXITCODE
if ($sfCode -ne 0) {
    Write-Output $sfOut
    $failed = $true; Write-Output "[GATE] public-face: FAIL (exit=$sfCode)"
} else {
    # PASS: keep the console quiet - only the trailing summary lines
    ($sfOut -split "`n" | Select-Object -Last 3) | ForEach-Object { Write-Output $_.TrimEnd() }
    Write-Output "[GATE] public-face: PASS"
}

if ($failed) { Write-Output '=== GATE RESULT: BLOCKED ==='; exit 1 }
Write-Output '=== GATE RESULT: PASS ==='
exit 0
