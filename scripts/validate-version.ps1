param(
    [switch]$Quiet
)
$ErrorActionPreference = 'Stop'
trap {
    Write-Host ("TRAP EXCEPTION: " + $_.Exception.GetType().FullName + " :: " + $_.Exception.Message)
    if ($_.InvocationInfo) {
        Write-Host ("TRAP INVOCATION: Line=" + $_.InvocationInfo.ScriptLineNumber + " :: " + $_.InvocationInfo.Line)
    }
    Write-Host ("TRAP STACK: " + $_.ScriptStackTrace)
    exit 99
}

$evoruleRoot = Split-Path -Parent $PSScriptRoot
$evoAgentRoot = "D:\evo-agent"
$semverPattern = '^(\d+)\.(\d+)\.(\d+)(?:-([a-z]+)\.(\d+))?$'

function Get-TomlVersion {
    param([Parameter(Mandatory=$false)][AllowNull()][string]$Path)
    if ([string]::IsNullOrEmpty($Path)) { return $null }
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $line = Get-Content -LiteralPath $Path | Where-Object { $_ -match '^\s*version\s*=\s*"' } | Select-Object -First 1
    if ($line -and $line -match '"([^"]+)"') { return $Matches[1] }
    return $null
}
function Get-JsonVersion {
    param([Parameter(Mandatory=$false)][AllowNull()][string]$Path)
    if ([string]::IsNullOrEmpty($Path)) { return $null }
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $line = Get-Content -LiteralPath $Path | Where-Object { $_ -match '"version"\s*:' } | Select-Object -First 1
    if ($line -and $line -match '"version"\s*:\s*"([^"]+)"') { return $Matches[1] }
    return $null
}

$projects = [ordered]@{}
$projects['evorule-workspace'] = (Join-Path $evoruleRoot "Cargo.toml")
$projects['evorule-tcb']         = (Join-Path $evoruleRoot "evorule-tcb\Cargo.toml")
$projects['evorule-reactor']     = (Join-Path $evoruleRoot "evorule-reactor\Cargo.toml")
$projects['evorule-governance']  = (Join-Path $evoruleRoot "evorule-governance\Cargo.toml")
$projects['evo-agent']         = (Join-Path $evoAgentRoot "Cargo.toml")
$projects['sdk-typescript']    = (Join-Path $evoruleRoot "sdk\typescript\package.json")
$projects['sdk-python']        = (Join-Path $evoruleRoot "sdk\python\pyproject.toml")

$versions = [ordered]@{}
$failed = $false
if (-not $Quiet) { Write-Host "`n=== Version Validation ===" -ForegroundColor Cyan }
foreach ($name in $projects.Keys) {
    $p = $projects[$name]
    if ($name -like '*sdk-typescript*' -or $name -like '*sdk-python*') {
        $v = Get-JsonVersion $p
    } else {
        $v = Get-TomlVersion $p
    }
    $versions[$name] = $v
    if ($null -eq $v) {
        Write-Host "[SKIP] $name : version not found" -ForegroundColor Yellow
        continue
    }
    if ($v -notmatch $semverPattern) {
        Write-Host "[FAIL] $name : '$v' is not valid SemVer 2.0" -ForegroundColor Red
        $failed = $true
    } else {
        Write-Host "[OK]   $name : $v" -ForegroundColor Green
    }
}

$npmLockfile = Join-Path $evoruleRoot "sdk\typescript\package-lock.json"
if (Test-Path -LiteralPath $npmLockfile) {
    $lockVersion = Get-JsonVersion $npmLockfile
    $pkgVersion = $versions['sdk-typescript']
    if ($lockVersion -and $pkgVersion -and $lockVersion -ne $pkgVersion) {
        Write-Host "[FAIL] sdk-typescript lockfile: version '$lockVersion' != package.json '$pkgVersion'" -ForegroundColor Red
        $failed = $true
    } elseif ($lockVersion -and $pkgVersion) {
        Write-Host "[OK]   sdk-typescript lockfile: $lockVersion (matches package.json)" -ForegroundColor Green
    }
}

$parsedMajors = @()
foreach ($name in $versions.Keys) {
    $v = $versions[$name]
    if ($v -and $v -match $semverPattern) { $parsedMajors += [int]$Matches[1] }
}
$uniqueMajors = $parsedMajors | Sort-Object -Unique
if ($uniqueMajors.Count -gt 1) {
    Write-Host "`n[FAIL] MAJOR mismatch: $($uniqueMajors -join ', ')" -ForegroundColor Red
    $failed = $true
} elseif ($uniqueMajors.Count -eq 1) {
    Write-Host "`n[OK]   All projects share MAJOR = $($uniqueMajors[0])" -ForegroundColor Green
}

