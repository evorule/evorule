import subprocess
import time
import urllib.request

proc = subprocess.Popen(
    [r"D:\evorule\.build\rust\debug\evorule-server.exe",
     "--addr", "127.0.0.1:18080",
     "--log-level", "error"],
    cwd=r"D:\evorule",
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)

for i in range(10):
    try:
        urllib.request.urlopen("http://127.0.0.1:18080/api/health", timeout=1)
        break
    except Exception:
        time.sleep(0.5)

print("Server ready on 18080")
time.sleep(1)

import subprocess
result = subprocess.run(
    ["npx", "tsx", "tests/test_e2e.ts"],
    cwd=r"D:\evorule\sdk\typescript",
    capture_output=True,
    text=True,
    timeout=120,
)

print("\n=== TypeScript E2E Test Output ===")
print(result.stdout)
if result.stderr:
    print("\nSTDERR:")
    print(result.stderr)

proc.terminate()
try:
    proc.wait(timeout=5)
except Exception:
    proc.kill()
    proc.wait()

print(f"\nExit code: {result.returncode}")
