#!/bin/bash
# e2e.sh - End-to-end test for evorule CLI
#
# Tests all 4 subcommands (validate / run / replay / diff) with realistic
# rule fixtures. Output in TAP format for CI integration.
#
# Usage:
#   bash tests/e2e.sh                                # auto-detect binary
#   bash tests/e2e.sh /path/to/evorule               # explicit binary path
#   bash tests/e2e.sh /path/to/evorule x86_64-musl   # with arch tag for logging
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
    # Auto-detect: check common locations
    for candidate in \
        "/mnt/d/evorule/.build/rust/x86_64-unknown-linux-musl/release/evorule" \
        "/mnt/d/evorule/.build/rust/aarch64-unknown-linux-musl/release/evorule" \
        "/mnt/d/evorule/.build/rust/release/evorule.exe" \
        "/mnt/d/evorule/.build/rust/release/evorule" \
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
    echo "Searched: $BIN" >&2
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
echo "# evorule e2e test"
echo "# binary: $BIN"
echo "#"

assert_cmd "--version exits 0" \
    "0" \
    "evorule" \
    "$BIN" --version

# ===== Test 2: validate valid rule =====
assert_cmd "validate valid rule exits 0" \
    "0" \
    "[PASS]" \
    "$BIN" validate "$FIXTURES_DIR/valid"

# ===== Test 3: validate invalid rule (missing type) =====
assert_cmd "validate invalid rule exits 1" \
    "1" \
    "[ERROR]" \
    "$BIN" validate "$FIXTURES_DIR/invalid"

# ===== Test 4: validate unknown type (WARN, not ERROR) =====
assert_cmd "validate unknown-type warns but passes" \
    "0" \
    "[WARN]" \
    "$BIN" validate "$FIXTURES_DIR/unknown-type"

# ===== Test 5: validate empty directory =====
assert_cmd "validate empty dir exits 1" \
    "1" \
    "No .json files" \
    "$BIN" validate "$FIXTURES_DIR/empty"

# ===== Test 6: validate nonexistent directory =====
assert_cmd "validate nonexistent dir exits 1" \
    "1" \
    "does not exist" \
    "$BIN" validate "$WORK_DIR/nonexistent"

# ===== Test 7: run valid rule (stdout) =====
assert_cmd "run valid rule exits 0" \
    "0" \
    "final" \
    "$BIN" run "$FIXTURES_DIR/valid"

# ===== Test 8: run with --payload =====
assert_cmd "run with --payload exits 0" \
    "0" \
    "final" \
    "$BIN" run "$FIXTURES_DIR/valid" --payload '{"x": 0}'

# ===== Test 9: run with --payload-file =====
cat > "$WORK_DIR/payload.json" <<'JSON'
{"x": 100, "y": "hello"}
JSON
assert_cmd "run with --payload-file exits 0" \
    "0" \
    "final" \
    "$BIN" run "$FIXTURES_DIR/valid" --payload-file "$WORK_DIR/payload.json"

# ===== Test 10: run with -o output file =====
OUT1="$WORK_DIR/fact1.log"
"$BIN" run "$FIXTURES_DIR/valid" -o "$OUT1" >/dev/null 2>&1
if [[ -f "$OUT1" ]] && [[ -s "$OUT1" ]]; then
    # Check first line is valid JSON
    first_line=$(head -1 "$OUT1")
    if command -v python3 >/dev/null 2>&1; then
        if python3 -c "import json,sys; json.loads(sys.stdin.read().strip().split(chr(10))[0])" < "$OUT1" 2>/dev/null; then
            tap_ok "run -o writes valid JSONL"
        else
            tap_not_ok "run -o writes valid JSONL" "first line not valid JSON: $first_line"
        fi
    else
        if [[ -n "$first_line" ]] && [[ "$first_line" == "{"* ]]; then
            tap_ok "run -o writes JSONL (python3 not available, basic check passed)"
        else
            tap_not_ok "run -o writes JSONL" "first line: $first_line"
        fi
    fi
else
    tap_not_ok "run -o creates output file" "file missing or empty"
fi

# ===== Test 11: replay a fact log =====
assert_cmd "replay exits 0" \
    "0" \
    "Replaying" \
    "$BIN" replay "$OUT1"

# ===== Test 12: replay nonexistent =====
assert_cmd "replay nonexistent exits 1" \
    "1" \
    "Failed to read" \
    "$BIN" replay "$WORK_DIR/nonexistent.log"

# ===== Test 13: diff identical =====
"$BIN" run "$FIXTURES_DIR/valid" -o "$OUT1" >/dev/null 2>&1
cp "$OUT1" "$WORK_DIR/fact2.log"
assert_cmd "diff identical logs" \
    "0" \
    "identical" \
    "$BIN" diff "$OUT1" "$WORK_DIR/fact2.log"

# ===== Test 14: diff different logs =====
assert_cmd "diff different logs" \
    "0" \
    "Only in" \
    "$BIN" diff "$OUT1" "$WORK_DIR/payload.json"

# ===== Test 15: JSON Lines integrity check =====
# Each line in a fact log should be valid JSON
line_count=$(wc -l < "$OUT1")
all_valid=true
for ((i=1; i<=line_count; i++)); do
    line=$(sed -n "${i}p" "$OUT1")
    if [[ -n "$line" ]]; then
        if command -v python3 >/dev/null 2>&1; then
            if ! echo "$line" | python3 -c "import json,sys; json.loads(sys.stdin.read())" 2>/dev/null; then
                all_valid=false
                break
            fi
        else
            if [[ "$line" != "{"* ]]; then
                all_valid=false
                break
            fi
        fi
    fi
done
if [[ "$all_valid" == "true" ]]; then
    tap_ok "all $line_count fact log lines are valid JSON"
else
    tap_not_ok "fact log JSON Lines integrity" "line $i invalid: $line"
fi

# ===== Test 16: hospital example rules validate =====
assert_cmd "validate hospital example rules" \
    "0" \
    "[PASS]" \
    "$BIN" validate "$SCRIPT_DIR/../examples/hospital/rules"

# ===== Test 17: law-firm example rules validate =====
assert_cmd "validate law-firm example rules" \
    "0" \
    "[PASS]" \
    "$BIN" validate "$SCRIPT_DIR/../examples/law-firm/rules"

# ===== Test 18: hospital example runs with payload =====
assert_cmd "hospital example runs with payload" \
    "0" \
    "final" \
    "$BIN" run "$SCRIPT_DIR/../examples/hospital/rules" --payload-file "$SCRIPT_DIR/../examples/hospital/payload.example.json"

# ===== Test 19: law-firm example runs with payload =====
assert_cmd "law-firm example runs with payload" \
    "0" \
    "final" \
    "$BIN" run "$SCRIPT_DIR/../examples/law-firm/rules" --payload-file "$SCRIPT_DIR/../examples/law-firm/payload.example.json"

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