$mechMinor = $null
if ($versions['evorule-workspace'] -and $versions['evorule-workspace'] -match $semverPattern) {
    $mechMinor = [int]$Matches[2]
}
if ($null -ne $mechMinor) {
    Write-Host ""
    foreach ($name in @('evo-agent','sdk-typescript','sdk-python')) {
        $v = $versions[$name]
        if ($v -and $v -match $semverPattern) {
            $minor = [int]$Matches[2]
            if ($minor -lt $mechMinor) {
                Write-Host "[FAIL] $name MINOR ($minor) < evorule MINOR ($mechMinor)" -ForegroundColor Red
                $failed = $true
            } else {
                Write-Host "[OK]   $name MINOR ($minor) >= evorule MINOR ($mechMinor)" -ForegroundColor Green
            }
        }
    }
}

$canonicalVersion = $versions['evorule-workspace']

if (-not $Quiet) { Write-Host "`n=== Document Version Consistency ===" -ForegroundColor Cyan }
$readmePath = Join-Path $evoruleRoot "README.md"
if (Test-Path -LiteralPath $readmePath) {
    $readmeContent = Get-Content -LiteralPath $readmePath -Raw -Encoding UTF8
    if ($readmeContent -match 'version\s*=\s*\{([^}]+)\}') {
        $bibtexVersion = $Matches[1].Trim()
        if ($bibtexVersion -notlike "$canonicalVersion*") {
            Write-Host "[FAIL] README.md bibtex version '$bibtexVersion' != Cargo.toml '$canonicalVersion'" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   README.md bibtex version = $bibtexVersion" -ForegroundColor Green
        }
    }
}

$contribPath = Join-Path $evoruleRoot "CONTRIBUTING.md"
if (Test-Path -LiteralPath $contribPath) {
    $c = Get-Content -LiteralPath $contribPath -Raw -Encoding UTF8
    if ($c -match '\*\*Version\*\*:\s*(.+)') {
        $cv = $Matches[1].Trim()
        if ($cv -notlike "$canonicalVersion*") {
            Write-Host "[FAIL] CONTRIBUTING.md Version '$cv' != Cargo.toml '$canonicalVersion'" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   CONTRIBUTING.md Version = $cv" -ForegroundColor Green
        }
    }
}

$changelogPath = Join-Path $evoruleRoot "CHANGELOG.md"
if (Test-Path -LiteralPath $changelogPath) {
    $c = [System.IO.File]::ReadAllText($changelogPath, [System.Text.Encoding]::UTF8)
    if ($c -match '##\s*\[[^\]]+\]\s*-\s*(\d+\.\d+\.\d+(?:-[a-z]+\.\d+)?)') {
        $uv = $Matches[1].Trim()
        if ($uv -match '^(\d+)\.(\d+)') {
            $unMajor = [int]$Matches[1]; $unMinor = [int]$Matches[2]
            if ($canonicalVersion -match '^(\d+)\.(\d+)') {
                $canMajor = [int]$Matches[1]; $canMinor = [int]$Matches[2]
                if ($unMajor -lt $canMajor -or ($unMajor -eq $canMajor -and $unMinor -lt $canMinor)) {
                    Write-Host "[FAIL] CHANGELOG first entry '$uv' < current '$canonicalVersion'" -ForegroundColor Red
                    $failed = $true
                } elseif ($unMajor -eq $canMajor -and $unMinor -eq $canMinor) {
                    Write-Host "[OK]   CHANGELOG first entry = $uv (initial release)" -ForegroundColor Green
                } else {
                    Write-Host "[OK]   CHANGELOG first entry = $uv (> $canonicalVersion)" -ForegroundColor Green
                }
            }
        }
    }
}

