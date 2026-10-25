#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Generate the evorule ecosystem third-party licence declaration set.

Outputs (into the root repo):
  THIRD-PARTY-NOTICES.md            human-readable declaration
  LICENSES/third-party/<id>/...     verbatim licence texts
  LICENSES/third-party/manifest.json machine-readable inventory

Licence texts are copied verbatim from the local cargo registry cache or
fetched from the npm registry tarball. Nothing is hand-written.
"""
import json
import os
import re
import shutil
import sys
import tarfile
import tomllib
import urllib.request
from io import BytesIO
from pathlib import Path

HOME = Path(os.path.expanduser("~"))
REG_SRC = HOME / ".cargo" / "registry" / "src"
UA = "evorule-license-audit/1.0 (third-party licence archive)"
ROOT = Path(r"D:\evorule")
OUT_LIC = ROOT / "LICENSES" / "third-party"

OBLIGATIONS = {
    "MIT": "保留版权声明与许可证全文；分发二进制时随附。",
    "MIT-0": "保留版权声明与许可证全文；分发二进制时随附。",
    "Apache-2.0": "保留版权声明、许可证全文与变更说明；上游若有 NOTICE 文件须一并附带（§4(d)）。",
    "BSD-2-Clause": "保留版权声明、条款与免责声明。",
    "BSD-3-Clause": "保留版权声明、条款与免责声明。",
    "BSD-1-Clause": "保留版权声明、条款与免责声明。",
    "ISC": "保留版权声明与许可文本。",
    "Zlib": "保留版权声明与许可文本。",
    "0BSD": "无附加义务。",
    "Unlicense": "无义务。",
    "Unicode-3.0": "保留版权声明与许可文本。",
    "Unicode-DFS-2016": "保留版权声明与许可文本。",
    "CC0-1.0": "公共领域贡献，无义务。",
    "MPL-2.0": "文件级 copyleft：被修改的 MPL 文件须以 MPL 公开；分发时保留声明与许可文本。",
    "AGPL-3.0-or-later": "网络 copyleft：以网络提供服务时须向使用者提供对应源码（§13）。",
    "BSL-1.0": "宽松许可：保留版权声明与许可文本。",
    "BlueOak-1.0.0": "宽松许可（非 OSI 认证）：保留版权声明与许可文本。",
    "CDLA-Permissive-2.0": "宽松许可：保留版权声明与许可文本。",
    "NCSA": "宽松许可：保留版权声明与许可文本。",
    "OFL-1.1": "字体许可：保留声明、不得单独售卖字体、须检查保留字体名条款。",
    "LGPL-2.1-or-later": "弱 copyleft：可改选 MIT/Apache 路径；若选 LGPL 须满足其链接与可替换要求。",
}

LIC_FILE_HINTS = {
    "MIT": ["LICENSE-MIT", "LICENSE", "COPYING", "MIT-LICENSE"],
    "MIT-0": ["LICENSE-MIT", "LICENSE", "LICENSE-MIT-0"],
    "Apache-2.0": ["LICENSE-APACHE", "LICENSE", "COPYING"],
    "BSD-2-Clause": ["LICENSE-BSD", "LICENSE"],
    "BSD-3-Clause": ["LICENSE-BSD", "LICENSE"],
    "BSD-1-Clause": ["LICENSE-BSD", "LICENSE"],
    "ISC": ["LICENSE-ISC", "LICENSE"],
    "Zlib": ["LICENSE-ZLIB", "LICENSE"],
    "0BSD": ["LICENSE", "LICENSE-0BSD"],
    "Unlicense": ["LICENSE-UNLICENSE", "LICENSE", "UNLICENSE"],
    "Unicode-3.0": ["LICENSE-UNICODE", "LICENSE"],
    "Unicode-DFS-2016": ["LICENSE-UNICODE", "LICENSE"],
    "CC0-1.0": ["LICENSE-CC0", "LICENSE"],
    "MPL-2.0": ["LICENSE-MPL-2.0", "LICENSE"],
    "AGPL-3.0-or-later": ["LICENSE", "COPYING"],
    "BSL-1.0": ["LICENSE", "LICENSE-BOOST"],
    "BlueOak-1.0.0": ["LICENSE", "LICENSE.md"],
    "CDLA-Permissive-2.0": ["LICENSE", "LICENSE.md"],
    "NCSA": ["LICENSE", "LICENSE.md"],
    "OFL-1.1": ["LICENSE", "OFL.txt"],
    "LGPL-2.1-or-later": ["LICENSE-LGPL-2.1", "LICENSE", "COPYING"],
}


def slug(lic):
    s = re.sub(r"[^A-Za-z0-9._+-]+", "-", lic).strip("-")
    return s or "UNKNOWN"


def http_get(url, timeout=60):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def scan_cargo_cache():
    """license_id -> list of (crate_dir, license_file_path)"""
    index = {}
    if not REG_SRC.is_dir():
        return index
    for idx in REG_SRC.iterdir():
        if not idx.is_dir():
            continue
        for crate in idx.iterdir():
            ct = crate / "Cargo.toml"
            if not ct.is_file():
                continue
            try:
                data = tomllib.loads(ct.read_text(encoding="utf-8", errors="replace"))
            except Exception:
                continue
            pkg = data.get("package", {})
            lic = pkg.get("license")
            if not lic:
                continue
            files = [p for p in crate.iterdir()
                     if p.is_file() and re.match(
                         r"(?i)^(license|copying|notice|unlicense|ofl)(\..*|[-_].*)?$",
                         p.name)]
            if not files:
                continue
            index.setdefault(lic, []).append((crate, files))
    return index


def pick_license_file(lic, files):
    hints = LIC_FILE_HINTS.get(lic, [])
    names = {f.name: f for f in files}
    for h in hints:
        for fn, path in names.items():
            if fn.lower() == h.lower():
                return path
    for h in hints:
        for fn, path in names.items():
            if h.lower() in fn.lower():
                return path
    for fn, path in sorted(names.items()):
        if fn.upper().startswith("LICENSE"):
            return path
    return sorted(files, key=lambda p: p.name)[0]


def npm_license_text(name, version):
    """Fetch the licence file out of the npm tarball. -> (text, filename) or None"""
    base = name.split("/")[-1]
    url = f"https://registry.npmjs.org/{name}/-/{base}-{version}.tgz"
    try:
        raw = http_get(url, timeout=90)
    except Exception:
        return None
    try:
        with tarfile.open(fileobj=BytesIO(raw), mode="r:gz") as tf:
            members = [m for m in tf.getmembers()
                       if m.isfile() and re.match(
                           r"(?i)^package/(license|copying|notice|ofl)(\..*|[-_].*)?$",
                           m.name)]
            if not members:
                return None
            members.sort(key=lambda m: (len(m.name), m.name))
            f = tf.extractfile(members[0])
            if f is None:
                return None
            return f.read().decode("utf-8", errors="replace"), members[0].name
    except Exception:
        return None


def copyright_snippet(text):
    for line in (text or "").splitlines()[:12]:
        if re.search(r"(?i)copyright", line) and len(line.strip()) > 6:
            return line.strip()[:200]
    return ""


def main():
    inv = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    OUT_LIC.mkdir(parents=True, exist_ok=True)

    # ---- collect every dependency with a resolved licence ----------------
    deps = []
    for repo, r in inv.items():
        for c in r["crates"]:
            if c["license"] in ("(local path/workspace)", "(git dependency)"):
                continue
            deps.append({"repo": repo, "ecosystem": "cargo",
                         "name": c["name"], "version": c["version"],
                         "license": c["license"]})
        for n in r["npm"]:
            deps.append({"repo": repo, "ecosystem": "npm",
                         "name": n["name"], "version": n["version"],
                         "license": n["license"], "scope": n["scope"],
                         "lockfile": n["lockfile"]})
        for p in r["python"]:
            deps.append({"repo": repo, "ecosystem": "pypi",
                         "name": p["name"], "version": p["spec"],
                         "license": p["license"] or p.get("classifier", "")})

    # normalise composite expressions to their strongest leaf for archiving
    KNOWN_IDS = {
        "AGPL-3.0-or-later", "AGPL-3.0", "Apache-2.0 WITH LLVM-exception",
        "Apache-2.0", "MIT-0", "MIT", "BSD-2-Clause", "BSD-3-Clause",
        "BSD-1-Clause", "ISC", "Zlib", "0BSD", "Unlicense", "Unicode-3.0",
        "Unicode-DFS-2016", "CC0-1.0", "MPL-2.0", "BSL-1.0", "BlueOak-1.0.0",
        "CDLA-Permissive-2.0", "NCSA", "OFL-1.1", "LGPL-2.1-or-later",
        "LGPL-2.1", "LGPL-3.0-or-later", "GPL-3.0-or-later", "EPL-2.0",
        "CC-BY-4.0", "CC-BY-3.0", "PostgreSQL", "X11", "NCSA",
    }

    def extract_ids(expr):
        """Pull every known SPDX id out of an expression (paren-safe)."""
        e = (expr or "").strip()
        if not e:
            return []
        ids = []
        for k in sorted(KNOWN_IDS, key=len, reverse=True):
            if k == "Apache-2.0 WITH LLVM-exception":
                if re.search(r"(?i)apache-2\.0\s+with\s+llvm-exception", e):
                    ids.append(k)
                continue
            if re.search(r"(?<![\w.+-])" + re.escape(k) + r"(?![\w.+-])", e):
                ids.append(k)
        return ids

    print("deps:", len(deps))
    cache = scan_cargo_cache()
    print("cargo cache license buckets:", len(cache))

    # ---- archive one verbatim text per licence id -------------------------
    wanted = set()
    for d in deps:
        wanted.update(extract_ids(d["license"]))
    wanted |= {"MIT", "Apache-2.0", "ISC", "BSD-3-Clause", "BSD-2-Clause",
               "Zlib", "Unicode-3.0", "MPL-2.0", "CC0-1.0",
               "Apache-2.0 WITH LLVM-exception"}

    archived = {}
    for lic in sorted(wanted):
        base = lic.split(" WITH ")[0].strip()
        cands = cache.get(lic) or cache.get(base) or []
        if not cands:
            # the cache key is the full expression (e.g. "(MIT OR Apache-2.0) AND NCSA")
            pat = re.compile(r"(?<![\w.+-])" + re.escape(base) + r"(?![\w.+-])")
            for key, lst in cache.items():
                if pat.search(key):
                    cands = lst
                    break
        target = OUT_LIC / slug(lic)
        target.mkdir(parents=True, exist_ok=True)
        text = None
        fname = None
        if cands:
            crate, files = cands[0]
            src = pick_license_file(base if base in LIC_FILE_HINTS else lic, files)
            if src is None:
                src = sorted(files, key=lambda p: p.name)[0]
            text = src.read_text(encoding="utf-8", errors="replace")
            fname = src.name
        else:
            # npm fallback: take a representative npm package carrying it
            for d in deps:
                if d["ecosystem"] != "npm":
                    continue
                if lic not in extract_ids(d["license"]):
                    continue
                if not d.get("version"):
                    continue
                got = npm_license_text(d["name"], d["version"])
                if got:
                    text, fname = got
                    fname = fname.split("/")[-1]
                    break
        if text:
            (target / (fname or "LICENSE")).write_bytes(
                text.replace("\r\n", "\n").replace("\r", "\n").encode("utf-8"))
            archived[lic] = {"file": fname or "LICENSE",
                              "copyright": copyright_snippet(text)}
            print(f"  archived {lic}  <- {fname}  ({len(text)} B)")
        else:
            archived[lic] = {"file": None, "copyright": "",
                             "status": "TEXT-NOT-FOUND-REQUIRED-MANUAL"}
            print(f"  !! no text for {lic}")

    Path(OUT_LIC / "manifest.json").write_bytes(
        json.dumps({"licenses": archived, "dependencies": deps},
                   indent=1, ensure_ascii=True).replace("\r\n", "\n").encode("utf-8"))
    print("wrote manifest +", len(archived), "licence dirs")


if __name__ == "__main__":
    main()
