# SPDX-License-Identifier: AGPL-3.0-or-later
# End-to-end API test + hash chain diagnosis
# Usage: powershell -ExecutionPolicy Bypass -File test-api-with-hash-diagnosis.ps1

$ErrorActionPreference = "Continue"
$BaseUrl = "http://127.0.0.1:18080"
$Token = "evorule-dev-token"
$SessionId = $null
$Passed = 0
$Failed = 0

function Invoke-Api {
    param([string]$Method, [string]$Endpoint, [object]$Body = $null)
    $url = "$BaseUrl$Endpoint"
    $headers = @{ "Authorization" = "Bearer $Token" }
    try {
        $params = @{ Uri = $url; Method = $Method; Headers = $headers; UseBasicParsing = $true; TimeoutSec = 10 }
        if ($Body) {
            $params.Body = ($Body | ConvertTo-Json -Depth 10)
            $params.ContentType = "application/json"
        }
        $response = Invoke-WebRequest @params
        return @{ Success = $true; StatusCode = $response.StatusCode; Content = ($response.Content | ConvertFrom-Json); Raw = $response.Content }
    } catch {
        return @{ Success = $false; Error = $_.Exception.Message; Content = $null }
    }
}

function Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = "")
    if ($Ok) { Write-Host "[PASS] $Name" -ForegroundColor Green; $script:Passed++ }
    else { Write-Host "[FAIL] $Name" -ForegroundColor Red; $script:Failed++ }
    if ($Detail) { Write-Host "       $Detail" -ForegroundColor Gray }
}

Write-Host ""
Write-Host "=== EvoRule API Test + Hash Chain Diagnosis ===" -ForegroundColor Yellow
Write-Host ""

# Test 1: Health
$r = Invoke-Api GET "/api/health"
Check "GET /api/health" ($r.Success -and $r.Content.success) "message=$($r.Content.message)"

# Test 2: Create session
$r = Invoke-Api POST "/api/sessions" @{ initial_payload = @{ counter = 0 }; max_rounds = 1000 }
$SessionId = $r.Content.session_id
Check "POST /api/sessions" ($r.Success -and $SessionId) "session_id=$SessionId"
if (-not $SessionId) { Write-Host "ABORT: cannot create session" -ForegroundColor Red; exit 1 }

# Test 3: Submit command
$r = Invoke-Api POST "/api/sessions/$SessionId/command" @{ instruction = @{ type = "transform"; path = "counter"; op = "increment"; value = 5 } }
Check "POST /api/sessions/$SessionId/command" ($r.Success -and $r.Content.success) "fact_id=$($r.Content.fact_id)"

# Test 4: Get state
$r = Invoke-Api GET "/api/sessions/$SessionId/state"
Check "GET /api/sessions/$SessionId/state" ($r.Success) "version=$($r.Content.version) phase=$($r.Content.phase)"

# Test 5: Verify audit chain
$r = Invoke-Api GET "/api/sessions/$SessionId/audit/verify"
$verified = $r.Success -and $r.Content.verified
Check "GET /api/sessions/$SessionId/audit/verify" $verified "verified=$($r.Content.verified) fact_count=$($r.Content.fact_count) last_hash=$($r.Content.last_hash)"

