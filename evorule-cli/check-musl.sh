#!/bin/bash
# 脚本位于 evorule-cli/ 下，仓根是其父目录（target/ 与 .build/ 输出在仓根）
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# 兼容两种输出目录：标准 cargo target/ 与 sandbox 模式的 .build/rust/
# 先找 musl 构建产物；找不到再 fallback 到 release 目录
for candidate in \
    "$SCRIPT_DIR/../.build/rust/x86_64-unknown-linux-musl/release/evorule" \
    "$SCRIPT_DIR/../target/x86_64-unknown-linux-musl/release/evorule" \
    "$SCRIPT_DIR/../.build/rust/release/evorule" \
    "$SCRIPT_DIR/../target/release/evorule" \
; do
    if [[ -f "$candidate" ]]; then
        BIN="$candidate"
        break
    fi
done
if [[ -z "${BIN:-}" ]]; then
    echo "ERROR: evorule binary not found. Run build-musl.sh first." >&2
    exit 2
fi
ls -la "$BIN"
echo "---"
file "$BIN"
echo "---"
ldd "$BIN" 2>&1
echo "---"
"$BIN" --version
echo "---"
"$BIN" validate /tmp 2>&1 | head -3

