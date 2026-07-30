#!/usr/bin/env bash
# =============================================================================
# run-kani.sh - evorule-tcb Kani 形式化验证 wrapper
#
# Kani 在 Windows 上官方不支持 (需 Linux/WSL/macOS)。
# 本脚本探测环境并选择合适的运行方式:
#   1) WSL Ubuntu (推荐): 已停/启动后 cargo kani
#   2) Docker: model-checking/kani 镜像
#   3) 本地 Linux/Mac: 直接 cargo kani
#
# 用法:
#   ./scripts/run-kani.sh              # 跑所有 proofs
#   ./scripts/run-kani.sh --proof X    # 跑单个 proof
#   ./scripts/run-kani.sh --list       # 列出所有 proofs
#   ./scripts/run-kani.sh --install    # 安装 Kani (WSL Ubuntu 22.04)
#   ./scripts/run-kani.sh --time       # 显示时间估算
#
# Kani 单个 proof 跑 1min - 24h 不等 (CBMC 状态爆炸)
# 建议先跑 --list + 短 proof 验证环境, 再完整跑
# =============================================================================

set -e

# === Config ===
KANI_VERSION="0.50.0"  # 与 Cargo.toml 一致, 后续需同步
PROOF_DIR="$(cd "$(dirname "$0")/../evorule-tcb" && pwd)"
WSL_DISTRO="Ubuntu-22.04"

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

# === Subcommands ===

cmd_list() {
    echo "Available proofs (from evorule-tcb/tests/kani_proofs.rs):"
    cd "$PROOF_DIR"
    grep -E '^fn verify_' tests/kani_proofs.rs | sed 's/^fn /  /; s/().*//'
}

cmd_install_wsl() {
    log "Installing Kani in WSL distro: $WSL_DISTRO"
    if ! command -v wsl >/dev/null 2>&1; then
        err "wsl not found. Enable WSL first: wsl --install"
        exit 1
    fi
    wsl -d "$WSL_DISTRO" -e bash -c '
        set -e
        echo "==> Installing Kani $KANI_VERSION via cargo install..."
        cargo install --locked kani-verifier --version "$KANI_VERSION" --root ~/.cargo
        echo "==> Installing Kani deps..."
        cargo-kani setup
        echo "==> Verifying kani install..."
        cargo kani --version
    '
    log "Kani installed. Run: $0 (no args) to verify proofs"
}

cmd_run_in_wsl() {
    local proof_arg="${1:-}"
    log "Running Kani via WSL Ubuntu-22.04"
    if ! command -v wsl >/dev/null 2>&1; then
        err "wsl not found"
        return 1
    fi
    # Convert Windows path to WSL path
    local wsl_path
    wsl_path=$(echo "$PROOF_DIR" | sed 's|\\|/|g; s|^\([A-Z]\):|/mnt/\L\1\E|' )
    wsl -d "$WSL_DISTRO" -e bash -c "cd '$wsl_path' && cargo kani $proof_arg"
}

cmd_run_docker() {
    local proof_arg="${1:-}"
    log "Running Kani via Docker (model-checking/kani)"
    if ! command -v docker >/dev/null 2>&1; then
        err "docker not found"
        return 1
    fi
    docker run --rm -v "$PROOF_DIR":/workspace -w /workspace model-checking/kani:latest \
        bash -c "cargo kani $proof_arg"
}

cmd_run_native() {
    local proof_arg="${1:-}"
    log "Running Kani natively (Linux/macOS)"
    if ! command -v cargo-kani >/dev/null 2>&1; then
        err "cargo-kani not in PATH. Install with:"
        err "  cargo install --locked kani-verifier --version $KANI_VERSION"
        err "  cargo-kani setup"
        exit 1
    fi
    cd "$PROOF_DIR"
    cargo kani $proof_arg
}

# === Main ===
main() {
    cd "$PROOF_DIR" || { err "Cannot cd to $PROOF_DIR"; exit 1; }

    case "${1:-}" in
        --list|-l)
            cmd_list
            ;;
        --install)
            cmd_install_wsl
            ;;
        --docker)
            shift
            cmd_run_docker "$*"
            ;;
        --help|-h)
            sed -n '2,30p' "$0" | sed 's/^# \?//'
            ;;
        "")
            # Default: detect platform and run
            case "$(detect_platform)" in
                linux|macos)
                    cmd_run_native
                    ;;
                wsl-or-windows)
                    log "Windows detected. Trying WSL Ubuntu-22.04..."
                    if wsl -d "$WSL_DISTRO" -e bash -c "command -v cargo-kani" >/dev/null 2>&1; then
                        cmd_run_wsl
                    else
                        err "Kani not installed in WSL. Run: $0 --install"
                        echo "  (or use Docker: $0 --docker)" >&2
                        exit 1
                    fi
                    ;;
            esac
            ;;
        *)
            # Treat as proof name
            cmd_run_wsl "--proof $1"
            ;;
    esac
}

main "$@"
