# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 临时脚本：连跑 P1/P2/P3（Layer 1 基础类型层），输出精简结果
source ~/.cargo/env 2>/dev/null
cd "$(dirname "$0")/.." || exit 1

for p in verify_partial_eq_never_panics verify_ord_never_panics verify_as_methods_never_panic; do
  echo "===== $p ====="
  timeout 300 cargo kani -p evorule-tcb --tests --harness "$p" --output-format=terse 2>&1 \
    | grep -E 'VERIFICATION|Complete|Verification Time|harnesses|error\['
  echo ""
done
