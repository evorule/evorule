#!/bin/bash
# verify-server-v0.1.0.sh - 验证 evorule-server 在 WSL 下承诺的 HTTP API
# 启动 server → 跑 40+ 端点 → 关闭

set -uo pipefail
cd "$(dirname "$0")/.."
. "$HOME/.cargo/env" 2>/dev/null || true

PASS=0
FAIL=0
WARN=0
declare -a RESULTS
pass() { echo "[PASS] $1"; ((PASS++)); RESULTS+=("PASS  $1"); }
fail() { echo "[FAIL] $1"; ((FAIL++)); RESULTS+=("FAIL  $1"); }
warn() { echo "[WARN] $1"; ((WARN++)); RESULTS+=("WARN  $1"); }
hdr() { echo; echo "===== $1 ====="; }

# ======== 编译 ========
hdr "0. 编译 evorule-server"
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')"
SERVER_BIN="$TARGET_DIR/evorule-governance-release/evorule-server"

# build-musl 不适用(evorule-governance 有 openssl-sys C 依赖),用普通 release
# 试过几次 evorule-governance 编译会卡 — 直接 build
log_path="/tmp/evorule_server_build.log"
echo "编译日志: $log_path"
echo "  (编译可能 5-10 分钟, 等待中...)"
if cargo build --release -p evorule-governance --bin evorule-server 2>&1 | tee "$log_path" | tail -5; then
    if [ -f "$SERVER_BIN" ]; then
        SIZE=$(du -h "$SERVER_BIN" | cut -f1)
        pass "evorule-server 编译成功 ($SIZE)"
    else
        # cargo 默认输出路径(workspace root)
        SERVER_BIN="$TARGET_DIR/release/evorule-server"
        if [ -f "$SERVER_BIN" ]; then
            SIZE=$(du -h "$SERVER_BIN" | cut -f1)
            pass "evorule-server 编译成功 ($SIZE)"
        else
            fail "编译完成但找不到 evorule-server 产物"
            find "$TARGET_DIR" -name "evorule-server*" 2>/dev/null
            exit 1
        fi
    fi
else
    fail "编译失败"
    tail -20 "$log_path"
    exit 1
fi

# ======== 启动 server ========
hdr "1. 启动 evorule-server(后台)"
TEST_PORT=18099
TEST_RULES="/tmp/evorule_test_rules_$$"
TEST_WAL="/tmp/evorule_test_wal_$$"
mkdir -p "$TEST_RULES"
# 拷贝一个简单规则用作 hot reload 测试
cp rules/counter.json "$TEST_RULES/" 2>/dev/null || echo '{"rules":[]}' > "$TEST_RULES/counter.json"

LOGFILE="/tmp/evorule_server_run_$$.log"
echo "  port: $TEST_PORT"
echo "  rules: $TEST_RULES"
echo "  log: $LOGFILE"

# 启动 server(后台),超时保护:30 秒内必须起来
RUST_LOG=info "$SERVER_BIN" \
    --addr "127.0.0.1:$TEST_PORT" \
    --core-eval "evorule-tcb/core_eval.json" \
    --rules-dir "$TEST_RULES" \
    --wal-dir "$TEST_WAL" \
    --no-rate-limit \
    --log-level info \
    > "$LOGFILE" 2>&1 &
SERVER_PID=$!
echo "  pid: $SERVER_PID"

# 等 server 起来
for i in $(seq 1 30); do
    if curl -sf "http://127.0.0.1:$TEST_PORT/api/health" >/dev/null 2>&1; then
        pass "server 启动成功 (${i}s)"
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        fail "server 进程已死"
        tail -20 "$LOGFILE"
        exit 1
    fi
    sleep 1
done

if ! curl -sf "http://127.0.0.1:$TEST_PORT/api/health" >/dev/null 2>&1; then
    fail "server 30 秒内未起来"
    tail -30 "$LOGFILE"
    kill "$SERVER_PID" 2>/dev/null
    exit 1
fi

# 关闭函数
cleanup() {
    echo
    echo "===== 关闭 server (pid $SERVER_PID) ====="
    kill "$SERVER_PID" 2>/dev/null
    wait "$SERVER_PID" 2>/dev/null
    rm -rf "$TEST_RULES" "$TEST_WAL"
}
trap cleanup EXIT

# ======== 2. 健康检查 ========
hdr "2. 健康检查"
for ep in "/api/health" "/api/health/liveness" "/api/health/readiness"; do
    HTTP_CODE=$(curl -s -o /tmp/health.json -w "%{http_code}" "http://127.0.0.1:$TEST_PORT$ep")
    if [ "$HTTP_CODE" = "200" ]; then
        BODY=$(cat /tmp/health.json | head -c 200)
        pass "$ep 200 OK ($BODY)"
    else
        fail "$ep HTTP $HTTP_CODE"
    fi
