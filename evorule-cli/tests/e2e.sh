#!/bin/bash
# e2e.sh - End-to-end test for evorule CLI
#
# Tests all 5 subcommands (validate / run / replay / diff / verify-chain)
# with realistic rule fixtures. Output in TAP format for CI integration.
#
# # 测试覆盖
# - validate: valid/invalid/unknown-type/empty/nonexistent
# - run: stdout/payload/payload-file/-o/max-steps
# - replay: normal/nonexistent
# - diff: identical/different
# - verify-chain: valid/tampered
# - FIFO queue order (push [step1,step2] → step1 executes first)
# - deterministic loading (multi-file sorted by filename)
# - JSONL integrity (every line valid JSON)
# - examples: hospital + law-firm (validate + run)
#
# Usage:
#   bash tests/e2e.sh                                # auto-detect binary
#   bash tests/e2e.sh /path/to/evorule               # explicit binary path
#
# Exit code:
#   0 = all tests passed
#   1 = at least one test failed
#   2 = environment error (binary not found, etc.)

set -u

# ===== Setup =====
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FIXTURES_DIR="$SCRIPT_DIR/fixtures"
WORK_DIR=$(mktemp -d)
trap "rm -rf $WORK_DIR" EXIT

# Find binary
BIN="${1:-}"
if [[ -z "$BIN" ]]; then
    # Auto-detect: check common locations (relative to workspace root — $SCRIPT_DIR/../..)
    WORKSPACE_ROOT="$SCRIPT_DIR/../.."
    for candidate in \
        "$WORKSPACE_ROOT/.build/rust/x86_64-unknown-linux-musl/release/evorule" \
        "$WORKSPACE_ROOT/.build/rust/aarch64-unknown-linux-musl/release/evorule" \
        "$WORKSPACE_ROOT/.build/rust/release/evorule.exe" \
        "$WORKSPACE_ROOT/.build/rust/release/evorule" \
        "$WORKSPACE_ROOT/target/x86_64-unknown-linux-musl/release/evorule" \
        "$WORKSPACE_ROOT/target/aarch64-unknown-linux-musl/release/evorule" \
        "$WORKSPACE_ROOT/target/release/evorule.exe" \
        "$WORKSPACE_ROOT/target/release/evorule" \
        "./target/release/evorule" \
        "./evorule" \
    ; do
        if [[ -x "$candidate" ]] || [[ -f "$candidate" ]]; then
            BIN="$candidate"
            break
        fi
    done
fi

if [[ -z "$BIN" || ! -f "$BIN" ]]; then
    echo "ERROR: evorule binary not found." >&2
    echo "Usage: $0 [path-to-evorule-binary]" >&2
    exit 2
fi

# Make binary executable (WSL /mnt/* often lacks +x)
chmod +x "$BIN" 2>/dev/null || true

# Test counter
TOTAL=0
PASSED=0
FAILED=0

# ===== TAP output helpers =====
tap_ok() {
    TOTAL=$((TOTAL+1))
    PASSED=$((PASSED+1))
    echo "ok $TOTAL - $1"
}

tap_not_ok() {
    TOTAL=$((TOTAL+1))
    FAILED=$((FAILED+1))
    echo "not ok $TOTAL - $1"
    if [[ -n "${2:-}" ]]; then
        echo "  ---"
        echo "  $2"
        echo "  ---"
    fi
}

# Assert helper: command should succeed (exit 0) and output should contain substring
assert_cmd() {
    local desc="$1"
    local expected_exit="$2"
    local needle="$3"
    shift 3
    local out
    local rc
    # Capture exit code BEFORE any fallback (|| true would mask it)
    out=$("$@" 2>&1)
    rc=$?

    if [[ "$rc" == "$expected_exit" ]] && { [[ -z "$needle" ]] || [[ "$out" == *"$needle"* ]]; }; then
        tap_ok "$desc"
    else
        local detail
        detail=$(printf "exit=%d (want %d) | output:\n%s" "$rc" "$expected_exit" "$out" | head -10)
        tap_not_ok "$desc" "$detail"
    fi
}

# ===== Test 1: --version =====
echo "# evorule e2e test (v0.2.0)"
echo "# binary: $BIN"
echo "#"

assert_cmd "--version exits 0" \
    "0" \
    "evorule" \
    "$BIN" --version

# ===== Test 2: --help =====
assert_cmd "--help shows subcommands" \
    "0" \
    "verify-chain" \
    "$BIN" --help

# ===== Test 3: validate valid rule =====
assert_cmd "validate valid rule exits 0" \
    "0" \
    "[OK]" \
    "$BIN" validate "$FIXTURES_DIR/valid"

# ===== Test 4: validate invalid rule (missing type) =====
assert_cmd "validate invalid rule exits 1" \
    "1" \
    "[ERROR]" \
    "$BIN" validate "$FIXTURES_DIR/invalid"

# ===== Test 5: validate unknown type (ERROR, not WARN) =====
assert_cmd "validate unknown-type errors" \
    "1" \
    "[ERROR]" \
    "$BIN" validate "$FIXTURES_DIR/unknown-type"

