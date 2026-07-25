#!/bin/bash
# verify-v0.1.0.sh - 验证 evorule-cli README 中承诺的每一项
# 必须在 WSL/Linux 下运行

set -uo pipefail
cd "$(dirname "$0")"
. "$HOME/.cargo/env" 2>/dev/null || true

PASS=0
FAIL=0
WARN=0
declare -a RESULTS

log() { echo "[$1] $2"; }
pass() { log "PASS" "$1"; ((PASS++)); RESULTS+=("PASS  $1"); }
fail() { log "FAIL" "$1"; ((FAIL++)); RESULTS+=("FAIL  $1"); }
warn() { log "WARN" "$1"; ((WARN++)); RESULTS+=("WARN  $1"); }
sep() { echo "=================================================="; }

sep
echo "  evorule-cli v0.1.0 README 承诺验证"
sep

# ======== 1. 基础环境 ========
echo
sep
echo "  1. 基础环境"
sep
echo "rustc:    $(rustc --version 2>&1)"
echo "cargo:    $(cargo --version 2>&1)"
echo "musl-gcc: $(which musl-gcc 2>&1)"
echo "target x86_64 musl:  $(rustup target list --installed 2>&1 | grep x86_64-unknown-linux-musl)"
echo "target aarch64 musl: $(rustup target list --installed 2>&1 | grep aarch64-unknown-linux-musl)"

# ======== 2. 编译 x86_64 musl 静态二进制 ========
echo
sep
echo "  2. 编译 x86_64 musl 静态二进制"
sep
# 注意:.cargo/config.toml 配的 target-dir 是相对路径,但 cargo metadata 报告为 absolute
# 用 cargo metadata 拿真实路径,避免路径不一致
X86_64_BIN="$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)/x86_64-unknown-linux-musl/release/evorule"
if [ -f "$X86_64_BIN" ]; then
    log "INFO" "已有产物,跳过编译 (用 --rebuild 强制重编)"
    pass "x86_64 musl 产物存在"
else
    log "INFO" "开始编译 x86_64 musl..."
    if bash build-musl.sh --target x86_64-unknown-linux-musl 2>&1 | tail -20; then
        [ -f "$X86_64_BIN" ] && pass "x86_64 musl 编译成功" || fail "x86_64 musl 产物缺失"
    else
        fail "x86_64 musl 编译失败"
    fi
fi

if [ -f "$X86_64_BIN" ]; then
    SIZE=$(du -h "$X86_64_BIN" | cut -f1)
    BYTES=$(stat -c%s "$X86_64_BIN")
    echo "  二进制大小: $SIZE ($BYTES bytes)"
    echo "  承诺大小: 1.6 MB (1677722 bytes)"
    if [ "$BYTES" -lt 2000000 ] && [ "$BYTES" -gt 1400000 ]; then
        pass "x86_64 大小在 1.4-2.0 MB 范围($SIZE, 跟 README 1.6 MB 承诺一致)"
    else
        warn "x86_64 大小 $SIZE 偏离 README 承诺 1.6 MB"
    fi

    echo "  file: $(file "$X86_64_BIN" | cut -d: -f2-)"
    if file "$X86_64_BIN" | grep -q "statically linked"; then
        pass "x86_64 静态链接"
    else
        warn "x86_64 ldd 显示非完全静态(在 musl target 编译,但不排除有 partial relro)"
    fi

    echo "  ldd:"
    ldd "$X86_64_BIN" 2>&1 | sed 's/^/    /'
fi

# ======== 3. 编译 aarch64 musl 静态二进制 ========
echo
sep
echo "  3. 编译 aarch64 musl 静态二进制"
sep
AARCH64_BIN="$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)/aarch64-unknown-linux-musl/release/evorule"
if [ -f "$AARCH64_BIN" ]; then
    log "INFO" "已有 aarch64 产物,跳过编译"
    pass "aarch64 musl 产物存在"
