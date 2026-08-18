# SPDX-License-Identifier: AGPL-3.0-or-later
#!/usr/bin/env bash
# =============================================================================
# run-kani.sh - evorule Kani 形式化验证统一入口
#
# 支持 evorule-reactor (11 proofs,CI 子集 3 个)。
# 注意: evorule-tcb 的 Kani 验证已移除 (旧 12 proofs 存在缺陷), 待重建后再纳入。
# Kani 在 Windows 上官方不支持 (需 Linux/WSL/macOS)。
# 本脚本探测环境并选择合适的运行方式:
#   1) WSL Ubuntu (推荐): 已停/启动后 cargo kani
#   2) Docker: model-checking/kani 镜像
#   3) 本地 Linux/Mac: 直接 cargo kani
#
# 用法:
#   ./scripts/run-kani.sh                              # 跑 evorule-reactor 所有 proofs
#   ./scripts/run-kani.sh --crate evorule-reactor      # 跑 reactor 所有 proofs
#   ./scripts/run-kani.sh --harness verify_value_roundtrip   # 跑单个 proof
#   ./scripts/run-kani.sh --crate evorule-reactor --harness max_rounds_termination
#   ./scripts/run-kani.sh --list                       # 列出 reactor proofs
#   ./scripts/run-kani.sh --install                    # 安装 Kani (WSL Ubuntu 22.04)
#   ./scripts/run-kani.sh --docker                     # 用 Docker 跑
#
# Kani 版本: 0.67.0 (与 rust-toolchain.toml + CI 一致)
# 单个 proof 耗时 3s - 600s 不等 (CBMC 状态爆炸时可能超时)
# =============================================================================

set -e

# === Config ===
KANI_VERSION="0.67.0"  # 与 WSL 实测环境 + CI + rust-toolchain.toml 一致
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WSL_DISTRO="Ubuntu-22.04"
CRATE="evorule-reactor"  # 默认 crate (tcb 已移除 Kani, 待重建)

# === Helpers ===
log() { echo "[run-kani]" "$@" >&2; }
err() { echo "[run-kani] ERROR:" "$@" >&2; }

detect_platform() {
    case "$(uname -s 2>/dev/null || echo Windows)" in
        Linux*)     echo "linux";;
        Darwin*)    echo "macos";;
        *)          echo "wsl-or-windows";;
    esac
}

# 将 Windows 路径转为 WSL 路径
to_wsl_path() {
    echo "$1" | sed 's|\\|/|g; s|^\([A-Za-z]\):|/mnt/\L\1|'
}

# === Subcommands ===

cmd_list() {
    local crate="$CRATE"
    local proof_file="$WORKSPACE_DIR/$crate/verification/kani_proofs.rs"
    if [ ! -f "$proof_file" ]; then
        err "Proof file not found: $proof_file"
        exit 1
    fi
    echo "Available proofs ($crate/verification/kani_proofs.rs):"
    grep -A1 '#\[kani::proof' "$proof_file" | grep -E '^(pub )?fn ' | sed 's/^pub fn /  /; s/^fn /  /; s/().*//'
}

cmd_install_wsl() {
    log "Installing Kani $KANI_VERSION in WSL distro: $WSL_DISTRO"
    if ! command -v wsl >/dev/null 2>&1; then
        err "wsl not found. Enable WSL first: wsl --install"
        exit 1
    fi
    wsl -d "$WSL_DISTRO" -e bash -c "
        set -e
        echo '==> Installing Kani $KANI_VERSION via cargo install...'
        cargo install --locked kani-verifier --version '$KANI_VERSION' --root ~/.cargo
        echo '==> Installing Kani deps...'
        cargo-kani setup
        echo '==> Verifying kani install...'
        cargo kani --version
    "
    log "Kani installed. Run: $0 (no args) to verify proofs"
}

cmd_run_in_wsl() {
    local harness_arg="$1"
    log "Running Kani via WSL $WSL_DISTRO (crate=$CRATE)"
    if ! command -v wsl >/dev/null 2>&1; then
        err "wsl not found"
        return 1
    fi
    local wsl_workspace
    wsl_workspace=$(to_wsl_path "$WORKSPACE_DIR")
    wsl -d "$WSL_DISTRO" -e bash -c "
        cd '$wsl_workspace'
        export PATH=\"\$HOME/.cargo/bin:\$PATH\"
        cargo kani -p '$CRATE' $harness_arg --output-format=terse
    "
}

cmd_run_docker() {
    local harness_arg="$1"
    log "Running Kani via Docker (model-checking/kani), crate=$CRATE"
    if ! command -v docker >/dev/null 2>&1; then
        err "docker not found"
        return 1
    fi
    docker run --rm -v "$WORKSPACE_DIR":/workspace -w /workspace model-checking/kani:latest \
        bash -c "cargo kani -p '$CRATE' $harness_arg --output-format=terse"
}

cmd_run_native() {
    local harness_arg="$1"
    log "Running Kani natively (Linux/macOS), crate=$CRATE"
    if ! command -v cargo-kani >/dev/null 2>&1; then
        err "cargo-kani not in PATH. Install with:"
        err "  cargo install --locked kani-verifier --version $KANI_VERSION"
        err "  cargo-kani setup"
        exit 1
    fi
    cd "$WORKSPACE_DIR"
    cargo kani -p "$CRATE" $harness_arg --output-format=terse
}

# === Argument Parsing ===

HARNESS=""
mode="run"  # run | list | install | docker | help

while [ $# -gt 0 ]; do
    case "$1" in
        --crate|-c)
            CRATE="$2"
            shift 2
            ;;
        --harness|--proof)
            HARNESS="$2"
            shift 2
            ;;
        --list|-l)
            mode="list"
            shift
            ;;
        --install)
            mode="install"
            shift
            ;;
        --docker)
            mode="docker"
            shift
            ;;
        --help|-h)
            mode="help"
            shift
            ;;
        *)
            err "Unknown argument: $1"
            exit 1
            ;;
    esac
done

# Validate crate
case "$CRATE" in
    evorule-reactor) ;;
    evorule-tcb)
        err "evorule-tcb 的 Kani 验证已移除 (旧 12 proofs 存在缺陷), 待重建后再纳入。"
        err "当前支持: evorule-reactor (11 proofs)。"
        exit 1
        ;;
    *)
        err "Invalid crate: $CRATE (expected: evorule-reactor)"
        exit 1
        ;;
esac

# Build harness argument
HARNESS_ARG=""
if [ -n "$HARNESS" ]; then
    HARNESS_ARG="--harness $HARNESS"
fi

# === Main ===

case "$mode" in
    list)
        cmd_list
        ;;
    install)
        cmd_install_wsl
        ;;
    help)
        sed -n '2,30p' "$0" | sed 's/^# \?//'
        ;;
    docker)
        cmd_run_docker "$HARNESS_ARG"
        ;;
    run)
        case "$(detect_platform)" in
            linux|macos)
                cmd_run_native "$HARNESS_ARG"
                ;;
            wsl-or-windows)
                log "Windows detected. Trying WSL $WSL_DISTRO..."
                if wsl -d "$WSL_DISTRO" -e bash -c "command -v cargo-kani" >/dev/null 2>&1; then
                    cmd_run_in_wsl "$HARNESS_ARG"
                else
                    err "Kani not installed in WSL. Run: $0 --install"
                    echo "  (or use Docker: $0 --docker)" >&2
                    exit 1
                fi
                ;;
        esac
        ;;
esac
