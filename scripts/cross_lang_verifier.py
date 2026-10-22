#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 EvoRule Project
"""G5 跨语言参考实现：evorule 审计链哈希契约的 Python 侧独立重推导。

与 Rust SSOT（evorule-audit-chain）的唯一契约：
  fact_hash   = blake3( stable_json_utf8 )
  chain_step  = blake3( (prev + content_hash).utf8 )
  stable_json = Fact 字段按本文件各 _*_fields() 显式构造的 dict，
                经 json.dumps(sort_keys=True)（键序字母序，与 Rust BTreeMap 序一致；
                None 字段省略与 Rust Option::None 省略一致）

黄金向量：g5_golden_vectors.json（由内核 SSOT 实算导出，11 组：
7 标准事实 + domain_attr 两态 + 中文换行 + i64 边界大整数）。
对拍不通过 = 契约漂移 = 红色（CI 强制，不靠注释宣称兼容——v4 的教训）。
"""

from blake3 import blake3  # pip install blake3（唯一非标依赖）
import json
import sys
from pathlib import Path


def _cmd_fields(stable: dict) -> dict:
    ins = stable["instruction"]
    return {"type": "Command", "id": stable["id"], "instruction": ins}


def _payload_fields(stable: dict) -> dict:
    return {
        "type": "PayloadUpdate",
        "id": stable["id"],
        "path": stable["path"],
        "value": stable["value"],
    }


def _transition_fields(stable: dict) -> dict:
    return {
        "type": "StateTransition",
        "id": stable["id"],
        "cause": stable["cause"],
        "new_payload": stable["new_payload"],
        "new_queue": stable["new_queue"],
    }


def _io_request_fields(stable: dict) -> dict:
    return {
        "type": "IoRequest",
        "id": stable["id"],
        "cause": stable["cause"],
        "io_type": stable["io_type"],
        "params": stable["params"],
    }


def _io_response_fields(stable: dict) -> dict:
    err = stable.get("error")
    return {
        "type": "IoResponse",
        "id": stable["id"],
        "request_id": stable["request_id"],
        "result": stable["result"],
        "error": err if err is not None else None,
    }


def _stable_fields(stable: dict) -> dict:
    return {"type": "Stable", "id": stable["id"], "version": stable["version"]}


def _error_fields(stable: dict) -> dict:
    return {"type": "Error", "id": stable["id"], "message": stable["message"]}


def _trace_fields(stable: dict) -> dict:
    hits = []
    for h in stable["rule_hits"]:
        hit = {"index": h["index"], "instr_type": h["instr_type"], "hit": h["hit"]}
        if "domain_attr" in h:  # None 省略（与 Rust Option::None 一致）
            hit["domain_attr"] = h["domain_attr"]
        hits.append(hit)
    return {
        "type": "TransitionTrace",
        "id": stable["id"],
        "cause": stable["cause"],
        "rule_hits": hits,
    }


def _violation_fields(stable: dict) -> dict:
    return {
        "type": "Violation",
        "id": stable["id"],
        "cause": stable["cause"],
        "rule_index": stable["rule_index"],
        "reason": stable["reason"],
        "instruction": stable["instruction"],
    }


_FIELD_BUILDERS = {
    "Command": _cmd_fields,
    "PayloadUpdate": _payload_fields,
    "StateTransition": _transition_fields,
    "IoRequest": _io_request_fields,
    "IoResponse": _io_response_fields,
    "Stable": _stable_fields,
    "Error": _error_fields,
    "TransitionTrace": _trace_fields,
    "Violation": _violation_fields,
}


def fact_hash(stable_json: dict) -> str:
    """stable_json（Rust fact_to_stable_json 的 JSON 对象）→ 64-hex fact_hash。"""
    kind = stable_json["type"]
    canonical = json.dumps(
        _FIELD_BUILDERS[kind](stable_json),
        sort_keys=True,
        ensure_ascii=False,
        separators=(",", ":"),
    )
    return blake3(canonical.encode("utf-8")).hexdigest()


def chain_step(prev_hash: str, content_hash: str) -> str:
    """blake3((prev + content).utf8)——与 Rust chain_step 同公式。"""
    return blake3((prev_hash + content_hash).encode("utf-8")).hexdigest()


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(f"usage: {argv[0]} <g5_golden_vectors.json>", file=sys.stderr)
        return 2
    vectors = json.loads(Path(argv[1]).read_text(encoding="utf-8"))
    failures = 0
    for v in vectors:
        got = fact_hash(v["stable_json"])
        want = v["fact_hash"]
        ok = got == want
        if not ok:
            failures += 1
        print(
            f"  [{'PASS' if ok else 'FAIL'}] {v['name']}: "
            f"{got[:16]}… (rust {want[:16]}…)"
        )
        # 链步对拍（从 genesis 推进一格）
        got_chain = chain_step("genesis", got)
        want_chain = v["chain_from_genesis"]
        if got_chain != want_chain:
            failures += 1
            print(f"  [FAIL] {v['name']} chain_step: {got_chain[:16]}… (rust {want_chain[:16]}…)")
    print(f"\n{'PASS' if failures == 0 else 'FAIL'}: {len(vectors)} 组向量 / {failures} 失配")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
