# check-push-secret-safety.ps1 - Pre-push secret leak scanner (local gate).
#
# Scans the outgoing diff (a git range) for real secret values collected from
# .env files plus a set of generic secret patterns. Matched VALUES are never
# printed - findings report key name / pattern class + file:line only.
#
# Usage:
#   powershell -File check-push-secret-safety.ps1 -Repo D:\evo-agent -EnvFile D:\evo-agent\.env
#   powershell -File check-push-secret-safety.ps1 -Repo <repo> -Range origin/main..HEAD -EnvFile <env1>,<env2>
#
# Exit codes: 0 = PASS, 1 = potential leak (PUSH BLOCKED), 3 = config/usage error.

param(
    [Parameter(Mandatory = $true)][string]$Repo,
    [string]$Range = "",
    [string[]]$EnvFile = @(),
    [string[]]$ExtraPattern = @()
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path (Join-Path $Repo '.git'))) {
    Write-Output "ERROR: not a git repo: $Repo"
    exit 3
}

# --- resolve range (default: upstream..HEAD, fallback origin/main|master) ---
if ($Range -eq "") {
    $up = $null
    git -C $Repo rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>$null | ForEach-Object { $up = $_ }
    if (-not $up) {
        foreach ($cand in @('origin/main', 'origin/master')) {
            git -C $Repo rev-parse --verify --quiet $cand 2>$null | Out-Null
            if ($LASTEXITCODE -eq 0) { $up = $cand; break }
        }
    }
    if (-not $up) {
        Write-Output "ERROR: no upstream / origin-main / origin-master found; pass -Range explicitly."
        exit 3
    }
    $Range = "$up..HEAD"
}
Write-Output "repo=$Repo range=$Range"

# --- collect literal secret candidates from env files ---
$literals = @{}
foreach ($f in $EnvFile) {
    if (-not (Test-Path $f)) { Write-Output "WARN: env file not found, skipped: $f"; continue }
    foreach ($line in (Get-Content $f)) {
        if ($line -match '^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(\S+?)\s*$') {
            $k = $Matches[1]; $v = $Matches[2]
            # skip placeholders and short values to avoid false positives
            if ($v.Length -lt 8) { continue }
            if ($v -match '^(your_|changeme|xxx|<|\$\{)' -or $v -match '_here$') { continue }
            $literals[$v] = $k
        }
    }
}

# --- generic secret patterns (fallback net; literal scan is the primary net) ---
$patterns = @(
    @{ name = 'openai-style-key(sk-)'; re = 'sk-[A-Za-z0-9_-]{16,}' },
    @{ name = 'github-token(ghp_)';    re = 'ghp_[A-Za-z0-9]{30,}' },
    @{ name = 'github-fine-grained';   re = 'github_pat_[A-Za-z0-9_]{20,}' },
    @{ name = 'aws-access-key(AKIA)';  re = 'AKIA[0-9A-Z]{16}' },
    @{ name = 'slack-token(xox)';      re = 'xox[baprs]-[A-Za-z0-9-]{10,}' },
    @{ name = 'private-key-block';     re = '-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----' }
)
foreach ($p in $ExtraPattern) { $patterns += @{ name = 'extra'; re = $p } }

# --- fetch diff and scan added lines with hunk tracking ---
$diff = git -C $Repo diff $Range -- 2>$null
if (-not $diff -or @($diff).Count -eq 0) {
    Write-Output "PASS: no outgoing diff (nothing to push for this range)."
    exit 0
}

$curFile = ''
$newLine = 0
$findings = New-Object System.Collections.Generic.List[string]
foreach ($ln in $diff) {
    if ($ln -match '^diff --git') { $curFile = ''; $newLine = 0; continue }
    if ($ln -match '^--- ') { continue }
    if ($ln -match '^\+\+\+ b/(.*)$') { $curFile = $Matches[1]; $newLine = 0; continue }
    if ($ln -match '^\\') { continue }
    if ($ln -match '^@@ -\d+(?:,\d+)? \+(\d+)') { $newLine = [int]$Matches[1]; continue }
    if ($ln.StartsWith('-')) { continue }
    if ($ln.StartsWith('+') -and -not $ln.StartsWith('+++')) {
        foreach ($kv in $literals.GetEnumerator()) {
            if ($ln.Contains($kv.Key)) {
                $findings.Add(("LITERAL[key={0}] {1}:{2}" -f $kv.Value, $curFile, $newLine))
            }
        }
        foreach ($p in $patterns) {
            if ($ln -match $p.re) {
                $findings.Add(("PATTERN[{0}] {1}:{2}" -f $p.name, $curFile, $newLine))
            }
        }
        $newLine++
    }
    elseif ($ln.StartsWith(' ')) { $newLine++ }
}

# --- report ---
if ($findings.Count -gt 0) {
    Write-Output "FAIL: potential secret leak(s) in outgoing diff:"
    $findings | ForEach-Object { Write-Output "  $_" }
    Write-Output "PUSH BLOCKED. (matched values intentionally not printed)"
    exit 1
}
Write-Output ("PASS: no secret candidates in outgoing diff (literals checked: {0}, patterns: {1})." -f $literals.Count, $patterns.Count)
exit 0
