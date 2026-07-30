#!/bin/bash
# build-musl.sh - 构建 evorule musl 静态二进制(单文件分发,圈 2 合规刚需)
#
# 必须在 Linux 容器或 WSL Ubuntu 22.04 中运行
# Windows 原生不支持交叉编译到 musl
#
# 用法:
#   bash build-musl.sh                          # 默认 x86_64
#   bash build-musl.sh --target aarch64         # 交叉编译 aarch64 (需 gcc-aarch64-linux-gnu 包,提供 aarch64-linux-gnu-gcc)
#   bash build-musl.sh --clean                  # clean + 编译
#   bash build-musl.sh --check                  # 只验证二进制,不重新编译
#   bash build-musl.sh --repro                  # 跑 reproducibility 测试(2 次构建 + SHA256 对比)
#
# 可重现构建(reproducible build):
#   - SOURCE_DATE_EPOCH 固定为 1700000000(2023-11-14),让所有时间戳确定
#   - CARGO_INCREMENTAL=0 强制全量构建
#   - --build-id=none 去掉 linker 引入的随机 build-id
#   - 两次构建 SHA256 必须一致(用 --repro 验证)
#
# 圈 2 卖点:监管可以独立复现 release artifact,验证供应链可信。

set -euo pipefail

# Source cargo env (WSL non-interactive shells don't have it in PATH)
. "$HOME/.cargo/env" 2>/dev/null || true

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# 解析参数
TARGET="x86_64-unknown-linux-musl"
CLEAN=0
CHECK=0
REPRO=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target=*) TARGET="${1#*=}" ;;
        --target)   TARGET="${2:-}"; shift ;;
        --clean)    CLEAN=1 ;;
        --check)    CHECK=1 ;;
        --repro)    REPRO=1 ;;
        --help|-h)
            grep -E '^#( |!)' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown arg: $1" >&2
            exit 1
            ;;
    esac
    shift
done

# 可重现构建 env(reproducible build standard)
# SOURCE_DATE_EPOCH: 固定时间戳(2023-11-14 22:13:20 UTC)
#   RFC: https://reproducible-builds.org/docs/source-date-epoch/
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1700000000}"
export CARGO_INCREMENTAL=0
# RUSTFLAGS: --build-id=none 让 linker 不写随机 build-id
# (默认 rustc 会写 .note.gnu.build-id 包含时间戳和随机值)
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,--build-id=none"
# 不要 debug 信息段
export CARGO_PROFILE_RELEASE_DEBUG=false
export CARGO_PROFILE_RELEASE_STRIP=true

# 设置交叉编译时的 linker
case "$TARGET" in
    aarch64-unknown-linux-musl)
        # 交叉编译到 aarch64,需要 aarch64-linux-gnu-gcc 作为 linker
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER:-aarch64-linux-gnu-gcc}"
        ;;
esac

if [[ $CLEAN -eq 1 ]]; then
    echo "=== Cleaning ==="
    cargo clean
fi

# 找到真实 binary 路径(过滤掉 cargo 的 warning 噪音)
ACTUAL_BIN=$(cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c "import sys, json; print(json.load(sys.stdin)['target_directory'])" \
    2>/dev/null)
ACTUAL_BIN="$ACTUAL_BIN/$TARGET/release/evorule"

# 检查 target 是否已安装
ensure_target() {
    local t="$1"
    if ! rustup target list --installed 2>/dev/null | grep -q "^$t$"; then
        echo "Target $t not installed. Run: rustup target add $t"
        exit 1
    fi
}

repro_compare() {
    local bin1="$1"
    local bin2="$2"
    local h1 h2
    h1=$(sha256sum "$bin1" | cut -d' ' -f1)
    h2=$(sha256sum "$bin2" | cut -d' ' -f1)
    if [ "$h1" = "$h2" ]; then
        echo "REPRODUCIBLE ✓ $h1"
        echo "  Binary 1: $bin1"
        echo "  Binary 2: $bin2"
        return 0
    else
        echo "NOT REPRODUCIBLE ✗"
        echo "  Binary 1: $h1"
        echo "  Binary 2: $h2"
        echo
        echo "First 10 differing bytes:"
        cmp -l "$bin1" "$bin2" 2>&1 | head -10 || true
        return 1
    fi
}

# 找可写临时目录
TMPDIR_REPRO=$(mktemp -d)
trap "rm -rf $TMPDIR_REPRO" EXIT

if [[ $REPRO -eq 1 ]]; then
    ensure_target "$TARGET"

    echo "=== Reproducible build test for $TARGET ==="
    echo "  SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH"
    echo

    # Build #1
    echo "--- Build #1 ---"
    cargo build --release --target "$TARGET" 2>&1 | tail -3
    cp "$ACTUAL_BIN" "$TMPDIR_REPRO/build1-evorule"

    # Clean and build #2
    echo "--- Build #2 (full clean) ---"
    cargo clean 2>&1 | tail -1 || true
    cargo build --release --target "$TARGET" 2>&1 | tail -3
    cp "$ACTUAL_BIN" "$TMPDIR_REPRO/build2-evorule"

    echo
    echo "--- Compare ---"
    repro_compare "$TMPDIR_REPRO/build1-evorule" "$TMPDIR_REPRO/build2-evorule"
    EXIT=$?
    echo
    echo "Build size: $(du -h "$ACTUAL_BIN" | cut -f1)"
    exit $EXIT
fi

if [[ $CHECK -eq 0 ]]; then
    ensure_target "$TARGET"
    echo "=== Building evorule (musl static, target=$TARGET) ==="
    cargo build --release --target "$TARGET"
fi

if [[ ! -f "$ACTUAL_BIN" ]]; then
    echo "ERROR: binary not found at $ACTUAL_BIN"
    exit 1
fi

echo
echo "=== Binary info ==="
ls -la "$ACTUAL_BIN"
file "$ACTUAL_BIN"
echo
echo "  SHA256: $(sha256sum "$ACTUAL_BIN" | cut -d' ' -f1)"

echo
echo "=== Dynamic dependencies ==="
if ldd "$ACTUAL_BIN" 2>&1 | grep -q "Not a valid dynamic program"; then
    echo "NOT a dynamic program (statically linked)"
elif ldd "$ACTUAL_BIN" 2>&1 | grep -q "statically linked"; then
    echo "statically linked"
else
    ldd "$ACTUAL_BIN"
fi

echo
echo "=== Self-test ==="
"$ACTUAL_BIN" --version
echo
mkdir -p "$TMPDIR_REPRO/rules"
cat > "$TMPDIR_REPRO/rules/rule.json" <<'JSON'
{
  "transform": [
    {"type":"branch","params":{"domain":{"type":"all","inner":[]},"on_true":[]}}
  ]
}
JSON
"$ACTUAL_BIN" validate "$TMPDIR_REPRO/rules"

echo
echo "=== Done ==="
echo "Binary: $ACTUAL_BIN"
echo "Size:   $(du -h "$ACTUAL_BIN" | cut -f1)"