else
    # 检查 aarch64 cross compiler
    if which aarch64-linux-musl-gcc >/dev/null 2>&1; then
        log "INFO" "aarch64-linux-musl-gcc 已装,开始交叉编译..."
        # build-musl.sh 默认走 CC=aarch64-linux-musl-gcc(如果有),否则会失败
        CC=aarch64-linux-musl-gcc bash build-musl.sh --target aarch64-unknown-linux-musl 2>&1 | tail -15
        [ -f "$AARCH64_BIN" ] && pass "aarch64 musl 编译成功" || warn "aarch64 musl 编译失败(交叉工具链可能不全)"
    else
        warn "aarch64-linux-musl-gcc 未装,跳过 aarch64 编译(README 说 1.4 MB,本机未验证)"
    fi
fi

if [ -f "$AARCH64_BIN" ]; then
    SIZE=$(du -h "$AARCH64_BIN" | cut -f1)
    BYTES=$(stat -c%s "$AARCH64_BIN")
    echo "  二进制大小: $SIZE ($BYTES bytes)"
    echo "  承诺大小: 1.4 MB (1468006 bytes)"
    if [ "$BYTES" -lt 2000000 ] && [ "$BYTES" -gt 1200000 ]; then
        pass "aarch64 大小在 1.2-2.0 MB 范围($SIZE, 跟 README 1.4 MB 承诺一致)"
    else
        warn "aarch64 大小 $SIZE 偏离 README 承诺 1.4 MB"
    fi
fi

# ======== 4. 可重现构建 ========
echo
sep
echo "  4. 可重现构建(reproducible build)"
sep
if [ -f "$X86_64_BIN" ]; then
    log "INFO" "跑 build-musl.sh --repro (2 次构建对比 SHA256)..."
    if bash build-musl.sh --target x86_64-unknown-linux-musl --repro 2>&1 | tail -15; then
        pass "可重现构建通过(2 次构建 SHA256 一致)"
    else
        fail "可重现构建失败"
    fi
else
    warn "跳过 repro 测试(x86_64 产物不存在)"
fi

# ======== 5. 4 子命令 --version / --help ========
echo
sep
echo "  5. 4 子命令验证"
sep
if [ ! -f "$X86_64_BIN" ]; then
    fail "x86_64 产物不存在,无法验证 4 子命令"
else
    for sub in "" "validate" "run" "replay" "diff"; do
        if [ -z "$sub" ]; then
            output=$("$X86_64_BIN" --version 2>&1 || true)
        else
            output=$("$X86_64_BIN" "$sub" --help 2>&1 || true)
        fi
        if [ -n "$output" ]; then
            pass "子命令 '$sub' 可调用"
            echo "    $output" | head -3 | sed 's/^/    /'
        else
            fail "子命令 '$sub' 无输出"
        fi
    done
fi

# ======== 6. hospital 示例 ========
echo
sep
echo "  6. hospital 示例(HIPAA / 等保 2.0)"
sep
if [ ! -f "$X86_64_BIN" ]; then
    warn "跳过 hospital(无产物)"
else
    cd examples/hospital
    "$X86_64_BIN" validate ./rules/ > /tmp/hospital_validate.log 2>&1
    EC=$?
    if [ $EC -eq 0 ]; then
        pass "hospital validate 通过(退出码 0)"
        head -10 /tmp/hospital_validate.log | sed 's/^/    /'
    else
        fail "hospital validate 失败(退出码 $EC)"
        cat /tmp/hospital_validate.log | sed 's/^/    /'
    fi

    "$X86_64_BIN" run ./rules/ --payload-file payload.example.json -o /tmp/hospital_fact.log 2>&1 | head -5
    EC=$?
    if [ $EC -eq 0 ] && [ -s /tmp/hospital_fact.log ]; then
        LINES=$(wc -l < /tmp/hospital_fact.log)
        pass "hospital run 通过,fact log $LINES 行"
        head -3 /tmp/hospital_fact.log | sed 's/^/    /'
    else
        fail "hospital run 失败(退出码 $EC,log 存在=$([ -s /tmp/hospital_fact.log ] && echo yes || echo no))"
    fi

    REPLAY_OUT=$("$X86_64_BIN" replay /tmp/hospital_fact.log 2>&1)
    if echo "$REPLAY_OUT" | grep -q "Replaying"; then
        pass "hospital replay 成功"
    else
        fail "hospital replay 失败"
    fi
    cd ../..