$retiredPatterns = @('v6\.0','v6\.1','v6\.2','v7\.0','6\.0\.0')
$docFiles = @(
    @{ Path = (Join-Path $evoruleRoot "README.md"); Name = 'README.md' },
    @{ Path = (Join-Path $evoruleRoot "CONTRIBUTING.md"); Name = 'CONTRIBUTING.md' },
    @{ Path = (Join-Path $evoruleRoot "ROADMAP.md"); Name = 'ROADMAP.md' },
    @{ Path = (Join-Path $evoruleRoot "VERSION_STRATEGY.md"); Name = 'VERSION_STRATEGY.md' }
)
$retiredFound = $false
foreach ($doc in $docFiles) {
    if ([string]::IsNullOrEmpty($doc.Path) -or -not (Test-Path -LiteralPath $doc.Path)) { continue }
    $lines = Get-Content -LiteralPath $doc.Path -Encoding UTF8
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        foreach ($pattern in $retiredPatterns) {
            if ($line -match $pattern) {
                Write-Host "[FAIL] $($doc.Name):$($i+1) contains retired version pattern '$pattern': $($line.Trim())" -ForegroundColor Red
                $failed = $true; $retiredFound = $true
            }
        }
    }
}
if (-not $retiredFound) { Write-Host "[OK]   No retired version references (v6.x/v7.0) in docs" -ForegroundColor Green }

# === NEW: R2 文档硬编码版号校验（4e~4h）===

# 4e. README.md vX.Y.Z 徽章/标题硬编码
$do4e = (Test-Path -LiteralPath $readmePath) -and -not [string]::IsNullOrEmpty($canonicalVersion)
if ($do4e) {
    $rmAll = [System.IO.File]::ReadAllText($readmePath, [System.Text.Encoding]::UTF8)
    $col = [regex]::Matches($rmAll, '\bv(\d+\.\d+\.\d+)\b')
    for ($i4e = 0; $i4e -lt $col.Count; $i4e++) {
        $it = $col[$i4e]
        $found = $it.Groups[1].Value
        if ($found -ne $canonicalVersion -and $found -notmatch '^6\.' -and $found -notmatch '^7\.') {
            $msg4e = "[WARN] README.md contains hardcoded v" + $found + " (expected canonical v" + $canonicalVersion + ")"
            Write-Host $msg4e -ForegroundColor Yellow
        }
    }
}

# 4f. STATUS.md 基线版号
$statusPath = Join-Path $evoruleRoot "STATUS.md"
if (Test-Path -LiteralPath $statusPath) {
    $s = [System.IO.File]::ReadAllText($statusPath, [System.Text.Encoding]::UTF8)
    if ($s -match '##\s*v(\d+\.\d+\.\d+)\s*基线数据') {
        $sv = $Matches[1]
        if ($sv -ne $canonicalVersion) {
            Write-Host "[FAIL] STATUS.md baseline header 'v$sv' != Cargo.toml canonical 'v$canonicalVersion'" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   STATUS.md baseline header = v$sv (matches canonical)" -ForegroundColor Green
        }
    }
}

# 4g. DOCS_INDEX.md 版本对齐标识
$docsIndexPath = Join-Path $evoruleRoot "DOCS_INDEX.md"
if (Test-Path -LiteralPath $docsIndexPath) {
    $idx = [System.IO.File]::ReadAllText($docsIndexPath, [System.Text.Encoding]::UTF8)
    if ($idx -match 'version\s*=\s*"(\d+\.\d+\.\d+)"') {
        $iv = $Matches[1]
        if ($iv -ne $canonicalVersion) {
            Write-Host "[FAIL] DOCS_INDEX.md version-align marker '$iv' != Cargo.toml canonical '$canonicalVersion'" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   DOCS_INDEX.md version-align marker = $iv (matches canonical)" -ForegroundColor Green
        }
    }
}

# 4h. 形式化验证白皮书当前基线
$wpPath = Join-Path $evoruleRoot "EVORULE_FORMAL_VERIFICATION_PLAN_v3.md"
if (Test-Path -LiteralPath $wpPath) {
    $wp = [System.IO.File]::ReadAllText($wpPath, [System.Text.Encoding]::UTF8)
    if ($wp -match '当前基线[^|]*\bv(\d+\.\d+\.\d+)\b') {
        $wv = $Matches[1]
        if ($wv -ne $canonicalVersion) {
            Write-Host "[FAIL] Formal Verification Whitepaper current-baseline 'v$wv' != Cargo.toml canonical 'v$canonicalVersion'" -ForegroundColor Red
            $failed = $true
        } else {
            Write-Host "[OK]   Formal Verification Whitepaper current-baseline = v$wv (matches canonical)" -ForegroundColor Green
        }
    }
}

if ($failed) {
    Write-Host "`n[RESULT] FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "`n[RESULT] PASSED" -ForegroundColor Green
exit 0
