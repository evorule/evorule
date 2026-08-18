# SPDX-License-Identifier: AGPL-3.0-or-later
#!/bin/bash
# =============================================================================
# run_kani_tcb.sh - evorule-tcb Kani 验证脚本（tests/kani/ 证明）
#
# 用法: bash scripts/run_kani_tcb.sh [proof_name] [timeout_seconds]
#   不带参数: 跑全部 21 个 proof
#   带参数:   跑指定 proof（默认超时 600s）
#
# 说明:
# - proof 位于 tests/kani/（tests/kani_entry.rs 为顶层入口），需加 --tests
# - 简单 proof (P1-P7) 无递归，可低 unwind；递归 proof (P8/P10) 需高 unwind
# - P1/P2 使用 #[kani::unwind(8)]（文件内已配置）
# =============================================================================
cd "$(dirname "$0")/.." || exit 1
source ~/.cargo/env 2>/dev/null

PROOFS=(
  # === Layer 1: 基础类型层 ===
  verify_partial_eq_never_panics
  verify_ord_never_panics
  verify_as_methods_never_panic
  # === Layer 2: 路径解析层（P4 拆分为 P4a-P4e 单路径证明）===
  verify_resolve_path_simple_field
  verify_resolve_path_nested_dot
  verify_resolve_path_array_index
  verify_resolve_path_double_dot
  verify_resolve_path_escaped_dot
  verify_resolve_path_deterministic
  verify_resolve_path_empty_returns_none
  verify_resolve_path_trailing_dot
  verify_resolve_path_invalid_index_char
  verify_resolve_path_missing_close_bracket
  verify_array_index_bounds
  # === Layer 3: 域评估层 ===
  verify_evaluate_domain_never_panics
  verify_evaluate_domain_deterministic
  verify_domain_depth_limit
  verify_has_fields_empty_array
  # === Layer 4: 元指令层 ===
  verify_execute_meta_instruction_never_panics
  verify_exec_set_arithmetic_safe
  verify_branch_depth_limit
  verify_collect_safe_with_after
  verify_merge_safe
  verify_substitute_template_never_panics
  verify_io_request_safe
  # === Layer 5: 状态转换层 ===
  verify_execute_transition_never_panics
  verify_transform_rules_limit
  verify_react_io_required
)

# 参数解析
PROOF=""
TIMEOUT_SEC=600
if [ -n "$1" ]; then
  PROOF="$1"
fi
if [ -n "$2" ]; then
  TIMEOUT_SEC="$2"
fi

PASS=0
FAIL=0
TIMEOUT=0

run_one() {
  local proof="$1"
  echo "======== $proof ========"
  local start dur exit_code
  start=$(date +%s)
  timeout "$TIMEOUT_SEC" cargo kani -p evorule-tcb --tests --harness "$proof" --output-format=terse > "/tmp/kani_${proof}.log" 2>&1
  exit_code=$?
  end=$(date +%s)
  dur=$((end - start))
  # 精简关键行
  grep -E "VERIFICATION|Failed Checks|SUMMARY|Checking harness|panic|error\[|SUCCESSFUL|FAILED" "/tmp/kani_${proof}.log" | head -12
  if [ $exit_code -eq 0 ]; then
    if grep -q "SUCCESSFUL" "/tmp/kani_${proof}.log"; then
      echo ">>> PASS (${dur}s)"
      PASS=$((PASS + 1))
    else
      echo ">>> ERROR: exit=0 but no SUCCESSFUL marker (${dur}s)"
      FAIL=$((FAIL + 1))
    fi
  elif [ $exit_code -eq 124 ]; then
    echo ">>> TIMEOUT (${dur}s, ${TIMEOUT_SEC}s limit)"
    TIMEOUT=$((TIMEOUT + 1))
  else
    echo ">>> FAIL (${dur}s, exit=$exit_code)"
    FAIL=$((FAIL + 1))
  fi
  echo ""
}

if [ -n "$PROOF" ]; then
  run_one "$PROOF"
else
  for p in "${PROOFS[@]}"; do
    run_one "$p"
  done
fi

echo "======== SUMMARY ========"
echo "PASS=$PASS FAIL=$FAIL TIMEOUT=$TIMEOUT TOTAL=$((PASS + FAIL + TIMEOUT))"
