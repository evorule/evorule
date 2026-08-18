# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 启动 P4-P7 验证并在完成后写 DONE 标记（脱离当前终端独立运行）
source ~/.cargo/env 2>/dev/null
cd /mnt/d/evorule/evorule-tcb || exit 1
LOG=/mnt/d/evorule/evorule-tcb/verification/evidence/kani/p4567_tmp.log
rm -f "$LOG"
{
  for p in verify_resolve_path_never_panics verify_resolve_path_deterministic verify_resolve_path_invalid_returns_none verify_array_index_bounds; do
    echo "===== $p ====="
    timeout 600 cargo kani -p evorule-tcb --tests --harness "$p" --output-format=terse 2>&1 \
      | grep -E 'VERIFICATION|Complete|Verification Time|harnesses|error\[|unwinding'
    echo ""
  done
  echo "DONE_ALL"
} >> "$LOG" 2>&1
