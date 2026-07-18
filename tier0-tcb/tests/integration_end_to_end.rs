//! tier0-tcb v6.0.0 -- End-to-End Integration Test
//!
//! 自动化 version of `examples/end_to_end.rs`. 加载真 `core_eval.json`
//! (宪法) -> 解析为 `JsonValue` -> 通过 `execute_transition()` 跑业务指令
//! -> 验证状态正确改变。所有断言在 `cargo test` 中跑。
//!
//! ## 与 example 的区别
//!
//! - 4 个 sub-test 拆为独立 `#[test]` 函数（不再是顺序执行 + println）
//! - 无 `main()`，无退出码
//! - 解析失败的错误用 `expect()` 报告，由 test runner 显示
//!
//! ## 不验证什么
//!
//! - **不**验证 tier1-reactor（本仓库未实现）
//! - **不**验证 tier2-governance（审计 / HTTP API 等）
//! - **不**验证 Kani 形式化验证（N-01, 需 kani 工具链）

use tier0_tcb::{execute_transition, JsonValue, TransitionResult};

use std::collections::BTreeMap;
const CORE_EVAL_JSON: &str = include_str!("../core_eval.json");

// =============================================================================
// Minimal JSON parser (subset: object, array, string, int, bool, null)
// =============================================================================
//
// tier0-tcb 本身是 no_std + 零依赖, 不带 JSON parser.
// core_eval.json 是"编译期输入", 生产路径是 build.rs / serde 编译.
// 本 demo 自带递归下降 parser, 只为验证"加载 JSON 文件 -> 喂给 TCB"路径可行.
// 如生产使用, 请替换为 serde_json 或类似.

