#!/bin/bash
# verify-all-v0.1.0.sh - 验证 evorule v0.1.0 release 全部承诺
# 必须在 WSL Ubuntu 22.04 下运行
#
# 串接 2 个子验证:
#   1. evorule-cli: musl 静态 + 4 子命令 + hospital/law-firm + diff + 0 网络/AI/遥测 + G8 门控
#   2. evorule-server: HTTP API(40+ 端点) + 时间机器 + 审计 + SSE + hot reload
#
# 退出码 = 两脚本失败数之和(0 = 全过)

set -o pipefail
cd "$(dirname "$0")"
. "$HOME/.cargo/env" 2>/dev/null || true

# ======== 0. 环境检查 ========
echo "==================================================="
echo "  evorule v0.1.0 全量验证(WSL)"
echo "==================================================="
echo

hdr() { echo; echo "===== $1 ====="; }

hdr "0. 环境检查"
echo "  uname:    $(uname -a | cut -d' ' -f1-3)"
echo "  rustc:    $(rustc --version 2>&1)"
echo "  cargo:    $(cargo --version 2>&1)"
echo "  musl-gcc: $(which musl-gcc 2>&1)"

if ! rustc --version >/dev/null 2>&1; then
    echo "[FAIL] rustc 未装,无法继续"
    exit 1
fi
if [ ! -d evorule-cli ]; then
    echo "[FAIL] evorule-cli 目录不存在"
    exit 1
fi
if [ ! -d tier2-governance ]; then
    echo "[FAIL] tier2-governance 目录不存在"
    exit 1
fi

# ======== 1. evorule-cli 验证 ========
hdr "1. evorule-cli 验证(WSL 下跑)"
echo "  脚本: evorule-cli/verify-v0.1.0.sh"
echo "  详细日志: .gate-logs/verify_v0.1.0_*.log"
echo

CLI_LOG=".gate-logs/verify_v0.1.0_all_$(date +%Y%m%d_%H%M%S).log"
bash evorule-cli/verify-v0.1.0.sh 2>&1 | tee "$CLI_LOG" | tail -30
CLI_EXIT=$?
echo
echo "  evorule-cli 验证退出码: $CLI_EXIT"
# 直接读子脚本末尾的 "PASS: N" 行,避免 grep 数错
CLI_PASS=$(grep -E '^PASS: [0-9]+$' "$CLI_LOG" | tail -1 | awk '{print $2}')
CLI_FAIL=$(grep -E '^FAIL: [0-9]+$' "$CLI_LOG" | tail -1 | awk '{print $2}')
CLI_WARN=$(grep -E '^WARN: [0-9]+$' "$CLI_LOG" | tail -1 | awk '{print $2}')
CLI_PASS=${CLI_PASS:-0}
CLI_FAIL=${CLI_FAIL:-0}
CLI_WARN=${CLI_WARN:-0}

# ======== 2. evorule-server 验证 ========
hdr "2. evorule-server 验证(WSL 下跑)"
echo "  脚本: tier2-governance/verify-server-v0.1.0.sh"
echo "  详细日志: .gate-logs/verify_server_v0.1.0_*.log"
echo

SERVER_LOG=".gate-logs/verify_server_v0.1.0_all_$(date +%Y%m%d_%H%M%S).log"
bash tier2-governance/verify-server-v0.1.0.sh 2>&1 | tee "$SERVER_LOG" | tail -30
SERVER_EXIT=$?
echo
echo "  evorule-server 验证退出码: $SERVER_EXIT"
# 子脚本总结行格式: "  总结: PASS=20 FAIL=0 WARN=0"
SERVER_PASS=$(grep -E 'PASS=[0-9]+ FAIL=[0-9]+ WARN=[0-9]+' "$SERVER_LOG" | tail -1 | grep -oE 'PASS=[0-9]+' | grep -oE '[0-9]+')
SERVER_FAIL=$(grep -E 'PASS=[0-9]+ FAIL=[0-9]+ WARN=[0-9]+' "$SERVER_LOG" | tail -1 | grep -oE 'FAIL=[0-9]+' | grep -oE '[0-9]+')
SERVER_WARN=$(grep -E 'PASS=[0-9]+ FAIL=[0-9]+ WARN=[0-9]+' "$SERVER_LOG" | tail -1 | grep -oE 'WARN=[0-9]+' | grep -oE '[0-9]+')
SERVER_PASS=${SERVER_PASS:-0}
SERVER_FAIL=${SERVER_FAIL:-0}
SERVER_WARN=${SERVER_WARN:-0}

# ======== 汇总 ========
hdr "总结"
TOTAL_PASS=$((CLI_PASS + SERVER_PASS))
TOTAL_FAIL=$((CLI_FAIL + SERVER_FAIL))
TOTAL_WARN=$((CLI_WARN + SERVER_WARN))
TOTAL_EXIT=$((CLI_EXIT + SERVER_EXIT))

printf "  %-30s %5s %5s %5s\n" "项目" "PASS" "FAIL" "WARN"
printf "  %-30s %5d %5d %5d\n" "evorule-cli" "$CLI_PASS" "$CLI_FAIL" "$CLI_WARN"
printf "  %-30s %5d %5d %5d\n" "evorule-server" "$SERVER_PASS" "$SERVER_FAIL" "$SERVER_WARN"
echo "  ------------------------------------------------------------"
printf "  %-30s %5d %5d %5d\n" "TOTAL" "$TOTAL_PASS" "$TOTAL_FAIL" "$TOTAL_WARN"
echo
echo "  退出码: $TOTAL_EXIT (0 = 全过, >0 = 有失败)"
echo
echo "  详细日志:"
echo "    - evorule-cli:   $CLI_LOG"
echo "    - evorule-server: $SERVER_LOG"

exit $TOTAL_EXIT