# ===== Test 6: validate empty directory (exit 2) =====
assert_cmd "validate empty dir exits 2" \
    "2" \
    "No .json files" \
    "$BIN" validate "$FIXTURES_DIR/empty"

# ===== Test 7: validate nonexistent directory (exit 2) =====
assert_cmd "validate nonexistent dir exits 2" \
    "2" \
    "does not exist" \
    "$BIN" validate "$WORK_DIR/nonexistent"

# ===== Test 8: run valid rule (stdout) =====
assert_cmd "run valid rule exits 0 with Stable fact" \
    "0" \
    "Stable" \
    "$BIN" run "$FIXTURES_DIR/valid"

# ===== Test 9: run with --payload =====
assert_cmd "run with --payload exits 0" \
    "0" \
    "Stable" \
    "$BIN" run "$FIXTURES_DIR/valid" --payload '{"x": 0}'

# ===== Test 10: run with --payload-file =====
cat > "$WORK_DIR/payload.json" <<'JSON'
{"x": 100, "y": "hello"}
JSON
assert_cmd "run with --payload-file exits 0" \
    "0" \
    "Stable" \
    "$BIN" run "$FIXTURES_DIR/valid" --payload-file "$WORK_DIR/payload.json"

# ===== Test 11: run with -o output file (JSONL format) =====
OUT1="$WORK_DIR/fact1.log"
"$BIN" run "$FIXTURES_DIR/valid" -o "$OUT1" >/dev/null 2>&1
if [[ -f "$OUT1" ]] && [[ -s "$OUT1" ]]; then
    # Check first line is valid JSON and contains "type":"Command"
    first_line=$(head -1 "$OUT1")
    if command -v python3 >/dev/null 2>&1; then
        if echo "$first_line" | python3 -c "import json,sys; d=json.loads(sys.stdin.read()); assert d.get('type')=='Command'" 2>/dev/null; then
            tap_ok "run -o writes JSONL with Command first line"
        else
            tap_not_ok "run -o writes JSONL with Command first line" "first line: $first_line"
        fi
    else
        if [[ "$first_line" == *'"type":"Command"'* ]]; then
            tap_ok "run -o writes JSONL with Command first line (no python3, basic check)"
        else
            tap_not_ok "run -o writes JSONL with Command first line" "first line: $first_line"
        fi
    fi
else
    tap_not_ok "run -o creates output file" "file missing or empty"
fi

# ===== Test 12: run with --max-steps 0 (immediate Error) =====
assert_cmd "run --max-steps 0 produces Error fact" \
    "0" \
    "max_steps" \
    "$BIN" run "$FIXTURES_DIR/valid" --max-steps 0

# ===== Test 13: replay a fact log =====
assert_cmd "replay exits 0 with Replaying header" \
    "0" \
    "Replaying" \
    "$BIN" replay "$OUT1"

# ===== Test 14: replay nonexistent (exit 1) =====
assert_cmd "replay nonexistent exits 1" \
    "1" \
    "I/O error" \
    "$BIN" replay "$WORK_DIR/nonexistent.log"

# ===== Test 15: diff identical =====
cp "$OUT1" "$WORK_DIR/fact2.log"
assert_cmd "diff identical logs" \
    "0" \
    "identical" \
    "$BIN" diff "$OUT1" "$WORK_DIR/fact2.log"

# ===== Test 16: diff different logs (echo fixture) =====
cat > "$WORK_DIR/echo_payload1.json" <<'JSON'
{"input": "hello"}
JSON
cat > "$WORK_DIR/echo_payload2.json" <<'JSON'
{"input": "world"}
JSON
"$BIN" run "$FIXTURES_DIR/echo" --payload-file "$WORK_DIR/echo_payload1.json" -o "$WORK_DIR/echo1.log" >/dev/null 2>&1
"$BIN" run "$FIXTURES_DIR/echo" --payload-file "$WORK_DIR/echo_payload2.json" -o "$WORK_DIR/echo2.log" >/dev/null 2>&1
assert_cmd "diff different logs shows differences" \
    "0" \
    "difference" \
    "$BIN" diff "$WORK_DIR/echo1.log" "$WORK_DIR/echo2.log"

# ===== Test 17: verify-chain valid =====
assert_cmd "verify-chain valid log exits 0" \
    "0" \
    "verified" \
    "$BIN" verify-chain "$OUT1"

# ===== Test 18: verify-chain tampered (structural invariant violation) =====
# Tamper: change id 2 to 99 (breaks FactId monotonicity)
sed 's/"id":2,/"id":99,/' "$OUT1" > "$WORK_DIR/tampered.log"
assert_cmd "verify-chain tampered log exits 1" \
    "1" \
    "monotonicity" \
    "$BIN" verify-chain "$WORK_DIR/tampered.log"

