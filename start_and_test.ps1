$serverPath = "d:\evorule\.build\rust\debug\evorule-server.exe"
$serverArgs = @("--addr", "127.0.0.1:18080", "--log-level", "error")

Write-Host "Starting server..."
$proc = [System.Diagnostics.Process]::Start($serverPath, $serverArgs)

Write-Host "Waiting for server..."
for ($i = 1; $i -le 30; $i++) {
    Start-Sleep -Seconds 1
    try {
        $r = Invoke-WebRequest -Uri "http://localhost:18080/api/health" -UseBasicParsing -TimeoutSec 1
        if ($r.StatusCode -eq 200) {
            Write-Host "Server ready!"
            break
        }
    } catch {}
    Write-Host "Waiting... ($i/30)"
}

Write-Host "Running tests..."
cd d:\evorule\sdk\typescript
npx tsx tests/test_e2e.ts

Write-Host "Stopping server..."
$proc.Kill()