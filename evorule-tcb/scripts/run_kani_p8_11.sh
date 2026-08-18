# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 跑 Layer 3 (P8a-g, P9-P11)：域评估层（拆分后独立 proof）
source ~/.cargo/env 2>/dev/null
cd "$(dirname "$0")/.." || exit 1
LOG=verification/evidence/kani/p8_11.log
: > "$LOG"
for p in \
  verify_evaluate_domain_eq_never_panics \
  verify_evaluate_domain_lt_never_panics \
  verify_evaluate_domain_exists_never_panics \
  verify_evaluate_domain_instruction_never_panics \
  verify_evaluate_domain_all_never_panics \
  verify_evaluate_domain_not_never_panics \
  verify_evaluate_domain_has_fields_never_panics \
  verify_evaluate_domain_deterministic \
  verify_domain_depth_limit \
  verify_has_fields_empty_array; do
  echo "===== $p =====" >> "$LOG"
  timeout 900 cargo kani -p evorule-tcb --tests --harness "$p" --output-format=terse 2>&1 \
    | grep -E 'VERIFICATION|Complete|Verification Time|error|unwinding' >> "$LOG"
  echo "" >> "$LOG"
done
echo "DONE_ALL" >> "$LOG"
