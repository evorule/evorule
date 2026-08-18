# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 连跑 P4c/P4d/P4e（单路径 + 精确 unwind）
source ~/.cargo/env 2>/dev/null
cd /mnt/d/evorule/evorule-tcb || exit 1
LOG=/mnt/d/evorule/evorule-tcb/verification/evidence/kani/p4cde.log
: > "$LOG"
for p in verify_resolve_path_array_index verify_resolve_path_double_dot verify_resolve_path_escaped_dot; do
  {
    echo "===== $p ====="
    timeout 120 cargo kani -p evorule-tcb --tests --harness "$p" --output-format=terse 2>&1 \
      | grep -E 'VERIFICATION|Complete|Verification Time|error'
    echo ""
  } >> "$LOG"
done
echo "DONE" >> "$LOG"