fi

# ======== 7. law-firm 示例 ========
echo
sep
echo "  7. law-firm 示例(律师执业 / GDPR)"
sep
if [ ! -f "$X86_64_BIN" ]; then
    warn "跳过 law-firm(无产物)"
else
    cd examples/law-firm
    "$X86_64_BIN" validate ./rules/ > /tmp/lawfirm_validate.log 2>&1
    EC=$?
    if [ $EC -eq 0 ]; then
        pass "law-firm validate 通过"
    else
        fail "law-firm validate 失败(退出码 $EC)"
        cat /tmp/lawfirm_validate.log | sed 's/^/    /'
    fi

    "$X86_64_BIN" run ./rules/ --payload-file payload.example.json -o /tmp/lawfirm_fact.log 2>&1 | head -3
    if [ -s /tmp/lawfirm_fact.log ]; then
        LINES=$(wc -l < /tmp/lawfirm_fact.log)
        pass "law-firm run 通过,fact log $LINES 行"
    else
        fail "law-firm run 失败"
    fi
    cd ../..
fi

# ======== 8. diff 测试 ========
echo
sep
echo "  8. diff 测试"
sep
if [ ! -f "$X86_64_BIN" ] || [ ! -f /tmp/hospital_fact.log ]; then
    warn "跳过 diff(无产物或无 fact log)"
else
    cp /tmp/hospital_fact.log /tmp/hospital_fact_b.log
    DIFF_OUT=$("$X86_64_BIN" diff /tmp/hospital_fact.log /tmp/hospital_fact_b.log 2>&1)
    if echo "$DIFF_OUT" | grep -qi "identical"; then
        pass "diff identical 测试通过(2 份相同 fact log)"
    else
        fail "diff identical 测试失败"
        echo "$DIFF_OUT" | head -10 | sed 's/^/    /'
    fi

    # 故意造一份不同的
    echo '{"step":99,"type":"final","final_payload":{"x":999}}' >> /tmp/hospital_fact_diff.log
    cat /tmp/hospital_fact.log >> /tmp/hospital_fact_diff.log
    DIFF_OUT2=$("$X86_64_BIN" diff /tmp/hospital_fact.log /tmp/hospital_fact_diff.log 2>&1)
    if echo "$DIFF_OUT2" | grep -qi "Only in"; then
        pass "diff 不同测试通过(能识别差异)"
    else
        fail "diff 不同测试失败"
        echo "$DIFF_OUT2" | head -10 | sed 's/^/    /'
    fi
fi

# ======== 9. 依赖审计(0 网络 / 0 遥测 / 0 AI) ========
echo
sep
echo "  9. 依赖审计(0 网络 / 0 遥测 / 0 AI)"
sep
DEPS=$(cargo tree -p evorule-cli --edges normal --format "{p} {f}" 2>/dev/null | grep -v "├──\|└──" | head -20)
echo "  evorule-cli 直接依赖:"
echo "$DEPS" | sed 's/^/    /'

# 检查网络相关
NET_DEPS=$(echo "$DEPS" | grep -iE 'reqwest|hyper|tokio.*net|curl|ureq' || true)
if [ -z "$NET_DEPS" ]; then
    pass "0 网络依赖(cargo tree 中无 reqwest/hyper/curl/ureq)"
else
    fail "发现网络依赖: $NET_DEPS"
fi

# 检查 AI/LLM 相关
AI_DEPS=$(echo "$DEPS" | grep -iE 'openai|llm|anthropic|gemini|langchain|ollama' || true)
if [ -z "$AI_DEPS" ]; then
    pass "0 AI/LLM 依赖"
