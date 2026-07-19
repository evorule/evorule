$serverPath = ".\.build\rust\debug\evorule-server.exe"
$arguments = "--addr 127.0.0.1:18080 --log-level error"
$workingDir = "d:\evorule"

Write-Host "Starting evorule-server..."
$processInfo = New-Object System.Diagnostics.ProcessStartInfo
$processInfo.FileName = $serverPath
$processInfo.Arguments = $arguments
$processInfo.WorkingDirectory = $workingDir
$processInfo.UseShellExecute = $false
$processInfo.CreateNoWindow = $true

$process = New-Object System.Diagnostics.Process
$process.StartInfo = $processInfo
$process.Start() | Out-Null

Write-Host "Waiting for server..."
for ($i = 1; $i -le 30; $i++) {
    try {
        $r = Invoke-WebRequest -Uri "http://localhost:18080/api/health" -UseBasicParsing -TimeoutSec 1
        if ($r.StatusCode -eq 200) {
            Write-Host "Server ready"
            break
        }
    } catch {
        Start-Sleep -Seconds 1
        Write-Host "Waiting... ($i/30)"
    }
}

Write-Host "Running TypeScript E2E tests..."
cd d:\evorule\sdk\typescript
npx tsx tests/test_e2e.ts

Write-Host "Stopping server..."
$process.Kill()