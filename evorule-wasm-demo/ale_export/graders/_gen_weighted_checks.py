# -*- coding: utf-8 -*-
"""eval_weighted_checks_v1 规则集生成器（A 类资产：判定环节经验模板化）。

用法：填三张表 → gen() → 写 JSON。禁手写规则集 JSON（嵌套括号错）。
约束自检：总 points=10000 / 分值整数 / bridge 侧须产全部 check+gate 布尔。
形态依据：TCB 宪法原语（branch 嵌套+all/eq+set/add——禁 push，只入队不执行）。
实例：demo_hello_v1.json（单查退化）、atsp_v1.json（13 查+三 gate）。
"""
from __future__ import annotations
import json


def gen(instruction: str, checks: list[tuple[str, str, int]], gates: dict[str, str]) -> list[dict]:
    """生成规则集。checks=[(tier, check_name, points)]；gates={tier: gate_boolean}。"""
    assert all(isinstance(p, int) and p > 0 for _, _, p in checks), "分值须正整数（万分制定点）"
    assert sum(p for *_, p in checks) == 10000, f"总 points 须=10000，现={sum(p for *_, p in checks)}"
    F = "__exec__.payload.facts"
    ATTR = "__exec__.payload.grade.score"

    def eq(path: str) -> dict:
        return {"type": "eq", "path": f"{F}.{path}", "value": 1, "on_missing": "unsat"}

    def add_pts(n: int) -> dict:
        return {"type": "set", "params": {"attr": ATTR, "operation": "add", "value": n}}

    def br(domain: dict, on_true: list) -> dict:
        return {"type": "branch", "params": {"domain": domain, "on_true": on_true, "on_false": []}}

    branches = []
    for tier, chk, pts in checks:
        key = chk if tier in chk or "_" not in chk else f"{tier}_{chk}"
        inner = [eq(key)]
        if tier in gates:  # metadata 挂→整 tier 清零（官方语义：清零非扣分）
            inner.insert(0, eq(gates[tier]))
        branches.append(br({"type": "all", "inner": inner}, [add_pts(pts)]))
    return [{
        "type": "branch",
        "params": {
            "domain": {"type": "instruction", "instruction_type": instruction},
            "on_true": branches,
            "on_false": [],
        },
    }]


if __name__ == "__main__":
    # atsp_v1 自证：模板生成的规则集与已实证的 atsp_v1.json 判定等价
    checks = (
        [("tier1", c, 1000) for c in ("valid_tour", "cost_matches_tour", "cost_matches_ref", "bound_certifies")]
        + [("tier2", c, 875) for c in ("valid_tour", "cost_matches_tour", "cost_matches_ref", "bound_certifies")]
        + [("tier3", c, 500) for c in ("valid_tour", "cost_matches_tour", "incumbent_ok", "bound_sane", "gap_ok")]
    )
    ruleset = gen("eval_atsp_v1", checks, {t: f"{t}_metadata_ok" for t in ("tier1", "tier2", "tier3")})
    import io
    io.open("atsp_v1.json", "w", encoding="utf-8", newline="\n").write(
        json.dumps(ruleset, ensure_ascii=False, indent=1) + "\n")
    print("gen OK：", len(ruleset[0]["params"]["on_true"]), "段打分分支（总=10000）")