#[derive(Debug)]
enum ParseError {
    UnexpectedChar(char, usize),
    UnexpectedEof,
    InvalidNumber(String),
    UnterminatedString,
    InvalidEscape(char, usize),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnexpectedChar(c, pos) => {
                write!(f, "unexpected character {c:?} at byte offset {pos}")
            }
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseError::InvalidNumber(s) => write!(f, "invalid number: {s:?}"),
            ParseError::UnterminatedString => write!(f, "unterminated string"),
            ParseError::InvalidEscape(c, pos) => {
                write!(f, "invalid escape sequence {c:?} at byte offset {pos}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

fn parse(s: &str) -> Result<JsonValue, ParseError> {
    let mut p = Parser { s, pos: 0 };
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos < p.s.len() {
        return Err(ParseError::UnexpectedChar(
            p.s.as_bytes()[p.pos] as char,
            p.pos,
        ));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(ParseError::UnexpectedChar(
                self.peek().map_or('?', |b| b as char),
                self.pos,
            ))
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, ParseError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(JsonValue::String),
            Some(b't' | b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(c) => Err(ParseError::UnexpectedChar(c as char, self.pos)),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    fn parse_object(&mut self) -> Result<JsonValue, ParseError> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::object(map));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonValue::object(map));
                }
                Some(c) => return Err(ParseError::UnexpectedChar(c as char, self.pos)),
                None => return Err(ParseError::UnexpectedEof),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, ParseError> {
        self.expect(b'[')?;
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::array(arr));
        }
        loop {
            let v = self.parse_value()?;
            arr.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonValue::array(arr));
                }
                Some(c) => return Err(ParseError::UnexpectedChar(c as char, self.pos)),
                None => return Err(ParseError::UnexpectedEof),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(s);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"') => {
                            s.push('"');
                            self.pos += 1;
                        }
                        Some(b'\\') => {
                            s.push('\\');
                            self.pos += 1;
                        }
                        Some(b'/') => {
                            s.push('/');
                            self.pos += 1;
                        }
                        Some(b'n') => {
                            s.push('\n');
                            self.pos += 1;
                        }
                        Some(b't') => {
                            s.push('\t');
                            self.pos += 1;
                        }
                        Some(b'r') => {
                            s.push('\r');
                            self.pos += 1;
                        }
                        Some(c) => return Err(ParseError::InvalidEscape(c as char, self.pos)),
                        None => return Err(ParseError::UnexpectedEof),
                    }
                }
                Some(_) => {
                    let rest = &self.s[self.pos..];
                    let ch = rest.chars().next().ok_or(ParseError::UnexpectedEof)?;
                    s.push(ch);
                    self.pos += ch.len_utf8();
                }
                None => return Err(ParseError::UnterminatedString),
            }
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, ParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(ParseError::InvalidNumber(
                self.s[start..self.pos].to_string(),
            ));
        }
        let n: i64 = self.s[start..self.pos]
            .parse()
            .map_err(|_| ParseError::InvalidNumber(self.s[start..self.pos].to_string()))?;
        Ok(JsonValue::integer(n))
    }

    fn parse_bool(&mut self) -> Result<JsonValue, ParseError> {
        if self.s[self.pos..].starts_with("true") {
            self.pos += 4;
            Ok(JsonValue::bool(true))
        } else if self.s[self.pos..].starts_with("false") {
            self.pos += 5;
            Ok(JsonValue::bool(false))
        } else {
            Err(ParseError::UnexpectedChar(
                self.s.as_bytes()[self.pos] as char,
                self.pos,
            ))
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, ParseError> {
        if self.s[self.pos..].starts_with("null") {
            self.pos += 4;
            Ok(JsonValue::null())
        } else {
            Err(ParseError::UnexpectedChar(
                self.s.as_bytes()[self.pos] as char,
                self.pos,
            ))
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn as_str(v: &JsonValue) -> &str {
    v.as_str()
        .unwrap_or_else(|| panic!("expected String, got {v:?}"))
}

fn as_int(v: &JsonValue) -> i64 {
    v.as_i64()
        .unwrap_or_else(|| panic!("expected Integer, got {v:?}"))
}

fn obj_get<'a>(v: &'a JsonValue, key: &str) -> &'a JsonValue {
    v.get(key).unwrap_or_else(|| panic!("missing key: {key}"))
}

// =============================================================================
// Test scenarios
// =============================================================================

/// Scenario 1: `increment` business instruction
///
/// 验证: TCB 找到 domain=increment 的 branch, 取 `on_true` 里的 set(add) 元指令,
/// 路径 __exec__.instruction.params.delta 解析为实际参数 (5),
/// 路径 __exec__.instruction.params.attr 解析为字段名 ("x"),
/// 最终 payload.x = 10 + 5 = 15.
fn test_increment(core_eval: &[JsonValue]) {
    println!("\n[1] test_increment: payload.x: 10 + delta=5 -> expect 15");

    let instruction = JsonValue::object_from_pairs(&[
        ("type", "increment".into()),
        (
            "params",
            JsonValue::object_from_pairs(&[("attr", "x".into()), ("delta", 5.into())]),
        ),
    ]);
    let payload = JsonValue::object_from_pairs(&[("x", 10.into())]);
    let queue: Vec<JsonValue> = vec![];

    let result = execute_transition(core_eval, &instruction, &payload, &queue)
        .expect("execute_transition should not error for valid input");

    match result {
        TransitionResult::State {
            new_payload,
            new_queue,
        } => {
            let x = as_int(obj_get(&new_payload, "x"));
            assert_eq!(x, 15, "increment: expected x=15, got x={x}");
            assert!(new_queue.is_empty(), "increment should not push anything");
            println!("    PASS  payload.x = {x}");
        }
        TransitionResult::IoRequired { io_type, .. } => {
            panic!("increment should NOT trigger I/O, got IoRequired({io_type})");
        }
    }
}

/// Scenario 2: `while_loop` business instruction (recursive via push)
///
/// 验证: `while_loop` 的递归 push 是否真的循环:
///   初始: queue=[`while_instr`], payload={x:0}
///   每轮: pop `while_instr`, condition 0<x<3, push [body, `while_instr`]
///         -> body 跑 (increment x), 再 pop `while_instr`
///   终止: condition 3<3=false, push noop, noop 让 queue=[]
///
/// 模拟 tier1-reactor 缺失时的最小反应器 (本函数即 tier1 占位).
fn test_while_loop(core_eval: &[JsonValue]) {
    println!("\n[2] test_while_loop: 累加 x 直到 x>=3, 起始 x=0");
    println!("    (验证 while_loop 通过 push(self) 实现递归 -- 纯 JSON 规则, 无 TCB 循环原语)");

    let body = JsonValue::object_from_pairs(&[
        ("type", "increment".into()),
        (
            "params",
            JsonValue::object_from_pairs(&[("attr", "x".into()), ("delta", 1.into())]),
        ),
    ]);
    // 注意: lt 域的字段是 {path, value} (顶层), 不是 {params: {attr, value}}
    // path 在 evaluate_domain 内被 resolve_path 解析, 所以路径是 __exec__.payload.x
    let condition = JsonValue::object_from_pairs(&[
        ("type", "lt".into()),
        ("path", "__exec__.payload.x".into()),
        ("value", 3.into()),
    ]);
    let while_instr = JsonValue::object_from_pairs(&[
        ("type", "while_loop".into()),
        (
            "params",
            JsonValue::object_from_pairs(&[("condition", condition), ("body", body)]),
        ),
    ]);

    let mut payload = JsonValue::object_from_pairs(&[("x", 0.into())]);
    let mut queue: Vec<JsonValue> = vec![while_instr];
    let mut iter = 0;

    while !queue.is_empty() {
        iter += 1;
        assert!(
            iter <= 50,
            "while_loop did not terminate after {iter} iterations"
        );

        let instr = &queue[0];
        let queue_arg: Vec<JsonValue> = queue[1..].to_vec();

        match execute_transition(core_eval, instr, &payload, &queue_arg)
            .expect("execute_transition error")
        {
            TransitionResult::State {
                new_payload,
                new_queue,
            } => {
                payload = new_payload;
                queue = new_queue;
            }
            TransitionResult::IoRequired { io_type, .. } => {
                panic!("while_loop should NOT trigger I/O, got {io_type}");
            }
        }
    }

    let x = as_int(obj_get(&payload, "x"));
    assert_eq!(x, 3, "while_loop: expected x=3, got x={x}");
    println!("    PASS  looped {iter} times, final payload.x = {x}");
}

/// Scenario 3: `call_external` triggers `io_request` signal
///
/// 验证: `call_external` 不修改 payload, 而是返回 `IoRequired` { `io_type`: "`call_external`", params: {...} }.
/// 反应器 (尚未实现) 会收到此信号 -> 调用外部服务 -> 注入 __`io_result`__ -> 再次调用 TCB.
fn test_call_external_io_request(core_eval: &[JsonValue]) {
    println!("\n[3] test_call_external_io_request: call_external 应产生 IoRequired, payload 不变");

    let instruction = JsonValue::object_from_pairs(&[
        ("type", "call_external".into()),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("prompt", "Hello world".into()),
                ("temperature", 7.into()),
            ]),
        ),
    ]);
    let payload = JsonValue::object_from_pairs(&[("x", 99.into())]);
    let queue: Vec<JsonValue> = vec![];

    let result = execute_transition(core_eval, &instruction, &payload, &queue)
        .expect("execute_transition should not error for valid input");

    match result {
        TransitionResult::IoRequired { io_type, params } => {
            assert_eq!(
                io_type, "call_external",
                "io_type should be 'call_external'"
            );
            let p = obj_get(&params, "prompt");
            assert_eq!(as_str(p), "Hello world", "prompt path resolution failed");
            let temp = obj_get(&params, "temperature");
            assert_eq!(as_int(temp), 7, "temperature path resolution failed");
            println!(
                "    PASS  IoRequired {{ io_type: \"{}\", prompt: {:?}, temperature: {} }}",
                io_type,
                as_str(p),
                as_int(temp),
            );
        }
        TransitionResult::State { new_payload, .. } => {
            panic!(
                "call_external should trigger I/O, NOT state change. Got payload={new_payload:?}"
            );
        }
    }
}

