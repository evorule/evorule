# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 补跑缺失证据：Layer1 (P1-P3) + Layer2 未验证项 (P4b/P5/P6a-d/P7)
source ~/.cargo/env 2>/dev/null
cd "$(dirname "$0")/.." || exit 1
LOG=verification/evidence/kani/p123_b_fill.log
: > "$LOG"
for p in \
  verify_partial_eq_never_panics \
  verify_ord_never_panics \
  verify_as_methods_never_panic \
  verify_resolve_path_nested_dot \
  verify_resolve_path_deterministic \
  verify_resolve_path_empty_returns_none \
  verify_resolve_path_trailing_dot \
  verify_resolve_path_invalid_index_char \
  verify_resolve_path_missing_close_bracket \
  verify_array_index_bounds; do
  echo "===== $p =====" >> "$LOG"
  timeout 600 cargo kani -p evorule-tcb --tests --harness "$p" --output-format=terse 2>&1 \
    | grep -E 'VERIFICATION|Complete|Verification Time|error|unwinding' >> "$LOG"
  echo "" >> "$LOG"
done
echo "DONE_ALL" >> "$LOG"
