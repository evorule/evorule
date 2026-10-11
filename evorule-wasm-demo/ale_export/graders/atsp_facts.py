# -*- coding: utf-8 -*-
"""branch_bound_atsp bridge —— 官方 scorer 语义原样移植（判据面）。

产出 check-booleans（供 atsp_v1.json 规则集组合）：
  tier{1,2}_{valid_tour,cost_matches_tour,cost_matches_ref,bound_certifies,metadata_ok}
  tier3_{valid_tour,cost_matches_tour,incumbent_ok,bound_sane,gap_ok,metadata_ok}

官方语义来源：tasks/computing_math/branch_bound_atsp/scripts/score_outputs.py
（rdi-berkeley/agents-last-exam）。numpy 重算成本 = 官方 tour_cost() 原样。
"""
from __future__ import annotations
import json
import math
import sys
from pathlib import Path

import numpy as np

SEED = 77777
WIND = (0.37, -0.21, 0.58)
WIND_SCALE = 0.15
COST_TOL = 1e-4
TIER3_GAP_LIMIT = 0.005


def generate_coords(n: int):
    rng = np.random.default_rng(SEED)
    return [tuple(map(float, row)) for row in rng.uniform(0.0, 100.0, size=(n, 3))]


def edge_cost(coords, i, j):
    dx, dy, dz = (coords[j][k] - coords[i][k] for k in range(3))
    norm = math.sqrt(dx * dx + dy * dy + dz * dz)
    if norm == 0.0:
        return math.inf
    wind_dot = (dx / norm) * WIND[0] + (dy / norm) * WIND[1] + (dz / norm) * WIND[2]
    return norm + WIND_SCALE * wind_dot


def tour_cost(tour, n):
    coords = generate_coords(n)
    total = 0.0
    for idx, city in enumerate(tour):
        total += edge_cost(coords, city, tour[(idx + 1) % n])
    return total


def is_valid_tour(value, n) -> bool:
    return (
        isinstance(value, list)
        and len(value) == n
        and value[0] == 0
        and all(isinstance(x, int) and not isinstance(x, bool) for x in value)
        and sorted(value) == list(range(n))
    )


def close(a, b, tol=COST_TOL):
    return a is not None and abs(a - b) <= tol


def _f(v):
    if isinstance(v, bool) or v is None:
        return None
    if isinstance(v, (int, float)):
        return float(v)
    return None


def _nn_int(v):
    return isinstance(v, int) and not isinstance(v, bool) and v >= 0


def _nn_float(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool) and v >= 0


