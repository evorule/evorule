#!/bin/bash
source ~/.cargo/env
# 脚本位于 evorule-cli/ 下，仓根是其父目录（与 evorule-tcb/evorule-reactor 同级）
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."
echo "=== Kani version ==="
cargo kani --version
echo ""
echo "=== Running Kani proof: invariant_io_count_consistency ==="
echo "=== (evorule-reactor depends on tokio, compilation may be slow) ==="
echo "=== Start: $(date) ==="
cargo kani -p evorule-reactor --harness invariant_io_count_consistency --output-format=terse 2>&1 | tail -60
echo "=== End: $(date) ==="
echo "=== Exit code: $? ==="
