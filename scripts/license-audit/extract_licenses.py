#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Third-party dependency license inventory for the EvoRule in-service repos.
Output: JSON, ASCII-only (ensure_ascii=True) to stay encoding-safe.
License source priority: local cargo registry cache -> crates.io API -> UNKNOWN.
"""
import json
import os
import re
import sys
import tomllib
import urllib.parse
import urllib.request
from pathlib import Path

HOME = Path(os.path.expanduser("~"))
REG_SRC = HOME / ".cargo" / "registry" / "src"
UA = "mavis-license-audit/1.0 (compliance inventory)"

ALLOW = {
    "AGPL-3.0-or-later", "AGPL-3.0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
    "MIT", "MIT-0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0",
    "Unicode-DFS-2016", "Zlib", "CC0-1.0", "MPL-2.0", "0BSD", "BSL-1.0",
    "Unlicense", "Apache-2.0 OR MIT", "MIT OR Apache-2.0", "Apache-2.0 OR MIT",
    "BSD-3-Clause OR MIT", "ISC OR MIT", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
    "BSD-2-Clause OR Apache-2.0 OR MIT", "Unicode-DFS-2016 OR Apache-2.0 OR MIT",
    "MIT OR Apache-2.0 OR Zlib", "MIT OR Apache-2.0", "Apache-2.0 OR BSL-1.0",
    "CC-BY-4.0", "OFL-1.1", "BSD-3-Clause AND MIT", "Apache-2.0 AND MIT",
    "MIT AND Apache-2.0", "CC-BY-3.0", "CC-BY-SA-4.0", "MIT OR Unlicense",
}

# Licenses that are NOT in the root deny.toml allow-list but are broadly compatible
# with AGPL-3.0 (file-level copyleft / weak copyleft / public-domain-like).
WEAK = {
    "MPL-2.0": "weak copyleft (file-level), AGPL-compatible",
    "CC0-1.0": "public domain dedication, no obligation",
    "CC-BY-4.0": "attribution required (content, not software)",
    "CC-BY-3.0": "attribution required (content, not software)",
    "OFL-1.1": "font license, OFL reserved-font-name clause needs check",
    "Unicode-3.0": "permissive, AGPL-compatible",
}

# Strong copyleft / non-commercial / source-available -> legal review required.
STRONG_COPYLEFT = re.compile(
    r"\b(GPL-2\.0|GPL-3\.0|LGPL|AGPL-3\.0-only|GPL-2\.0-only)\b", re.I)
NONCOMMERCIAL = re.compile(r"NC|NonCommercial", re.I)
SOURCELESS = re.compile(r"SSPL|BUSL|Elastic|JSON|Commons-Clause|NoSell", re.I)
PROPRIETARY = re.compile(r"proprietary|unlicense OR proprietary", re.I)


def registry_license(name, version):
    """Read license from the local cargo registry source cache."""
    if not REG_SRC.is_dir():
        return None
    for idx in REG_SRC.iterdir():
        if not idx.is_dir():
            continue
        crate = idx / f"{name}-{version}"
        ct = crate / "Cargo.toml"
        if not ct.is_file():
            continue
        try:
            data = tomllib.loads(ct.read_text(encoding="utf-8", errors="replace"))
        except Exception:
            return None
        pkg = data.get("package", {}) if isinstance(data.get("package"), dict) else {}
        lic = pkg.get("license")
        if lic:
            return ("registry-cache", lic, "")
        lf = pkg.get("license-file")
        if lf:
            cand = crate / lf
            head = ""
            if cand.is_file():
                try:
                    head = cand.read_text(encoding="utf-8", errors="replace")[:400]
                except Exception:
                    head = ""
            return ("registry-cache", "SEE-LICENSE-FILE", head)
        return ("registry-cache", "NO-LICENSE-FIELD", "")
    return None


def crates_io_licenses(pairs):
    """Batch query crates.io for license metadata. pairs = [(name, version)]."""
    out = {}
    base = "https://crates.io/api/v1/crates?"
    for i in range(0, len(pairs), 60):
        chunk = pairs[i:i + 60]
        params = "&".join(
            "ids[]=" + urllib.parse.quote(f"{n}@{v}", safe="") for n, v in chunk)
        req = urllib.request.Request(base + params, headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                payload = json.loads(r.read().decode("utf-8"))
        except Exception as e:
            for n, v in chunk:
                out[f"{n}@{v}"] = ("crates.io-error", f"ERR:{type(e).__name__}", "")
            continue
        got = set()
        for c in payload.get("crates", []):
            key = f"{c['name']}@{c.get('max_version','')}"
            got.add(key)
            lic = c.get("license") or ""
            out[key] = ("crates.io", lic, "")
            if not lic:
                out[key] = ("crates.io", "NO-LICENSE-FIELD", "")
        for n, v in chunk:
            k = f"{n}@{v}"
            if k not in out and k not in got:
                out[k] = ("crates.io", "NOT-FOUND", "")
    return out


def parse_cargo_lock(path):
    """Return list of (name, version, source_kind)."""
    try:
        data = tomllib.loads(Path(path).read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return []
    pkgs = []
    for p in data.get("package", []):
        src = p.get("source", "")
        kind = "path/workspace" if not src else (
            "registry" if src.startswith("registry+") else "git")
        pkgs.append((p["name"], p.get("version", ""), kind))
    return pkgs


def parse_package_lock(path):
    """Return list of (name, version, license, scope)."""
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8", errors="replace"))
    except Exception:
        return []
    out = []
    pkgs = data.get("packages") or {}
    for key, meta in pkgs.items():
        if not isinstance(meta, dict):
            continue
        name = meta.get("name")
        if not name:
            if key.startswith("node_modules/"):
                name = key.split("node_modules/")[-1]
            elif key == "":
                name = data.get("name", "(root)")
            else:
                name = key
        lic = meta.get("license")
        if not lic:
            lic = "UNLICENSED/UNDECLARED"
        scope = "prod"
        if meta.get("dev"):
            scope = "dev"
        if meta.get("optional"):
            scope += "+optional"
        out.append((name, meta.get("version", ""), lic, scope))
    if not out:
        for key, meta in (data.get("dependencies") or {}).items():
            if isinstance(meta, dict):
                out.append((key, meta.get("version", ""),
                            meta.get("license", "UNLICENSED/UNDECLARED"), "legacy"))
    return out


def classify(lic):
    """Classify one license string into a risk bucket."""
    l = (lic or "").strip()
    if not l or l in ("NO-LICENSE-FIELD", "UNLICENSED/UNDECLARED", "NOT-FOUND"):
        return ("UNKNOWN", "no license metadata - must resolve manually")
    if l == "SEE-LICENSE-FILE":
        return ("UNKNOWN", "license-file points to bundled text - needs reading")
    if l in ALLOW:
        return ("ALLOWED", "in root deny.toml allow-list")
    if l in WEAK:
        return ("REVIEW", WEAK[l])
    if SOURCELESS.search(l):
        return ("BLOCK", "source-available / non-OSS license - cannot ship in AGPL product")
    if NONCOMMERCIAL.search(l):
        return ("BLOCK", "non-commercial restriction - incompatible with commercial dual-license model")
    if STRONG_COPYLEFT.search(l):
        return ("REVIEW", "strong copyleft - verify compatibility with AGPL + commercial offer")
    if "MIT" in l or "Apache" in l or "BSD" in l or "ISC" in l:
        return ("REVIEW", "composite expression not in allow-list - check SPDX expression")
    return ("REVIEW", "unrecognized license expression")


def main():
    repos = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    inventory = {}
    for repo, path in repos.items():
        p = Path(path)
        entry = {"cargo": [], "npm": [], "python": [], "other": []}

        lock = p / "Cargo.lock"
        if lock.is_file():
            entry["cargo"] = parse_cargo_lock(lock)

        for root, dirs, files in os.walk(p):
            dirs[:] = [d for d in dirs
                       if d not in ("node_modules", "target", ".git", "dist",
                                    "build", ".venv", "__pycache__", ".next",
                                    "vendor", "out", ".turbo")]
            rel_root = Path(root).relative_to(p)
            if len(rel_root.parts) > 3:
                dirs[:] = []
                continue
            for fn in files:
                if fn == "package-lock.json":
                    rel = str((rel_root / fn)).replace("\\", "/")
                    items = parse_package_lock(Path(root) / fn)
                    if items:
                        entry["npm"].append({"lockfile": rel, "packages": items})
                elif fn.startswith("requirements") and fn.endswith(".txt"):
                    req = Path(root) / fn
                    lines = []
                    for ln in req.read_text(encoding="utf-8", errors="replace").splitlines():
                        ln = ln.strip()
                        if not ln or ln.startswith("#") or ln.startswith("-"):
                            continue
                        lines.append(ln)
                    if lines:
                        entry["python"].append({
                            "file": str((rel_root / fn)).replace("\\", "/"),
                            "requirements": lines})

        inventory[repo] = entry

    out = Path(sys.argv[2])
    out.write_text(json.dumps(inventory, indent=1, ensure_ascii=True),
                   encoding="utf-8")
    print("wrote", out)


if __name__ == "__main__":
    main()
