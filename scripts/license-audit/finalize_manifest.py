#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Fill the manifest from the actual archive directories + normalise line endings."""
import json
import re
from pathlib import Path

OUT = Path(r"D:\evorule\LICENSES\third-party")
man = json.loads((OUT / "manifest.json").read_text(encoding="utf-8"))

# 1. normalise every archived text to LF, no BOM
for d in sorted(OUT.iterdir()):
    if not d.is_dir():
        continue
    for f in d.iterdir():
        if not f.is_file():
            continue
        raw = f.read_bytes()
        txt = raw.decode("utf-8", errors="replace")
        txt = txt.replace("\r\n", "\n").replace("\r", "\n")
        f.write_bytes(txt.replace("\r\n", "\n").replace("\r", "\n").encode("utf-8"))
        if "SOURCE.txt" not in f.name:
            head = "\n".join(txt.split("\n")[:12])
            m = re.search(r"(?i)copyright[^\n]*", head)
            if m:
                man["licenses"].setdefault(d.name, {})["copyright"] = \
                    m.group(0).strip()[:200]

# 2. reflect the real archive contents in the manifest
for d in sorted(OUT.iterdir()):
    if not d.is_dir():
        continue
    files = sorted(f.name for f in d.iterdir() if f.is_file()
                   and f.name != "SOURCE.txt")
    entry = man["licenses"].setdefault(d.name, {})
    entry["file"] = files[0] if files else None
    entry["files"] = files
    entry.setdefault("status", "archived")

man["_meta"] = {
    "generated": "2026-10-24",
    "ecosystem": "evorule",
    "repos_covered": 13,
    "note": "licence texts copied verbatim from the cargo registry cache or the "
            "npm registry tarball; NCSA / LGPL-2.1 fetched from OSI and gnu.org "
            "because the upstream crates ship no text (see SOURCE.txt in each dir)",
}
(OUT / "manifest.json").write_bytes(
    json.dumps(man, indent=1, ensure_ascii=True).replace("\r\n", "\n").encode("utf-8"))

print("licence dirs:", len([d for d in OUT.iterdir() if d.is_dir()]))
missing = [k for k, v in man["licenses"].items() if not v.get("file")]
print("dirs without a text file:", missing)
print("total dependency records:", len(man["dependencies"]))
