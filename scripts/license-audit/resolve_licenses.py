#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Resolve license metadata for the extracted inventory and classify risk."""
import json
import os
import sys
import tomllib
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from extract_licenses import (registry_license, crates_io_licenses, classify,
                              ALLOW, WEAK)

UA = "mavis-license-audit/1.0 (compliance inventory)"
PYPI = "https://pypi.org/pypi/{}/json"


def pypi_license(pkg):
    name = re_split(pkg)
    try:
        req = urllib.request.Request(PYPI.format(urllib.parse.quote(name)),
                                     headers={"User-Agent": UA})
        with urllib.request.urlopen(req, timeout=30) as r:
            data = json.loads(r.read().decode("utf-8"))
        lic = data.get("info", {}).get("license") or ""
        classifiers = data.get("info", {}).get("classifiers") or []
        for c in classifiers:
            if c.startswith("License :: "):
                return lic, c
        return lic, ""
    except Exception as e:
        return "", f"ERR:{type(e).__name__}"


def re_split(spec):
    for sep in ("==", ">=", "<=", "~=", ">", "<", "[", "@"):
        if sep in spec:
            return spec.split(sep)[0].strip()
    return spec.strip()


def main():
    inv = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    out_path = Path(sys.argv[2])

    # ---- 1. collect unique registry crates across all repos -------------
    need = {}
    for repo, entry in inv.items():
        for name, ver, kind in entry["cargo"]:
            if kind == "registry":
                need.setdefault(f"{name}@{ver}", (name, ver))
    print("unique registry crates:", len(need))

    resolved = {}
    missing = []
    for key, (n, v) in need.items():
        hit = registry_license(n, v)
        if hit:
            resolved[key] = hit
        else:
            missing.append((n, v))
    print("resolved from local registry cache:", len(resolved),
          " needing crates.io:", len(missing))

    if missing:
        pairs = [(n, v) for n, v in missing]
        api = crates_io_licenses(pairs)
        for k, val in api.items():
            resolved[k] = val

    # ---- 2. per-repo classification -------------------------------------
    report = {}
    for repo, entry in inv.items():
        crates = []
        for name, ver, kind in entry["cargo"]:
            if kind != "registry":
                crates.append({"name": name, "version": ver, "kind": kind,
                               "license": "(local path/workspace)" if kind == "path/workspace"
                                          else "(git dependency)",
                               "bucket": "LOCAL" if kind == "path/workspace" else "GIT"})
                continue
            key = f"{name}@{ver}"
            src, lic, extra = resolved.get(key, ("none", "UNRESOLVED", ""))
            bucket, why = classify(lic)
            item = {"name": name, "version": ver, "license": lic,
                    "bucket": bucket, "why": why, "source": src}
            if lic == "SEE-LICENSE-FILE" and extra:
                item["file_head"] = extra.replace("\n", " ")[:200]
            crates.append(item)

        npm = []
        for lock in entry["npm"]:
            for name, ver, lic, scope in lock["packages"]:
                bucket, why = classify(lic)
                npm.append({"lockfile": lock["lockfile"], "name": name,
                            "version": ver, "license": lic, "scope": scope,
                            "bucket": bucket, "why": why})

        py = []
        for rf in entry["python"]:
            for spec in rf["requirements"]:
                nm = re_split(spec)
                lic, cls = pypi_license(nm)
                bucket, why = classify(lic or cls)
                py.append({"file": rf["file"], "spec": spec, "name": nm,
                           "license": lic, "classifier": cls,
                           "bucket": bucket, "why": why})

        report[repo] = {"crates": crates, "npm": npm, "python": py}

    out_path.write_text(json.dumps(report, indent=1, ensure_ascii=True),
                        encoding="utf-8")
    print("wrote", out_path)

    # ---- 3. console summary ---------------------------------------------
    agg = {}
    for repo, r in report.items():
        for c in r["crates"] + r["npm"] + r["python"]:
            agg.setdefault(c["bucket"], []).append((repo, c["name"], c["license"]))
    for b in ("BLOCK", "REVIEW", "UNKNOWN", "ALLOWED", "LOCAL", "GIT"):
        items = agg.get(b, [])
        print(f"\n== {b} ({len(items)}) ==")
        seen = set()
        for repo, name, lic in items:
            k = (name, lic)
            if k in seen:
                continue
            seen.add(k)
            print(f"  {name:<44} {lic:<40} [{repo}]")


if __name__ == "__main__":
    main()
