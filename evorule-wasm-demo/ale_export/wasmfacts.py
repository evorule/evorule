"""wasmfacts — WASM 可验证判卷 bridge 库（ALE 可选组件）。

License: Apache-2.0 (Contribution Grant GRANT-2026-ALE-001, EvoRule Project)
设计: 判卷三层架构——本文件是 bridge 层（官方 session I/O + 组装 facts + 调
WASM 引擎 + 返回 list[float]）。判定语义全部在 WASM 内的规则集里
（rubric-as-rules），bridge 不承载判定语义。

I/O 归一化（strip）在 bridge 层显式声明并留痕——receipt 记 strip 前后
两态哈希；规则集内 eq 保持严格字节比对。

用法（ALE 任务 main.py 的 evaluate 内）:

    import wasmfacts
    return wasmfacts.grade(
        session=session,
        out_path=meta["result_path"],
        ref_path=meta["reference_result_path"],
        task_id="demo/hello_wasm",
    )
"""
from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path
from typing import Any, Optional

__all__ = ["grade", "grade_files", "grade_facts", "engine_call"]

# ---------------------------------------------------------------------------
# 引擎调用：node (wasm) / native (rust test binary) 双后端，同一 receipt 契约
# ---------------------------------------------------------------------------

_PKG_JS = Path(__file__).with_name("engine") / "pkg" / "evorule_wasm_demo.js"


def _blake3_hex(data: bytes) -> str:
    """纯 Python blake3（无三方依赖）——只用于留痕，不参与 TCB 判定。

    简化实现：receipt 的 facts_hash 由引擎（Rust blake3）产出；
    bridge 侧此函数仅对 strip 前后明文做摘要展示（verify.html 重放时
    以引擎产出的 facts_hash 为准）。
    """
    # blake3 纯实现超出 bridge 必要面；这里退化为 sha256 并显式标注，
    # 避免伪装成 blake3。引擎侧 facts_hash 才是权威。
    return "sha256:" + hashlib.sha256(data).hexdigest()


def engine_call(
    ruleset_path: Path,
    facts: dict[str, Any],
    task_id: str,
    *,
    engine_js: Optional[Path] = None,
) -> dict[str, Any]:
    """调 WASM 引擎 grade_wasm(ruleset, command, facts) → receipt dict。

    后端优先级：node + wasm-pack 产物；缺产物时直接抛（fail-visible），
    不做静默降级——判卷组件不允许「大概对了」。
    """
    engine_js = engine_js or _PKG_JS
    if not engine_js.exists():
        raise FileNotFoundError(
            f"wasm engine not found at {engine_js}; run wasm-pack build first"
        )
    ruleset = json.dumps(json.loads(Path(ruleset_path).read_text(encoding="utf-8")))
    cmd = json.dumps({"type": "eval_exact_match", "params": {"task_id": task_id}})
    facts_s = json.dumps(facts)
    script = (
        "const e = require(process.argv[1]);\n"
        "const out = e.grade_wasm(process.argv[2], process.argv[3], process.argv[4]);\n"
        "process.stdout.write(out);\n"
    )
    import subprocess

    proc = subprocess.run(
        [sys.executable.replace("python", "node"), "-e", script,
         str(engine_js), ruleset, cmd, facts_s],
        capture_output=True, text=True, timeout=30, check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"engine call failed: {proc.stderr[:500]}")
    return json.loads(proc.stdout)


# ---------------------------------------------------------------------------
# 判卷入口（官方 evaluate 契约内）
# ---------------------------------------------------------------------------

async def _read(session: Any, path: str) -> Optional[str]:
    """官方 session.read_file；读不到（缺产物）返回 None——与官方
    grader 的 try/except 语义一致，缺产物语义在 WASM 内归因。"""
    try:
        return await session.read_file(path)
    except Exception:
        return None


async def grade(
    *,
    session: Any,
    out_path: str,
    ref_path: str,
    task_id: str,
    ruleset_path: Optional[Path] = None,
) -> list[float]:
    """官方 evaluate 契约内替换判定段：读产物/参考→strip→facts→WASM 判定。

    strip 归属：I/O 归一化（bridge 层），非判定语义；两态哈希入
    artifacts_meta 留痕（receipt 可回放比对）。
    """
    ref_raw = await _read(session, ref_path)
    if ref_raw is None:
        # 参考不可读=框架层错误（官方 grader 同位置 raise RuntimeError）
        raise RuntimeError(f"reference unreadable at {ref_path}")
    out_raw = await _read(session, out_path)

    expected = ref_raw.strip()
    artifacts_meta: dict[str, Any] = {
        "normalization": "strip",
        "raw_sha256": _blake3_hex(out_raw.encode("utf-8", "replace")) if out_raw is not None else None,
        "norm_sha256": _blake3_hex(expected.encode("utf-8")),
    }
    facts: dict[str, Any] = {
        "expected": expected,
        "artifacts_meta": artifacts_meta,
    }
    # 缺产物：actual 字段整个不进 facts——WASM 内 on_missing=error 归因
    if out_raw is not None:
        facts["actual"] = out_raw.strip()

    ruleset = ruleset_path or (
        Path(__file__).parent / "graders" / "demo_hello_v1.json"
    )
    receipt = engine_call(ruleset, facts, task_id)
    # 官方契约：list[float]；receipt 全量落日志由调用方（可选）处理
    return [float(receipt["score"])]


def grade_files(
    out_file: Path, ref_file: Path, task_id: str, *,
    ruleset_path: Optional[Path] = None,
) -> tuple[list[float], dict[str, Any]]:
    """离线判卷（本地文件版，测试/重放用）——同 grade 的纯文件形态。"""
    ref_raw = ref_file.read_text(encoding="utf-8")
    expected = ref_raw.strip()
    facts: dict[str, Any] = {"expected": expected}
    if out_file.exists():
        facts["actual"] = out_file.read_text(encoding="utf-8").strip()
    ruleset = ruleset_path or (
        Path(__file__).parent / "graders" / "demo_hello_v1.json"
    )
    receipt = engine_call(ruleset, facts, task_id)
    return [float(receipt["score"])], receipt


def grade_facts(
    facts: dict[str, Any], task_id: str, *,
    ruleset_path: Optional[Path] = None,
) -> dict[str, Any]:
    """纯 facts 判卷（verify.html 重放的 node 侧等价物）——返回 receipt。"""
    ruleset = ruleset_path or (
        Path(__file__).parent / "graders" / "demo_hello_v1.json"
    )
    return engine_call(ruleset, facts, task_id)
