"""J-A1 等价性判据：十变体对拍——官方 Python grader 语义 vs WASM 判卷。

官方语义（demo/hello main.py evaluate）:
    expected = (await session.read_file(ref_path)).strip()
    actual   = (await session.read_file(out_path)).strip()  # 读不到 → [0.0]
    return [1.0 if actual == expected else 0.0]

跑法: python ja1_equivariance.py   → 十行 PASS/FAIL + 总结论
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).parent
PKG = HERE / "engine" / "pkg" / "evorule_wasm_demo.js"
RULESET = HERE / "graders" / "demo_hello_v1.json"

EXPECT = "CONF-A7X-1234"  # 假设参考值


def official(ref: str | None, out: str | None) -> list[float]:
    """官方 grader 语义的忠实复刻（同一行代码结构）。"""
    if ref is None:
        raise RuntimeError("reference unreadable")
    expected = ref.strip()
    if out is None:
        return [0.0]  # 官方：读不到产物 → [0.0]
    actual = out.strip()
    return [1.0 if actual == expected else 0.0]


def wasm(ref: str | None, out: str | None) -> list[float]:
    """WASM 判卷链路：strip（bridge 层）→ facts → grade_wasm。"""
    expected = ref.strip() if ref is not None else None
    facts: dict = {"expected": expected}
    if out is not None:
        facts["actual"] = out.strip()
    script = (
        "const e = require(process.argv[1]);\n"
        "const out = e.grade_wasm(process.argv[2], process.argv[3], process.argv[4]);\n"
        "process.stdout.write(out);\n"
    )
    proc = subprocess.run(
        ["node", "-e", script, str(PKG),
         RULESET.read_text(encoding="utf-8"),
         json.dumps({"type": "eval_exact_match", "params": {"task_id": "demo/hello_wasm"}}),
         json.dumps(facts)],
        capture_output=True, text=True, timeout=30, check=True,
    )
    return [float(json.loads(proc.stdout)["score"])]


CASES = [
    ("1 正确",            EXPECT, EXPECT,                    None),
    ("2 错误值",          EXPECT, "CONF-9999-XXXX",           None),
    ("3 缺产物",          EXPECT, None,                       None),  # out 文件不存在
    ("4 空文件",          EXPECT, "",                         None),
    ("5 前空白+正确",     EXPECT, "  \t" + EXPECT,            None),
    ("6 后空白+正确",     EXPECT, EXPECT + "\n  ",            None),
    ("7 大小写不同",      EXPECT, EXPECT.lower(),             None),
    ("8 尾随换行",        EXPECT, EXPECT + "\n",              None),
    ("9 Unicode 同形",    EXPECT, "CONF-А7X-1234",            None),  # А=西里尔
    ("10 空对空",         "",      "",                        None),
]


def main() -> int:
    print(f"{'变体':<14} {'官方':>5} {'WASM':>5} 判定")
    fails = 0
    for name, ref, out, _ in CASES:
        o = official(ref, out)
        w = wasm(ref, out)
        ok = o == w
        fails += 0 if ok else 1
        print(f"{name:<14} {o[0]:>5.1f} {w[0]:>5.1f} {'PASS' if ok else 'FAIL'}")
    print(f"\n结论: {10 - fails}/10 一致 —— {'J-A1 通过' if fails == 0 else 'J-A1 未过'}")
    return 0 if fails == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