# ===== Test 19: JSONL integrity check =====
# Each line in a fact log should be valid JSON with "type" field
line_count=$(wc -l < "$OUT1")
all_valid=true
bad_line=""
for ((i=1; i<=line_count; i++)); do
    line=$(sed -n "${i}p" "$OUT1")
    if [[ -n "$line" ]]; then
        if command -v python3 >/dev/null 2>&1; then
            if ! echo "$line" | python3 -c "import json,sys; d=json.loads(sys.stdin.read()); assert 'type' in d and 'id' in d" 2>/dev/null; then
                all_valid=false
                bad_line="line $i: $line"
                break
            fi
        else
            if [[ "$line" != *'"type"'* ]] || [[ "$line" != *'"id"'* ]]; then
                all_valid=false
                bad_line="line $i: $line"
                break
            fi
        fi
    fi
done
if [[ "$all_valid" == "true" ]]; then
    tap_ok "all $line_count fact log lines are valid JSON with type+id"
else
    tap_not_ok "fact log JSONL integrity" "$bad_line"
fi

# ===== Test 20: FIFO queue order (push [step1,step2] → step1 first) =====
"$BIN" run "$FIXTURES_DIR/fifo" -o "$WORK_DIR/fifo.log" >/dev/null 2>&1
# With FIFO: step1 executes first, then step2. Final order="second" (last set wins).
# With LIFO: step2 executes first, then step1. Final order="first" (last set wins).
fifo_last=$(tail -1 "$WORK_DIR/fifo.log")
if [[ "$fifo_last" == *'"order":"second"'* ]]; then
    tap_ok "FIFO queue: step1 then step2 (order=second confirms FIFO)"
else
    tap_not_ok "FIFO queue: step1 then step2" "expected order=second, got: $fifo_last"
fi

# ===== Test 21: deterministic loading (multi-file sorted by filename) =====
"$BIN" run "$FIXTURES_DIR/multi" -o "$WORK_DIR/multi1.log" >/dev/null 2>&1
"$BIN" run "$FIXTURES_DIR/multi" -o "$WORK_DIR/multi2.log" >/dev/null 2>&1
if diff -q "$WORK_DIR/multi1.log" "$WORK_DIR/multi2.log" >/dev/null 2>&1; then
    # Verify all 3 fields set: a=1, b=2, c=3
    multi_content=$(cat "$WORK_DIR/multi1.log")
    if [[ "$multi_content" == *'"a":1'* ]] && [[ "$multi_content" == *'"b":2'* ]] && [[ "$multi_content" == *'"c":3'* ]]; then
        tap_ok "deterministic loading: 3 files loaded in order, all fields set"
    else
        tap_not_ok "deterministic loading" "missing fields in output: $multi_content"
    fi
else
    tap_not_ok "deterministic loading" "two runs produced different output"
fi

# ===== Test 22: hash chain cross-validation (verify-chain on echo log) =====
assert_cmd "verify-chain on echo log exits 0" \
    "0" \
    "verified" \
    "$BIN" verify-chain "$WORK_DIR/echo1.log"

# ===== Test 23: hospital example rules validate =====
assert_cmd "validate hospital example rules" \
    "0" \
    "[OK]" \
    "$BIN" validate "$SCRIPT_DIR/../examples/hospital/rules"

# ===== Test 24: law-firm example rules validate =====
assert_cmd "validate law-firm example rules" \
    "0" \
    "[OK]" \
    "$BIN" validate "$SCRIPT_DIR/../examples/law-firm/rules"

# ===== Test 25: hospital example runs with payload =====
assert_cmd "hospital example runs with payload (Stable)" \
    "0" \
    "Stable" \
    "$BIN" run "$SCRIPT_DIR/../examples/hospital/rules" --payload-file "$SCRIPT_DIR/../examples/hospital/payload.example.json"

# ===== Test 26: law-firm example runs with payload =====
assert_cmd "law-firm example runs with payload (Stable)" \
    "0" \
    "Stable" \
    "$BIN" run "$SCRIPT_DIR/../examples/law-firm/rules" --payload-file "$SCRIPT_DIR/../examples/law-firm/payload.example.json"

# ===== Test 27: hospital example produces IoRequest (no handler → Error) =====
"$BIN" run "$SCRIPT_DIR/../examples/hospital/rules" \
    --payload-file "$SCRIPT_DIR/../examples/hospital/payload.example.json" \
    -o "$WORK_DIR/hospital.log" >/dev/null 2>&1
hospital_content=$(cat "$WORK_DIR/hospital.log")
if [[ "$hospital_content" == *'"type":"IoRequest"'* ]] && [[ "$hospital_content" == *'"type":"Error"'* ]]; then
    tap_ok "hospital example produces IoRequest + Error (no handler in 0.2.0)"
else
    tap_not_ok "hospital example produces IoRequest + Error" "expected IoRequest and Error facts"
fi

# ===== Test 28: verify-chain on hospital log (with IoRequest + Error) =====
assert_cmd "verify-chain on hospital log (complex facts) exits 0" \
    "0" \
    "verified" \
    "$BIN" verify-chain "$WORK_DIR/hospital.log"

# ===== Summary =====
echo "#"
echo "# tests $TOTAL | passed $PASSED | failed $FAILED"

if [[ $FAILED -gt 0 ]]; then
    echo "# FAILED"
    exit 1
else
    echo "# all tests passed"
    exit 0
fi