def build_checks(submission: dict, reference: dict) -> dict:
    """官方判据 → 布尔。submission/results.json + reference/results.json。"""
    out = {}
    for tier in ("tier1", "tier2"):
        obs = submission.get(tier) if isinstance(submission.get(tier), dict) else {}
        ref = reference.get(tier, {}) or {}
        n = ref.get("n_cities")
        n_ok = n is not None and obs.get("n_cities") == n
        metadata_ok = n_ok and _nn_int(obs.get("nodes_explored")) and _nn_float(obs.get("runtime_seconds"))
        out[f"{tier}_metadata_ok"] = 1 if metadata_ok else 0
        if not n_ok:
            for chk in ("valid_tour", "cost_matches_tour", "cost_matches_ref", "bound_certifies"):
                out[f"{tier}_{chk}"] = 0
            continue
        tour = obs.get("optimal_tour")
        valid = is_valid_tour(tour, n)
        out[f"{tier}_valid_tour"] = 1 if valid else 0
        recomputed = tour_cost(tour, n) if valid else None
        reported = _f(obs.get("optimal_cost"))
        cost_tour = close(recomputed, reported) if valid else False
        out[f"{tier}_cost_matches_tour"] = 1 if cost_tour else 0
        cost_ref = cost_tour and close(reported, float(ref["optimal_cost"]))
        out[f"{tier}_cost_matches_ref"] = 1 if cost_ref else 0
        bound = _f(obs.get("best_bound"))
        gap = _f(obs.get("optimality_gap"))
        cert = (
            cost_ref
            and close(bound, float(ref["optimal_cost"]))
            and close(gap, 0.0)
            and close(bound, reported if reported is not None else math.inf)
        )
        out[f"{tier}_bound_certifies"] = 1 if cert else 0

    # tier3
    obs = submission.get("tier3") if isinstance(submission.get("tier3"), dict) else {}
    ref = reference.get("tier3", {}) or {}
    n = ref.get("n_cities")
    n_ok = n is not None and obs.get("n_cities") == n
    metadata_ok = n_ok and _nn_int(obs.get("nodes_explored")) and _nn_int(obs.get("lagrangian_iterations")) and _nn_float(obs.get("runtime_seconds"))
    out["tier3_metadata_ok"] = 1 if metadata_ok else 0
    if not n_ok:
        for chk in ("valid_tour", "cost_matches_tour", "incumbent_ok", "bound_sane", "gap_ok"):
            out[f"tier3_{chk}"] = 0
        return out
    tour = obs.get("best_tour")
    valid = is_valid_tour(tour, n)
    out["tier3_valid_tour"] = 1 if valid else 0
    recomputed = tour_cost(tour, n) if valid else None
    reported = _f(obs.get("best_cost"))
    cost_tour = close(recomputed, reported) if valid else False
    out["tier3_cost_matches_tour"] = 1 if cost_tour else 0
    ref_cost = float(ref["best_cost"])
    incumbent = cost_tour and reported is not None and reported <= ref_cost + COST_TOL
    out["tier3_incumbent_ok"] = 1 if incumbent else 0
    bound = _f(obs.get("best_bound"))
    sane = bound is not None and reported is not None and 0.0 < bound <= reported + COST_TOL
    out["tier3_bound_sane"] = 1 if sane else 0
    gap = _f(obs.get("optimality_gap"))
    expected_gap = None if bound is None or reported is None or bound <= 0 else (reported - bound) / bound
    gap_ok = expected_gap is not None and close(gap, expected_gap, 1e-6) and gap is not None and gap <= TIER3_GAP_LIMIT + 1e-9
    out["tier3_gap_ok"] = 1 if gap_ok else 0
    return out


def grade_facts(facts: dict, ruleset_path: str | Path, engine: str | Path | None = None) -> dict:
    """facts = {submission: {...}, reference: {...}} → wasm grade。"""
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
    import wasmfacts
    checks = build_checks(facts["submission"], facts["reference"])
    payload = dict(checks)
    payload["task_id"] = facts.get("task_id", "computing_math/branch_bound_atsp")
    return wasmfacts.engine_call(
        ruleset_path, payload, payload["task_id"],
        instruction={"type": "eval_atsp_v1", "params": {"task_id": payload["task_id"]}},
    )


if __name__ == "__main__":
    demo = {
        "submission": {
            "tier1": {"n_cities": 8, "optimal_tour": [0, 3, 1, 4, 2, 6, 5, 7], "optimal_cost": 35.0,
                       "best_bound": 35.0, "optimality_gap": 0.0, "nodes_explored": 10, "runtime_seconds": 1.0},
            "tier2": {"n_cities": 10, "optimal_tour": [0, 2, 5, 1, 8, 3, 7, 4, 9, 6], "optimal_cost": 41.25,
                       "best_bound": 41.25, "optimality_gap": 0.0, "nodes_explored": 20, "runtime_seconds": 2.0},
            "tier3": {"n_cities": 12, "best_tour": list(range(12)), "best_cost": 50.0,
                       "best_bound": 50.0, "optimality_gap": 0.0, "nodes_explored": 30,
                       "lagrangian_iterations": 5, "runtime_seconds": 3.0},
        },
        "reference": {
            "tier1": {"n_cities": 8, "optimal_cost": 35.0},
            "tier2": {"n_cities": 10, "optimal_cost": 41.25},
            "tier3": {"n_cities": 12, "best_cost": 50.0},
        },
    }
    print(json.dumps(grade_facts(demo, Path(__file__).parent / "atsp_v1.json"), indent=1, ensure_ascii=False))
