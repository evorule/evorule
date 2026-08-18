# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# 临时脚本：连跑 Layer 2 (P4a-P7)，输出精简结果
# 说明：每条 proof 只含 1 条路径（避免累积展开状态爆炸），unwind 按路径长度精确设置
source ~/.cargo/env 2>/dev/null
cd "$(dirname "$0")/.." || exit 1

for p in \
  verify_resolve_path_simple_field \
  verify_resolve_path_nested_dot \
  verify_resolve_path_array_index \
  verify_resolve_path_double_dot \
  verify_resolve_path_escaped_dot \
  verify_resolve_path_deterministic \
  verify_resolve_path_empty_returns_none \
  verify_resolve_path_trailing_dot \
  verify_resolve_path_invalid_index_char \
  verify_resolve_path_missing_close_bracket \
  verify_array_index_bounds; do
  echo "===== $p ====="
  timeout 300 cargo kani -p evorule-tcb --tests --harness "$p" --output-format=terse 2>&1 \
    | grep -E 'VERIFICATION|Complete|Verification Time|harnesses|error\[|unwinding'
  echo ""
done
