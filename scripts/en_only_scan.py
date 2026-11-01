#!/usr/bin/env python3
"""English-only scan (layer 3, language face), B-091 single-master batch.

Flags CJK in tracked text files except:
  - paths matched by scripts/en_only_allowlist.txt (whole-file globs,
    e.g. product i18n locale files; file must justify "why CJK lives here");
  - files recorded in the ratchet baseline (pre-existing CJK stock).

Ratchet semantics (design appendix D.3 "数量登记"): baseline stores one
`<path>|<cjk-line-count>` entry per stock file. A file NOT in the baseline
or whose CJK line count GREW is red; shrinking is welcome (translate stock
in place, one entry at a time) and reported as shrink candidates.

Orthogonality: this scanner enforces the LANGUAGE face only. Disclosure
scanning (public_face_scan.py) always runs on ALL files including exempt
ones — an allowlisted file is never exempt from disclosure rules.

Modes:
  en_only_scan.py                full scan; exit 1 on any hit
  en_only_scan.py --dump         print baseline lines (path|count)
  en_only_scan.py --ratchet F    exit 1 on new stock or grown counts
"""
import fnmatch
import os
import re
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

ALLOWLIST_PATH = os.path.join("scripts", "en_only_allowlist.txt")
BASELINE_PATH = os.path.join("scripts", "en-only-baseline.txt")
SELF_EXEMPT = {
    "scripts/en_only_scan.py",
    ALLOWLIST_PATH.replace("\\", "/"),
    BASELINE_PATH.replace("\\", "/"),
}
SKIP_DIRS = {".git", "node_modules", "__pycache__", "target-sc1b", ".obsidian"}
TEXT_EXT = {
    ".md", ".rs", ".py", ".ts", ".tsx", ".js", ".mjs", ".json", ".toml",
    ".yml", ".yaml", ".html", ".htm", ".css", ".txt", ".sh", ".ps1",
    ".bat", ".sql", ".cfg", ".ini", ".xml", ".svg",
}
CJK_RX = re.compile(r"[\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff]")


def load_allowlist():
    """Whole-file path globs (repo-relative, '/' separators). Missing file = no exemptions."""
    path = os.path.join(REPO_ROOT, ALLOWLIST_PATH)
    if not os.path.exists(path):
        return []
    with open(path, "r", encoding="utf-8") as f:
        return [ln.split("#", 1)[0].strip() for ln in f
                if ln.strip() and not ln.strip().startswith("#")]


def is_allowed(rel, globs):
    return any(fnmatch.fnmatch(rel, g) for g in globs)


def is_text(path):
    if os.path.splitext(path)[1].lower() not in TEXT_EXT:
        return False
    if os.path.getsize(path) > 2 * 1024 * 1024:
        return False
    try:
        with open(path, "rb") as f:
            return b"\x00" not in f.read(4096)
    except OSError:
        return False


def scan_tree():
    """Return {relpath: cjk_line_count} outside allowlist."""
    globs = load_allowlist()
    counts = {}
    for dirpath, dirnames, filenames in os.walk(REPO_ROOT):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, REPO_ROOT).replace(os.sep, "/")
            if rel in SELF_EXEMPT or rel in globs or is_allowed(rel, globs):
                continue
            if not is_text(full):
                continue
            n = 0
            try:
                with open(full, "r", encoding="utf-8", errors="replace") as f:
                    for line in f:
                        if CJK_RX.search(line):
                            n += 1
            except OSError:
                continue
            if n:
                counts[rel] = n
    return counts


def load_baseline(path):
    if not os.path.exists(path):
        return {}
    out = {}
    with open(path, "r", encoding="utf-8") as f:
        for ln in f:
            ln = ln.strip()
            if not ln or ln.startswith("#"):
                continue
            rel, _, num = ln.rpartition("|")
            if rel and num.isdigit():
                out[rel] = int(num)
    return out


def main(argv):
    if "--dump" in argv:
        counts = scan_tree()
        for rel in sorted(counts):
            print("%s|%d" % (rel, counts[rel]))
        return 0
    counts = scan_tree()
    if "--ratchet" in argv:
        i = argv.index("--ratchet")
        base = load_baseline(argv[i + 1])
        new = [rel for rel in sorted(counts) if rel not in base]
        grown = ["%s (%d > %d)" % (rel, counts[rel], base[rel])
                 for rel in sorted(counts) if rel in base and counts[rel] > base[rel]]
        shrunk = [rel for rel in sorted(base) if counts.get(rel, 0) < base[rel]]
        rc = 0
        if new:
            print("NEW CJK files (not in baseline): %d" % len(new))
            for rel in new:
                print("  %s|%d" % (rel, counts[rel]))
            rc = 1
        if grown:
            print("CJK line count GREW in stock files: %d" % len(grown))
            for g in grown:
                print("  " + g)
            rc = 1
        if rc:
            print("Move copy into allowlisted locale paths, translate to English,")
            print("or (stock review, shrink-only) refresh baseline via --dump.")
        if shrunk:
            print("info: %d baseline entries shrunk to zero or below; consider shrinking baseline." % len(shrunk))
        return rc
    if counts:
        print("CJK files outside allowlist: %d" % len(counts))
        for rel in sorted(counts):
            print("  %s|%d" % (rel, counts[rel]))
        return 1
    print("en-only: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
