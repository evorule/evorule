# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 单跑指定 proof 并写入日志（临时验证用）
source ~/.cargo/env 2>/dev/null
cd "$(dirname "$0")/.." || exit 1
p="$1"
SOLVER="${2:-minisat}"
LOG=verification/evidence/kani/single.log
: > "$LOG"
start=$(date +%s)
timeout 600 cargo kani -p evorule-tcb --tests --harness "$p" --solver "$SOLVER" --output-format=terse 2>&1 \
  | grep -E 'VERIFICATION|Complete|Verification Time|error|unwinding' > "$LOG"
end=$(date +%s)
echo "ELAPSED=$((end-start))s SOLVER=$SOLVER" >> "$LOG"
echo "DONE" >> "$LOG"