if (-not $verified) {
    Write-Host ""
    Write-Host "--- Hash Chain Diagnosis ---" -ForegroundColor Yellow

    # Diagnosis 1: Audit report
    $a = Invoke-Api GET "/api/sessions/$SessionId/audit"
    if ($a.Success) {
        $entries = $a.Content.entries
        Write-Host "[DIAG] Audit entries: $($entries.Count)" -ForegroundColor Cyan
        $noHash = @($entries | Where-Object { -not $_.content_hash -or -not $_.chain_hash })
        if ($noHash.Count -gt 0) {
            Write-Host "[DIAG] WARNING: $($noHash.Count) entries missing hash fields" -ForegroundColor Red
        } else {
            Write-Host "[DIAG] All entries have hash fields" -ForegroundColor Green
        }
    }

    # Diagnosis 2: Export audit chain
    $e = Invoke-Api GET "/api/sessions/$SessionId/audit/export"
    if ($e.Success) {
        $exportPath = "$env:TEMP\evorule-audit-$SessionId.json"
        $e.Raw | Out-File -FilePath $exportPath -Encoding UTF8
        Write-Host "[DIAG] Exported to: $exportPath" -ForegroundColor Cyan
    }

    # Diagnosis 3: Check WAL file
    $walDir = "D:\evorule\data\wal"
    $walFile = Get-ChildItem -Path $walDir -Filter "session_${SessionId}_*.wal" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($walFile) {
        Write-Host "[DIAG] WAL file: $($walFile.FullName) ($($walFile.Length) bytes)" -ForegroundColor Cyan
        $lines = Get-Content $walFile.FullName | Where-Object { $_.Trim() -ne "" }
        Write-Host "[DIAG] WAL records: $($lines.Count)" -ForegroundColor Cyan
        if ($lines.Count -gt 0) {
            $first = $lines[0] | ConvertFrom-Json
            if ($first.content_hash -and $first.chain_hash) {
                Write-Host "[DIAG] WAL format: v2 (has hash fields)" -ForegroundColor Green
            } else {
                Write-Host "[DIAG] WAL format: v1 (NO hash fields) - this is the problem!" -ForegroundColor Red
            }
        }
    } else {
        Write-Host "[DIAG] No WAL file found in $walDir" -ForegroundColor Yellow
    }

    # Diagnosis 4: Run unit tests for hash
    Write-Host ""
    Write-Host "[DIAG] Running hash unit tests..." -ForegroundColor Cyan
    $testOut = cargo test -p evorule-reactor hash --quiet 2>&1
    Write-Host $testOut -ForegroundColor Gray

    # Diagnosis 5: Run end-to-end test
    Write-Host ""
    Write-Host "[DIAG] Running end-to-end audit chain test..." -ForegroundColor Cyan
    $e2eOut = cargo test --test end_to_end_audit_chain --quiet 2>&1
    Write-Host $e2eOut -ForegroundColor Gray

    Write-Host ""
    Write-Host "--- Root Cause Analysis ---" -ForegroundColor Yellow
    Write-Host "If WAL is v1 (no hash fields):" -ForegroundColor Gray
    Write-Host "  -> FactsLog::append() not calling append_record_with_hash()" -ForegroundColor Gray
    Write-Host "  -> Check evorule-reactor/src/facts_log.rs append() method" -ForegroundColor Gray
    Write-Host "  -> Ensure persistence feature is enabled" -ForegroundColor Gray
    Write-Host ""
    Write-Host "If WAL is v2 but verify fails:" -ForegroundColor Gray
    Write-Host "  -> Auditor::load_from_tier1_wal() hash mismatch" -ForegroundColor Gray
    Write-Host "  -> Check evorule-governance/src/auditor.rs verification logic" -ForegroundColor Gray
    Write-Host "  -> Run: cargo test --test end_to_end_audit_chain -- --nocapture" -ForegroundColor Gray
}

# Test 6: Rewind
$r = Invoke-Api GET "/api/sessions/$SessionId/rewind?version=0"
Check "GET /api/sessions/$SessionId/rewind" ($r.Success) "target=$($r.Content.target_version) actual=$($r.Content.actual_version)"

# Test 7: Diff (escape & for PowerShell)
$diffUrl = "/api/sessions/$SessionId/diff?a=0" + "&" + "b=1"
$r = Invoke-Api GET $diffUrl
Check "GET /api/sessions/$SessionId/diff" ($r.Success) "summary=$($r.Content.summary)"

# Cleanup
Invoke-Api DELETE "/api/sessions/$SessionId" | Out-Null

# Summary
Write-Host ""
Write-Host "=== Summary ===" -ForegroundColor Yellow
Write-Host "Passed: $Passed" -ForegroundColor Green
Write-Host "Failed: $Failed" -ForegroundColor $(if ($Failed -gt 0) { "Red" } else { "Green" })
Write-Host ""

if ($Failed -gt 0) { exit 1 } else { exit 0 }
