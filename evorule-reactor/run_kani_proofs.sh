#!/bin/bash
# Kani verification script for evorule-reactor proofs (all 6)
cd /mnt/d/evorule
source ~/.cargo/env 2>/dev/null

PROOFS=(
  invariant_version_monotonic
  max_rounds_termination
  command_does_not_decrease_queue
  invariant_io_count_register_complete
  invariant_io_count_force_remove
  invariant_io_recovery_iff_result
)

PASS=0
FAIL=0

for proof in "${PROOFS[@]}"; do
  echo "======== $proof ========"
  output=$(cargo kani -p evorule-reactor --harness "$proof" --output-format=terse 2>&1)
  exit_code=$?
  echo "$output" | grep -E "VERIFICATION|Checking|FAILED|SUCCESSFUL|failed|Summary" | head -8
  if echo "$output" | grep -q "SUCCESSFUL"; then
    echo ">>> PASS"
    PASS=$((PASS + 1))
  else
    echo ">>> FAIL (exit=$exit_code)"
    FAIL=$((FAIL + 1))
  fi
  echo ""
done

echo "======== SUMMARY ========"
echo "PASS=$PASS FAIL=$FAIL TOTAL=$((PASS + FAIL))"