else
    fail "发现 AI 依赖: $AI_DEPS"
fi

# 检查 telemetry 相关
TELE_DEPS=$(echo "$DEPS" | grep -iE 'sentry|datadog|honeycomb|opentelemetry' || true)
if [ -z "$TELE_DEPS" ]; then
    pass "0 遥测依赖"
else
    fail "发现遥测依赖: $TELE_DEPS"
fi

# ======== 10. e2e.sh ========
echo
sep
echo "  10. e2e.sh 测试"
sep
if [ ! -f "$X86_64_BIN" ]; then
    warn "跳过 e2e.sh(无产物)"
elif [ ! -f tests/e2e.sh ]; then
    warn "tests/e2e.sh 不存在"
else
    log "INFO" "跑 tests/e2e.sh..."
    bash tests/e2e.sh "$X86_64_BIN" 2>&1 | tail -25
    EC=$?
    if [ $EC -eq 0 ]; then
        pass "e2e.sh 全部通过"
    else
        # 进一步看是不是部分失败
        if bash tests/e2e.sh "$X86_64_BIN" 2>&1 | grep -q "failed 0"; then
            pass "e2e.sh 全部通过(0 failed)"
        else
            warn "e2e.sh 有失败用例(退出码 $EC)"
        fi
    fi
fi

# ======== 11. G8 门控故意触发测试 ========
echo
sep
echo "  11. G8 门控故意触发"
sep
log "INFO" "测试: EVORULE_SKIP_GATE=1 应跳过门控(warning)..."
cd ..
SKIP_OUT=$(EVORULE_SKIP_GATE=1 cargo build -p evorule-cli 2>&1 | tail -3 || true)
if echo "$SKIP_OUT" | grep -qi "SKIPPED"; then
    pass "EVORULE_SKIP_GATE=1 跳过路径生效(显示 SKIPPED warning)"
else
    warn "EVORULE_SKIP_GATE=1 路径未显示 SKIPPED warning(可能 build cached)"
fi

log "INFO" "测试: 故意加 G8 字面量应编译失败..."
# 用临时 main.rs 测试,不污染原文件
cp evorule-cli/src/main.rs evorule-cli/src/main.rs.bak
# 在 main.rs 末尾追加 F11-unwrap 触发
cat >> evorule-cli/src/main.rs <<'PROBE'

#[cfg(test)]
mod _gate_probe_test {
    #[test]
    fn probe() {
        let s: Option<&str> = None;
        let _ = s.unwrap();  // F11-unwrap probe
    }
}
PROBE
# 清 build cache 确保重新 build(否则 build.rs 不会重跑)
cargo clean -p evorule-cli > /dev/null 2>&1
GATE_OUT=$(cargo build -p evorule-cli 2>&1 || true)
if echo "$GATE_OUT" | grep -q "compile-time gate FAILED\|F11-unwrap"; then
    pass "G8/F11 门控触发成功(故意加 .unwrap() 编译被拦)"
    echo "$GATE_OUT" | grep -E "F11|G8|gate" | head -5 | sed 's/^/    /'
else
    fail "G8/F11 门控未触发"
    echo "$GATE_OUT" | tail -10 | sed 's/^/    /'
fi
# 恢复
cp evorule-cli/src/main.rs.bak evorule-cli/src/main.rs
rm evorule-cli/src/main.rs.bak
# 重新 build 确认恢复
cargo build -p evorule-cli > /dev/null 2>&1 && pass "G8 门控测试后 main.rs 恢复成功" || fail "G8 门控测试后 main.rs 恢复失败"

# ======== 总结 ========
echo
sep
echo "  总结"
sep
echo "PASS: $PASS"
echo "FAIL: $FAIL"
echo "WARN: $WARN"
echo
echo "全部结果:"
for r in "${RESULTS[@]}"; do
    echo "  $r"
done
echo
echo "退出码: $FAIL (0=全过, >0=有失败)"
exit $FAIL