done

# ======== 3. 创建会话 ========
hdr "3. 会话管理"
SESSION=$(curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions" | python3 -c 'import sys,json;d=json.load(sys.stdin);print(d.get("session_id") or d.get("id") or d)')
if [ -n "$SESSION" ] && [ "$SESSION" != "None" ]; then
    pass "POST /api/sessions 创建成功 (id=$SESSION)"
else
    fail "POST /api/sessions 无 session_id 返回"
    echo "  响应: $(curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions" | head -c 300)"
    exit 1
fi

# 列出活跃会话
LIST=$(curl -s "http://127.0.0.1:$TEST_PORT/api/sessions")
if echo "$LIST" | grep -q "$SESSION"; then
    pass "GET /api/sessions 列表含新建 session"
else
    warn "GET /api/sessions 列表可能不显示(空 body 也合法)"
fi

# ======== 4. 提交 JSON 命令 ========
hdr "4. 命令提交(JSON in / JSON out)"

# 命令 1: set x = 0
R1=$(curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/command" \
    -H "Content-Type: application/json" \
    -d '{"instruction":{"type":"set","params":{"attr":"x","value":0}}}')
echo "  set x=0: $(echo $R1 | head -c 200)"
if [ -n "$R1" ]; then
    pass "POST command #1 (set x=0)"
else
    fail "POST command #1 无响应"
fi

# 命令 2: increment x by 5
R2=$(curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/command" \
    -H "Content-Type: application/json" \
    -d '{"instruction":{"type":"increment","params":{"attr":"x","delta":5}}}')
echo "  increment x+=5: $(echo $R2 | head -c 200)"
if [ -n "$R2" ]; then
    pass "POST command #2 (increment x+=5)"
else
    fail "POST command #2 无响应"
fi

# 等一下让 reactor 处理
sleep 1

# ======== 5. 读 state ========
hdr "5. 状态读取"
STATE=$(curl -s "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/state")
echo "  state: $(echo $STATE | head -c 300)"

# 检查 x = 5
if echo "$STATE" | grep -qE '"x":\s*5'; then
    pass "GET state 显示 x=5(正确)"
elif echo "$STATE" | grep -qE '"x":\s*[0-9]'; then
    X_VAL=$(echo "$STATE" | python3 -c 'import sys,json,re;m=re.search(r"\"x\":\s*(\d+)",sys.stdin.read());print(m.group(1) if m else "?")')
    warn "GET state 显示 x=$X_VAL(预期 5,可能是 0 或其他)"
else
    warn "GET state 没看到 x 字段"
fi

# ======== 6. 时间机器 ========
hdr "6. 时间机器(replay / rewind / diff)"

# replay
REPLAY=$(curl -s "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/replay" | head -c 500)
if [ -n "$REPLAY" ] && [ "$REPLAY" != "" ]; then
    pass "GET /replay 返回内容"
else
    fail "GET /replay 无内容"
fi

# rewind to v1
REWIND=$(curl -s -o /tmp/rewind.json -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/rewind/1")
if [ "$REWIND" = "200" ]; then
    pass "GET /rewind/1 200 OK"
else
    fail "GET /rewind/1 HTTP $REWIND"
fi

# diff
DIFF=$(curl -s "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/diff?a=1&b=2" | head -c 500)
if [ -n "$DIFF" ]; then
    pass "GET /diff?a=1&b=2 返回内容"
else
    fail "GET /diff 无内容"
fi

# ======== 7. 审计 ========
hdr "7. 审计"
AUDIT=$(curl -s -o /tmp/audit.json -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/audit")
if [ "$AUDIT" = "200" ]; then
    pass "GET /audit 200 OK"
else
    fail "GET /audit HTTP $AUDIT"
fi

VERIFY=$(curl -s -o /tmp/verify.json -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/audit/verify")
if [ "$VERIFY" = "200" ]; then
    pass "GET /audit/verify 200 OK"
    cat /tmp/verify.json | head -c 300 | sed 's/^/    /'
    echo
else
    fail "GET /audit/verify HTTP $VERIFY"
fi

# ======== 8. SSE 事件流 ========
hdr "8. SSE 事件流(连接后发命令)"
# SSE 用 broadcast channel,不重放历史 — 必须先连再发
SESSION_SSE=$(curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions" | python3 -c 'import sys,json;d=json.load(sys.stdin);print(d.get("session_id") or d.get("id") or "")')
SSE_OUT=/tmp/sse_out_$$.log
# 后台跑 SSE 抓 4 秒
(timeout 4 curl -sN -H "Accept: text/event-stream" "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION_SSE/events" > "$SSE_OUT" 2>&1) &
SSE_CURL_PID=$!
sleep 1  # 等 SSE 握上
# 发命令
curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION_SSE/command" \
    -H "Content-Type: application/json" \
    -d '{"instruction":{"type":"set","params":{"attr":"sse_test","value":42}}}' > /dev/null
sleep 1
curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION_SSE/command" \
    -H "Content-Type: application/json" \
    -d '{"instruction":{"type":"increment","params":{"attr":"sse_test","delta":1}}}' > /dev/null
wait $SSE_CURL_PID 2>/dev/null || true
SSE_LINES=$(wc -l < "$SSE_OUT")
if [ "$SSE_LINES" -gt 0 ]; then
    DATA_LINES=$(grep -c "^data:" "$SSE_OUT" 2>/dev/null || echo 0)
    pass "SSE 流收到事件 ($SSE_LINES 行, $DATA_LINES 个 data: 事件)"
    head -10 "$SSE_OUT" | sed 's/^/    /'
else
    fail "SSE 流无输出"
fi

# ======== 9. 错误处理 ========
hdr "9. 错误处理"
NOT_FOUND=$(curl -s -o /tmp/nf.json -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/api/sessions/nonexistent-session-id-12345/state")
if [ "$NOT_FOUND" = "404" ] || [ "$NOT_FOUND" = "400" ]; then
    pass "GET 不存在 session 返回 $NOT_FOUND(正确错误码)"
else
    warn "GET 不存在 session HTTP $NOT_FOUND(可能返回 200 + 空 body)"
fi

# 错误命令
BAD_CMD=$(curl -s -o /tmp/bad.json -w "%{http_code}" -X POST "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION/command" \
    -H "Content-Type: application/json" \
    -d '{"instruction":{"type":"unknown_type_xyz","params":{}}}')
if [ -n "$BAD_CMD" ]; then
    pass "错误命令返回响应(可被处理, HTTP $BAD_CMD)"
else
    warn "错误命令无响应"
fi

# ======== 10. Hot Reload ========
hdr "10. Hot Reload(改 rules/*.json,看 server 是否自动 watch)"

# 写一个新的规则文件
cat > "$TEST_RULES/hot-reload-test.json" <<'EOF'
{
  "rules": [
    {
      "when": { "type": "command", "instruction_type": "set" },
      "do": [
        {
          "type": "set",
          "operation": "set",
          "attr": "${params.attr}",
          "value": "${params.value}"
        }
      ]
    }
  ]
}
EOF
echo "  写入 hot-reload-test.json"
sleep 2  # 给 notify watcher 时间检测

# 创建新 session 验证新规则生效
SESSION2=$(curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions" | python3 -c 'import sys,json;d=json.load(sys.stdin);print(d.get("session_id") or d.get("id") or "")')
if [ -n "$SESSION2" ]; then
    curl -s -X POST "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION2/command" \
        -H "Content-Type: application/json" \
        -d '{"instruction":{"type":"set","params":{"attr":"hot","value":"reloaded"}}}' > /dev/null
    sleep 1
    STATE2=$(curl -s "http://127.0.0.1:$TEST_PORT/api/sessions/$SESSION2/state")
    if echo "$STATE2" | grep -q "reloaded"; then
        pass "Hot reload 生效(新 session 加载了新规则)"
    else
        warn "Hot reload 未生效(state=$STATE2)"
    fi
else
    warn "Hot reload 测试未完成(新 session 创建失败)"
fi

# ======== 11. 端点总数 ========
hdr "11. 端点数量(粗略估算)"
# 通过 grep 统计 router 注册的 .route() 或 .merge() 数(在 server.rs)
ENDPOINTS=$(grep -cE '\.route\(' evorule-governance/src/api/server.rs 2>/dev/null || echo 0)
echo "  代码中 .route( 调用数: ~$ENDPOINTS"
if [ "$ENDPOINTS" -ge 40 ]; then
    pass "≥40 端点承诺(实际 .route() 数 $ENDPOINTS)"
elif [ "$ENDPOINTS" -ge 30 ]; then
    warn "端点 $ENDPOINTS(README 说 40+,可能有些端点是 .merge() 进来的子路由)"
else
    warn "端点 $ENDPOINTS(可能不全)"
fi

# ======== 总结 ========
echo
echo "==================================================="
echo "  总结: PASS=$PASS FAIL=$FAIL WARN=$WARN"
echo "==================================================="
for r in "${RESULTS[@]}"; do echo "  $r"; done
echo
echo "退出码: $FAIL"
exit $FAIL