/// Bonus: catch-all `all([])` 兜底规则
///
/// 验证: 未识别的指令走最后一条 all([]) 分支, `on_true`=[],
/// 返回 State { payload 不变, queue 不变 }.
fn test_catch_all_noop(core_eval: &[JsonValue]) {
    println!("\n[4] test_catch_all_noop: 未知指令 'frobnicate' -> catch-all noop");

    let instruction = JsonValue::object_from_pairs(&[("type", "frobnicate".into())]);
    let payload = JsonValue::object_from_pairs(&[("x", 42.into())]);
    let queue: Vec<JsonValue> = vec![];

    let result = execute_transition(core_eval, &instruction, &payload, &queue)
        .expect("execute_transition should not error");

    match result {
        TransitionResult::State {
            new_payload,
            new_queue,
        } => {
            let x = as_int(obj_get(&new_payload, "x"));
            assert_eq!(x, 42, "catch-all should not modify payload, got x={x}");
            assert!(new_queue.is_empty(), "catch-all should not push anything");
            println!("    PASS  payload.x = {x} (unchanged), queue empty");
        }
        TransitionResult::IoRequired { io_type, .. } => {
            panic!("catch-all should not trigger I/O, got {io_type}");
        }
    }
}

// =============================================================================
// =============================================================================
// Test harness: load core_eval.json + 4 #[test] functions
// =============================================================================

/// 加载并解析 `core_eval.json`，返回 `transform` 数组的 owned Vec。
/// 解析失败 / transform 字段缺失 / 类型不对都 panic（让 test runner 报告）。
fn load_transform() -> Vec<JsonValue> {
    let doc = parse(CORE_EVAL_JSON).expect("core_eval.json parse error");
    let arr = doc
        .get("transform")
        .and_then(|v| v.as_array())
        .expect("`transform` field is not an array");
    arr.to_vec()
}

#[test]
fn e2e_increment_with_real_core_eval() {
    test_increment(&load_transform());
}

#[test]
fn e2e_while_loop_with_real_core_eval() {
    test_while_loop(&load_transform());
}

#[test]
fn e2e_call_external_io_request_with_real_core_eval() {
    test_call_external_io_request(&load_transform());
}

#[test]
fn e2e_catch_all_noop_with_real_core_eval() {
    test_catch_all_noop(&load_transform());
}
