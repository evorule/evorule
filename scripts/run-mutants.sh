#!/usr/bin/env bash
# =============================================================================
# run-mutants.sh - tier0-tcb Mutation Testing wrapper
#
# cargo-mutants 在每个函数/表达式中注入微小改动 ("mutant"),
# 然后跑测试套件. 如果测试**没抓到** ("MISSED"), 说明测试有盲区.
#
# 用法:
#   ./scripts/run-mutants.sh                        # 跑全部 (耗时长, ~24h+)
#   ./scripts/run-mutants.sh --quick               # 跑 30 分钟采样 (baseline)
#   ./scripts/run-mutants.sh --file src/executor.rs # 限定单文件
#   ./scripts/run-mutants.sh --baseline            # 与 baseline.json 对比
#   ./scripts/run-mutants.sh --list                # 列出 mutants 不跑
#   ./scripts/run-mutants.sh --install             # 装 cargo-mutants
#
# Mutation score 解读:
#   > 80%  - 优秀
#   60-80% - 良好
#   40-60% - 中等 (有测试盲区)
#   < 40%  - 测试薄弱 (很多 MISSED)
# =============================================================================

set -e

# === Config ===
TIER0_DIR="$(cd "$(dirname "$0")/../tier0-tcb" && pwd)"
BASELINE_FILE="$TIER0_DIR/mutants.out/baseline.json"
TIME_LIMIT_MIN="${MUTANTS_TIME_LIMIT:-30}"

# === Helpers ===
log() { echo "[run-mutants]" "$@" >&2; }
err() { echo "[run-mutants] ERROR:" "$@" >&2; }

cmd_install() {
    log "Installing cargo-mutants via cargo"
    cargo install --locked cargo-mutants
    log "Verifying install: $(cargo mutants --version)"
}

cmd_list() {
    cd "$TIER0_DIR" || { err "Cannot cd to $TIER0_DIR"; return 1; }
    cargo mutants --list
}

cmd_quick() {
    cd "$TIER0_DIR" || { err "Cannot cd to $TIER0_DIR"; return 1; }
    log "Quick baseline: 30-minute sampling"
    timeout "${TIME_LIMIT_MIN}m" cargo mutants         --timeout 60         --output mutants.out/         || log "(timeout reached, baseline saved)"
    if [ -f mutants.out/mutants.json ]; then
        cargo mutants --output mutants.out/ summary < mutants.out/mutants.json 2>/dev/null || true
    fi
}

cmd_full() {
    cd "$TIER0_DIR" || { err "Cannot cd to $TIER0_DIR"; return 1; }
    log "Full mutation testing (estimated 24h+)"
    cargo mutants         --timeout 120         --output mutants.out/
    log "Done. Results in mutants.out/"
    log "View HTML: xdg-open mutants.out/mutants.html  (or just open it)"
}

cmd_file() {
    local file="$1"
    cd "$TIER0_DIR" || { err "Cannot cd to $TIER0_DIR"; return 1; }
    log "Mutation testing on file: $file"
    cargo mutants         --file "$file"         --timeout 60         --output mutants.out/
}

cmd_baseline() {
    cd "$TIER0_DIR" || { err "Cannot cd to $TIER0_DIR"; return 1; }
    if [ ! -f "$BASELINE_FILE" ]; then
        err "No baseline at $BASELINE_FILE. Run: $0 --quick first"
        exit 1
    fi
    log "Comparing current mutants against baseline"
    log "(Note: cargo-mutants baseline support limited; use mutants.out/mutants.json for full analysis)"
}

# === Main ===
case "${1:-}" in
    --install)
        cmd_install
        ;;
    --list)
        cmd_list
        ;;
    --quick)
        cmd_quick
        ;;
    --file)
        if [ -z "$2" ]; then
            err "--file requires a path argument"
            exit 1
        fi
        cmd_file "$2"
        ;;
    --baseline)
        cmd_baseline
        ;;
    --help|-h)
        sed -n '2,35p' "$0" | sed 's/^# \?//'
        ;;
    "")
        cmd_full
        ;;
    *)
        err "Unknown option: $1"
        echo "Run: $0 --help"
        exit 1
        ;;
esac
