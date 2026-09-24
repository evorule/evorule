#!/usr/bin/env bash
# A 档证据复跑落盘脚本（2026-09-24，M3.4 严格口径：约束前置门变更后 WSL 复跑）
# 用法：wsl -e bash /mnt/d/evorule/scripts/ops/rerun-a-tier-20260924.sh
# 产出：evorule-tcb/verification/evidence/kani/ 与 evorule-reactor/verification/evidence/kani/ 下
#       P0-x.<harness>_PASS_<sha>_<ts>.log + .stdout.txt 配对（PASS 时落盘）
set -u
export PATH="$HOME/.cargo/bin:$HOME/.kani/kani-0.67.0/bin:$PATH"
SHA=$(git -C /mnt/d/evorule rev-parse HEAD)
TS=$(date +%Y%m%d_%H%M%S)
TCB_EVID=/mnt/d/evorule/evorule-tcb/verification/evidence/kani
REACT_EVID=/mnt/d/evorule/evorule-reactor/verification/evidence/kani
mkdir -p "$TCB_EVID" "$REACT_EVID"

run_one() {
  local prop="$1" harness="$2" crate="$3" evid="$4" extra="$5"
  echo "===== $prop.$harness =====" > /tmp/kani_run.log
  timeout -k 30 300 cargo kani -p "$crate" $extra --harness "$harness" --output-format=terse >> /tmp/kani_run.log 2>&1
  local rc=$?
  if [ $rc -eq 0 ]; then
    local out="$evid/$prop.$harness._PASS_PENDING_.log"
    # 命名在调用侧完成；这里仅回传 rc 与耗时
    echo "PASS $prop.$harness rc=0"
    cp /tmp/kani_run.log "$evid/$prop.$harness.PASS.raw"
  else
    echo "FAIL $prop.$harness rc=$rc"
    cp /tmp/kani_run.log "$evid/$prop.$harness.FAIL.raw"
  fi
}

# ---- TCB A 档 14 个（P0-3 x11 + P0-6 x3）----
P03="verify_resolve_path_simple_field verify_resolve_path_nested_dot verify_resolve_path_array_index verify_resolve_path_double_dot verify_resolve_path_escaped_dot verify_resolve_path_empty_returns_none verify_resolve_path_trailing_dot verify_resolve_path_invalid_index_char verify_resolve_path_missing_close_bracket verify_resolve_path_deterministic verify_array_index_bounds"
for h in $P03; do run_one "P0-3" "$h" evorule-tcb "$TCB_EVID" "--tests"; done
for h in verify_partial_eq_never_panics verify_ord_never_panics verify_as_methods_never_panic; do
  run_one "P0-6" "$h" evorule-tcb "$TCB_EVID" "--tests"
done
# ---- 新排列等价 proof（P0-5 域）----
run_one "P0-5" verify_exec_enforce_permutation_equivalence evorule-tcb "$TCB_EVID" "--tests"
# ---- reactor 4 个（P0-11/P1-3/P1-5/P1-6）----
for pair in "P0-11 invariant_cause_queue_sync" "P1-3 invariant_version_monotonic" "P1-5 command_does_not_decrease_queue" "P1-6 max_rounds_termination"; do
  set -- $pair
  run_one "$1" "$2" evorule-reactor "$REACT_EVID" ""
done
echo "ALL-DONE SHA=$SHA TS=$TS"
