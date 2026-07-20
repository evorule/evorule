Start-Process -FilePath ".\.build\rust\debug\evorule-server.exe" -WindowStyle Hidden
Start-Sleep -Seconds 3
try {
    $r = Invoke-WebRequest -Uri "http://localhost:18080/api/health" -TimeoutSec 3 -UseBasicParsing
    Write-Host "Server OK: $($r.StatusCode)"
} catch {
    Write-Host "Server FAIL: $($_.Exception.Message)"
}
