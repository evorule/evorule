#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""SPDX-aware policy evaluation of the evorule dependency inventory.

Answers: which third-party dependencies would FAIL the root deny.toml
allow-list, and which are legally problematic regardless of policy.
"""
import json
import re
import sys
from pathlib import Path

# Root deny.toml [licenses].allow -- verbatim, do not drift.
ROOT_ALLOW = {
    "AGPL-3.0-or-later", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
    "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0",
    "Unicode-DFS-2016", "Zlib", "CC0-1.0", "MPL-2.0",
}

# Permissive but not on the root allow-list: legal to use, needs allow-list
# addition (or a documented exception) for cargo-deny to pass.
PERMISSIVE_UNLISTED = {
    "0BSD": "permissive (BSD 0-clause)",
    "Unlicense": "public domain dedication",
    "MIT-0": "permissive (MIT-0)",
    "BSD-1-Clause": "permissive (BSD 1-clause)",
    "BSL-1.0": "permissive (Boost)",
    "BlueOak-1.0.0": "permissive but NOT OSI-approved",
    "CDLA-Permissive-2.0": "permissive (Community Data License)",
    "PostgreSQL": "permissive (PostgreSQL licence)",
    "NCSA": "permissive (UIUC)",
    "X11": "permissive (X11)",
    "Beerware": "permissive (Beerware)",
    "CC-BY-4.0": "attribution required (content licence)",
    "CC-BY-3.0": "attribution required (content licence)",
    "OFL-1.1": "SIL font licence - check reserved-font-name clause",
}

# Legally significant: copyleft, attribution-carrying, or restricted.
COPYLEFT = {
    "GPL-2.0": "strong copyleft (GPL-2.0-only): NOT compatible with AGPL-3.0",
    "GPL-2.0-only": "strong copyleft (GPL-2.0-only): NOT compatible with AGPL-3.0",
    "GPL-2.0-or-later": "GPL-2.0-or-later: may be upgraded to GPL-3.0 -> compatible",
    "GPL-3.0": "strong copyleft (GPL-3.0): compatible with AGPL-3.0",
    "GPL-3.0-only": "strong copyleft (GPL-3.0-only): compatible with AGPL-3.0",
    "GPL-3.0-or-later": "strong copyleft (GPL-3.0-or-later): compatible",
    "AGPL-3.0": "network copyleft: compatible (but doubles obligations)",
    "AGPL-3.0-only": "network copyleft: compatible",
    "LGPL-2.1": "weak copyleft (LGPL-2.1-only): compatible via GPL-3.0 upgrade path",
    "LGPL-2.1-only": "weak copyleft (LGPL-2.1-only): compatible via upgrade path",
    "LGPL-2.1-or-later": "weak copyleft: may elect LGPL-3.0 -> AGPL-compatible",
    "LGPL-3.0": "weak copyleft (LGPL-3.0): compatible with AGPL-3.0",
    "LGPL-3.0-only": "weak copyleft (LGPL-3.0-only): compatible",
    "LGPL-3.0-or-later": "weak copyleft: compatible",
    "MPL-2.0": "file-level copyleft: AGPL-compatible, per-file notice required",
    "EPL-2.0": "file-level copyleft: GPL-compatible, per-file notice required",
    "CDDL-1.0": "file-level copyleft: GPL-compatible, per-file notice required",
}

RESTRICTED = re.compile(
    r"SSPL|BUSL|Elastic|Commons-Clause|NoSell|noncommercial|NonCommercial|"
    r"CC-BY-NC|PolyForm|prosop|JSON-6|AGPL-3\.0-only$", re.I)

UNKNOWN_LIC = {"", "NO-LICENSE-FIELD", "UNLICENSED/UNDECLARED", "NOT-FOUND",
               "UNRESOLVED", "SEE-LICENSE-FILE"}


def leaves(expr):
    """Return (list_of_leaf_licenses, operators) for a licence expression."""
    e = (expr or "").strip()
    if not e:
        return [], []
    # WITH binds to the preceding leaf
    parts = re.split(r"\s+OR\s+|\s+or\s+|\s+AND\s+|\s+and\s+|/", e)
    ops = []
    for m in re.finditer(r"\s+OR\s+|\s+or\s+|\s+AND\s+|\s+and\s+|/", e):
        ops.append("OR" if m.group(0).strip().lower() == "or" else
                   ("AND" if m.group(0).strip().lower() == "and" else "OR"))
    # strip grouping parens: "(MPL-2.0 OR Apache-2.0)" -> two leaves
    cleaned = []
    for p in parts:
        p = p.strip()
        p = p.strip("()").strip()
        if p:
            cleaned.append(p)
    return cleaned, ops


def leaf_base(leaf):
    """Strip exception suffix; return (base, exception)."""
    if " WITH " in leaf:
        b, x = leaf.split(" WITH ", 1)
        return b.strip(), x.strip()
    return leaf.strip(), None


def evaluate(expr):
    """Return dict with policy verdict + legal note for one licence expression."""
    ls, ops = leaves(expr)
    if not ls:
        return {"verdict": "UNKNOWN", "note": "no licence expression",
                "licenses": []}
    if expr.strip() in UNKNOWN_LIC:
        return {"verdict": "UNKNOWN", "note": "licence metadata absent",
                "licenses": ls}

    bases = []
    for l in ls:
        b, exc = leaf_base(l)
        bases.append((b, exc))

    all_allow = all(
        (b in ROOT_ALLOW) or
        (b == "Apache-2.0" and exc == "LLVM-exception")
        for b, exc in bases)

    any_allow = any(
        (b in ROOT_ALLOW) or
        (b == "Apache-2.0" and exc == "LLVM-exception")
        for b, exc in bases)

    is_or = "OR" in ops

    restricted = [b for b, _ in bases if RESTRICTED.search(b)]
    if restricted:
        return {"verdict": "BLOCK", "licenses": ls,
                "note": "restricted / source-available licence: " +
                        ", ".join(restricted)}

    unknown = [b for b, _ in bases if b not in ROOT_ALLOW
               and b not in PERMISSIVE_UNLISTED and b not in COPYLEFT]
    copyleft = [b for b, _ in bases if b in COPYLEFT]
    unlisted = [b for b, _ in bases if b in PERMISSIVE_UNLISTED]

    notes = []
    for b in copyleft:
        notes.append(f"{b}: {COPYLEFT[b]}")
    for b in unlisted:
        notes.append(f"{b}: {PERMISSIVE_UNLISTED[b]}")
    for b in unknown:
        notes.append(f"{b}: not in allow-list, not classified -- review")

    if all_allow:
        verdict = "PASS"
    elif is_or and any_allow:
        verdict = "PASS"          # OR: user may elect the allowed alternative
    elif copyleft and not unknown:
        verdict = "REVIEW"        # legally fine, policy list needs widening
    elif unlisted and not unknown:
        verdict = "REVIEW"
    elif unknown:
        verdict = "REVIEW"
    else:
        verdict = "REVIEW"

    return {"verdict": verdict, "licenses": ls,
            "note": "; ".join(notes) if notes else "all components allowed"}


def main():
    inv = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    summary = {}
    fails = []
    reviews = []
    unknowns = []
    blocks = []

    for repo, r in inv.items():
        rows = []
        for c in r["crates"]:
            if c["license"] in ("(local path/workspace)", "(git dependency)"):
                continue
            ev = evaluate(c["license"])
            rows.append((c["name"], c["version"], c["license"], ev))
            if ev["verdict"] == "BLOCK":
                blocks.append((repo, c["name"], c["version"], c["license"], ev["note"]))
            elif ev["verdict"] == "REVIEW":
                reviews.append((repo, c["name"], c["version"], c["license"], ev["note"]))
            elif ev["verdict"] == "UNKNOWN":
                unknowns.append((repo, c["name"], c["version"], c["license"], ev["note"]))
        for n in r["npm"]:
            ev = evaluate(n["license"])
            if ev["verdict"] == "BLOCK":
                blocks.append((repo, n["name"], n["version"], n["license"], ev["note"]))
            elif ev["verdict"] == "REVIEW":
                reviews.append((repo, n["name"], n["version"], n["license"], ev["note"]))
            elif ev["verdict"] == "UNKNOWN":
                unknowns.append((repo, n["name"], n["version"], n["license"], ev["note"]))
        for p in r["python"]:
            ev = evaluate(p["license"] or p.get("classifier", ""))
            if ev["verdict"] == "BLOCK":
                blocks.append((repo, p["name"], "", p["license"], ev["note"]))
            elif ev["verdict"] == "REVIEW":
                reviews.append((repo, p["name"], "", p["license"], ev["note"]))
            elif ev["verdict"] == "UNKNOWN":
                unknowns.append((repo, p["name"], "", p["license"], ev["note"]))
        summary[repo] = rows

    def uniq(items):
        seen = set()
        out = []
        for it in items:
            k = (it[1], it[3])
            if k in seen:
                continue
            seen.add(k)
            out.append(it)
        return out

    blocks, reviews, unknowns = uniq(blocks), uniq(reviews), uniq(unknowns)

    print("== BLOCK ==", len(blocks))
    for b in blocks:
        print("   ", b)
    print("\n== REVIEW (unique) ==", len(reviews))
    for b in reviews:
        print("   ", b[1], b[2], "|", b[3], "|", b[4], "|", b[0])
    print("\n== UNKNOWN (unique) ==", len(unknowns))
    for b in unknowns:
        print("   ", b[1], b[2], "|", b[3], "|", b[4], "|", b[0])

    out = {"blocks": blocks, "reviews": reviews, "unknowns": unknowns}
    Path(sys.argv[2]).write_text(json.dumps(out, indent=1, ensure_ascii=True),
                                  encoding="utf-8")
    print("\nwrote", sys.argv[2])


if __name__ == "__main__":
    main()
