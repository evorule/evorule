#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Fetch the two licence texts that are not distributed by their upstream crates."""
import html
import re
import urllib.request
from pathlib import Path

UA = {"User-Agent": "mavis-license-audit/1.0 (compliance inventory)"}
OUT = Path(r"D:\evorule\LICENSES\third-party")


def get(url):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=90) as r:
        return r.read().decode("utf-8", errors="replace")


def strip_tags(s):
    s = re.sub(r"(?is)<(script|style).*?</\1>", " ", s)
    s = re.sub(r"(?i)<br\s*/?>", "\n", s)
    s = re.sub(r"(?i)</p>", "\n\n", s)
    s = re.sub(r"<[^>]+>", "", s)
    s = html.unescape(s)
    s = re.sub(r"[ \t]+", " ", s)
    s = re.sub(r"\n\s*\n\s*\n+", "\n\n", s)
    return s.strip()


def main():
    # 1. LGPL-2.1 (plain text, authoritative: gnu.org)
    tgt = OUT / "LGPL-2.1-or-later"
    tgt.mkdir(parents=True, exist_ok=True)
    txt = get("https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt")
    (tgt / "LICENSE-LGPL-2.1").write_text(txt, encoding="utf-8")
    (tgt / "SOURCE.txt").write_text(
        "Source: https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt\n"
        "Retrieved: 2026-10-24\n"
        "Reason: r-efi and 10 other crates declare 'MIT OR Apache-2.0 OR LGPL-2.1-or-later'.\n"
        "        The upstream crate ships no LGPL text. EvoRule elects the MIT/Apache\n"
        "        branch, so LGPL is archived for reference only (no LGPL obligations apply).\n",
        encoding="utf-8")
    print("LGPL-2.1 saved:", len(txt), "B")

    # 2. NCSA (HTML page -> license text block)
    tgt = OUT / "NCSA"
    tgt.mkdir(parents=True, exist_ok=True)
    page = get("https://opensource.org/licenses/NCSA")
    body = strip_tags(page)
    start = body.find("Permission is hereby granted")
    if start < 0:
        start = body.find("University of Illinois/NCSA Open Source License")
    end = body.find("END OF TERMS AND CONDITIONS")
    if start < 0:
        print("NCSA: could not locate text block")
        return
    chunk = body[start:]
    if end > start:
        chunk = body[start:end + len("END OF TERMS AND CONDITIONS")]
    chunk = chunk.strip()
    (tgt / "LICENSE-NCSA").write_text(chunk + "\n", encoding="utf-8")
    (tgt / "SOURCE.txt").write_text(
        "Source: https://opensource.org/licenses/NCSA (OSI, University of Illinois/NCSA\n"
        "        Open Source License page), licence text block extracted verbatim.\n"
        "Retrieved: 2026-10-24\n"
        "Reason: libfuzzer-sys (evo-agent dev-dependency) declares\n"
        "        '(MIT OR Apache-2.0) AND NCSA'. The NCSA part is a mandatory AND\n"
        "        component and cannot be elected away, but the upstream crate ships\n"
        "        no NCSA text.\n",
        encoding="utf-8")
    print("NCSA saved:", len(chunk), "B")
    print(chunk[:300])


if __name__ == "__main__":
    main()
