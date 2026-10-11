# -*- coding: utf-8 -*-
"""atsp_v1 判卷规则集（check-boolean 形态）——branch_bound_atsp 官方判据映射。

分层（与 DETERMINISM.md 宣称边界一致）：
- bridge（Python/numpy，官方代码原样）：tour 结构合法性、numpy 重算成本、
  浮点 close()（含 |a-b|<=tol 等号边界）、metadata 非负——产出原子布尔。
- wasm（本规则集）：逐查组合 + 权重 + 分档门控——确定性判定 SSOT。

官方分值（score_outputs.py）：tier1/2 各 4 查 ×0.25、tier3 5 查 ×0.20，
权重 0.40/0.35/0.25 → 万分制：t1 1000/查、t2 875/查、t3 500/查。
tier3 另有 gap 达标查（gap_valid 已含 <=0.005+1e-9）。
"""
import json, io

def eq(p):  # 布尔查：facts.<check> == 1
    return {"type": "eq", "path": f"__exec__.payload.facts.{p}", "value": 1, "on_missing": "unsat"}

def add_pts(n):
    return {"type": "set", "params": {"attr": "__exec__.payload.grade.score", "operation": "add", "value": n}}

def br(dom, pts):
    return {"type": "branch", "params": {"domain": dom, "on_true": [add_pts(pts)], "on_false": []}}

def tier_block(tier, per_check):
    # 官方顺序：valid_tour → cost_matches_tour → cost_matches_ref → bound_certifies（各 0.25）
    # metadata 无效 → 整 tier 归 0（门控：metadata 挂则不加分——由 bridge 布尔 metadata_gate）
    inner = []
    for chk in ("valid_tour", "cost_matches_tour", "cost_matches_ref", "bound_certifies"):
        inner.append(br({"type": "all", "inner": [
            eq(f"{tier}_metadata_ok"), eq(f"{tier}_{chk}"),
        ]}, per_check))
    return inner

def tier3_block():
    out = []
    for chk in ("valid_tour", "cost_matches_tour", "incumbent_ok", "bound_sane", "gap_ok"):
        out.append(br({"type": "all", "inner": [
            eq("tier3_metadata_ok"), eq(f"tier3_{chk}"),
        ]}, 500))
    return out

ruleset = [{
    "type": "branch",
    "params": {
        "domain": {"type": "instruction", "instruction_type": "eval_atsp_v1"},
        "on_true": [
            *tier_block("tier1", 1000),
            *tier_block("tier2", 875),
            *tier3_block(),
        ],
        "on_false": [],
    },
}]

out = json.dumps(ruleset, ensure_ascii=False, indent=1) + "\n"
io.open("atsp_v1.json", "w", encoding="utf-8", newline="\n").write(out)
print("atsp_v1.json check-boolean 版落成：", len(ruleset[0]["params"]["on_true"]), "段打分分支")
