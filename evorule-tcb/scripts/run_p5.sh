# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 跑 P5（deterministic），精简输出
source ~/.cargo/env 2>/dev/null
cd "$(dirname "$0")/.." || exit 1
LOG=verification/evidence/kani/p5.log
: > "$LOG"
timeout 300 cargo kani -p evorule-tcb --tests --harness verify_resolve_path_deterministic --output-format=terse 2>&1 \
  | grep -E 'VERIFICATION|Complete|Verification Time|error|unwinding' >> "$LOG"
echo "DONE" >> "$LOG"
