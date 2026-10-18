#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# =============================================================================
# check_ecosystem_deps.py — EvoRule 生态依赖地图校验器（依赖随动机制一期）
#
# 定位：ecosystem-deps.toml（生态依赖地图 SSOT）的一致性校验 + 发布面落后巡检。
#       地图声明「谁发布什么、谁消费什么、何种耦合」；本脚本保证地图不漂移、
#       依赖不静默落后。
#
# 规则：
#   R1 地图↔实况一致性：consume.deps 逐条对照 manifest 实况
#      （version / patch 三查 / git-rev / sibling-path / build-fixture 守护脚本）
#   R2 反向完整性：检出仓 Cargo.toml 中的 evorule-* 依赖必须已在地图登记
#   R3 patch 双向一致：patch mode 声明 ↔ patches/ 目录互为镜像
#   R4 发布面落后巡检（--check-latest，需网络）：crates.io 最新版 vs 各仓锁定，
#      0.x 语义下 major.minor 落后即违规（caret 依赖同 minor 内不判落后）
#
# 用法：
#   python scripts/check_ecosystem_deps.py                     # R1-R3 本地一致性
#   python scripts/check_ecosystem_deps.py --check-latest      # 加 R4（需网络）
#   python scripts/check_ecosystem_deps.py --json              # JSON 输出（CI 消费）
#
# 退出码：0 = 全部通过；1 = 违规；2 = 环境错误
# 检出解析：按脚本所在仓的父目录找兄弟检出（如 D:\evorule 的兄弟 D:\evorule-server）；
#           未检出的仓 SKIP（CI 侧按需 checkout 组合后再跑）。
# =============================================================================

import argparse
import json
import re
import sys
import urllib.request
from pathlib import Path

try:
    import tomllib
except ImportError:  # Python < 3.11
    print("ERROR: 需要 Python 3.11+（tomllib）", file=sys.stderr)
    sys.exit(2)

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO_ROOT = Path(__file__).resolve().parent.parent
MAP_PATH = REPO_ROOT / "ecosystem-deps.toml"
SKIP_DIRS = {"target", "target-sc1b", "node_modules", ".git", "fuzz", "patches"}
CRATES_API = "https://crates.io/api/v1/crates/{name}"

violations: list = []
notices: list = []


def fail(rule: str, msg: str) -> None:
    violations.append((rule, msg))


def note(msg: str) -> None:
    notices.append(msg)


def load_map() -> dict:
    if not MAP_PATH.exists():
        print(f"ERROR: 地图不存在: {MAP_PATH}", file=sys.stderr)
        sys.exit(2)
    with open(MAP_PATH, "rb") as f:
        return tomllib.load(f)


def find_checkout(repo: str) -> Path | None:
    """按兄弟检出解析仓目录；主仓自身返回 REPO_ROOT。"""
    if repo == _self_repo():
        return REPO_ROOT
    sibling = REPO_ROOT.parent / repo
    return sibling if (sibling / ".git").exists() or (sibling / "Cargo.toml").exists() else None


def _self_repo() -> str:
    return REPO_ROOT.name


def parse_manifest_toml(path: Path) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def dep_section(manifest: dict, section: str) -> dict:
    return manifest.get(section, {}) or {}


def iter_dep_tables(manifest: dict):
    for sec in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, val in dep_section(manifest, sec).items():
            yield sec, name, val


# ---------------------------------------------------------------------------
# R1: 地图声明 ↔ manifest 实况
# ---------------------------------------------------------------------------

def check_r1(ecomap: dict) -> None:
    for key, consumer in ecomap.get("consume", {}).items():
        repo = consumer["repo"]
        checkout = find_checkout(repo)
        if checkout is None:
            note(f"SKIP {repo}: 未检出（兄弟目录不存在）")
            continue
        for rel in consumer.get("manifests", []):
            mpath = checkout / rel
            if not mpath.exists():
                fail("R1", f"{repo}: manifest 不存在 {rel}")
                continue
            manifest = parse_manifest_toml(mpath)
            for dep in consumer.get("deps", []):
                _check_one_dep(repo, checkout, manifest, dep)


def _check_one_dep(repo: str, checkout: Path, manifest: dict, dep: dict) -> None:
    pkg, mode, pinned = dep["package"], dep["mode"], str(dep["pinned"])
    if mode == "build-fixture":
        _check_build_fixture(repo, checkout, dep)
        return
    found = None
    for _sec, _name, val in iter_dep_tables(manifest):
        if _name == pkg:
            found = val
            break
    if found is None:
        fail("R1", f"{repo}: 地图声明依赖 {pkg}({mode}) 但 manifest 未引用")
        return
    if mode == "version":
        ver = found if isinstance(found, str) else (found or {}).get("version")
        if not ver:
            fail("R1", f"{repo}: {pkg} 依赖无 version 字段")
        elif str(ver) != pinned:
            fail("R1", f"{repo}: {pkg} 锁定 {ver} ≠ 地图 {pinned}")
        feats = (found or {}).get("features") if isinstance(found, dict) else None
        want = dep.get("features")
        if want and (feats or []) != want:
            fail("R1", f"{repo}: {pkg} features={feats} ≠ 地图 {want}")
    elif mode == "patch":
        _check_patch_dep(repo, checkout, manifest, pkg, pinned, found)
    elif mode == "git-rev":
        gitv = found if isinstance(found, dict) else {}
        rev = str(gitv.get("rev", ""))
        if not rev:
            fail("R1", f"{repo}: {pkg} 依赖无 rev 锁定")
        elif not rev.startswith(pinned):
            fail("R1", f"{repo}: {pkg} rev {rev[:8]} ≠ 地图 {pinned}")
    elif mode == "sibling-path":
        p = (found or {}).get("path") if isinstance(found, dict) else None
        if not p:
            fail("R1", f"{repo}: {pkg} 依赖无 path 字段")
        elif str(p) != pinned and not str(pinned).endswith(str(p)):
            fail("R1", f"{repo}: {pkg} path {p} ≠ 地图 {pinned}")


def _check_patch_dep(repo: str, checkout: Path, manifest: dict, pkg: str, pinned: str, found) -> None:
    ver = found if isinstance(found, str) else (found or {}).get("version")
    if not ver or str(ver) != pinned:
        fail("R1", f"{repo}: {pkg}(patch) manifest 锁定 {ver} ≠ 地图 {pinned}")
    root_manifest = checkout / "Cargo.toml"
    if not root_manifest.exists():
        fail("R1", f"{repo}: 仓根 Cargo.toml 缺失（[patch.crates-io] 无法核验）")
        return
    rm = parse_manifest_toml(root_manifest)
    patch_table = rm.get("patch", {}).get("crates-io", {}) or {}
    if pkg not in patch_table:
        fail("R1", f"{repo}: {pkg}(patch) 未在仓根 [patch.crates-io] 登记")
    pdir = checkout / "patches" / pkg
    if not pdir.exists():
        fail("R1", f"{repo}: patches/{pkg} 目录不存在")


def _check_build_fixture(repo: str, checkout: Path, dep: dict) -> None:
    # 一期口径：核验声明中引用的跨仓守护脚本存在（一致性本体由该脚本负责）
    guard = dep.get("note", "")
    m = re.search(r"([\w\-/]+\.py)", guard)
    if not m:
        note(f"R1 {repo}: build-fixture {dep['package']} 未声明守护脚本（建议补充）")
        return
    gpath = checkout / "scripts" / m.group(1).split("/")[-1]
    if not gpath.exists():
        fail("R1", f"{repo}: build-fixture 守护脚本缺失 {gpath.name}")


# ---------------------------------------------------------------------------
# R2: 反向完整性（检出的仓里 evorule-* 依赖必须已登记）
# ---------------------------------------------------------------------------

def iter_manifests(checkout: Path):
    """剪枝遍历仓内 Cargo.toml（跳过 SKIP_DIRS 目录子树，避免误入巨型产物目录）。"""
    import os
    for dirpath, dirnames, filenames in os.walk(checkout):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS and not d.startswith("target")]
        if "Cargo.toml" in filenames:
            yield Path(dirpath) / "Cargo.toml"


def check_r2(ecomap: dict) -> None:
    registered = set()
    for p in ecomap.get("publish", {}).values():
        registered.update(p.get("crates", []))
    for intra in ecomap.get("intra", {}).values():
        registered.update(intra.get("crates", []))

    repos = set()
    for c in ecomap.get("consume", {}).values():
        repos.add(c["repo"])
    repos.add(_self_repo())

    for repo in sorted(repos):
        checkout = find_checkout(repo)
        if checkout is None:
            continue
        local = registered | _patch_pkgs(checkout)
        for mpath in iter_manifests(checkout):
            rel_parts = mpath.relative_to(checkout).parts
            try:
                manifest = parse_manifest_toml(mpath)
            except Exception as e:
                note(f"R2 {repo}: 跳过不可解析 {mpath.relative_to(checkout)} ({e})")
                continue
            for _sec, name, val in iter_dep_tables(manifest):
                if not name.startswith("evorule-"):
                    continue
                if name == _manifest_self_name(manifest):
                    continue
                if isinstance(val, dict) and val.get("path") and repo != _self_repo():
                    intra_paths = ecomap.get("intra", {}).get(repo, {}).get("crates", [])
                    if _manifest_self_name(manifest) in intra_paths:
                        continue  # 仓内 path 依赖已由 intra 登记
                if name not in local:
                    fail("R2", f"{repo}: {mpath.relative_to(checkout)} 依赖 {name} 未在地图登记")


def _manifest_self_name(manifest: dict) -> str:
    return (manifest.get("package") or {}).get("name", "")


def _patch_pkgs(checkout: Path) -> set:
    rm_path = checkout / "Cargo.toml"
    if not rm_path.exists():
        return set()
    try:
        rm = parse_manifest_toml(rm_path)
    except Exception:
        return set()
    return set((rm.get("patch", {}).get("crates-io", {}) or {}).keys())


# ---------------------------------------------------------------------------
# R3: patch 目录双向一致
# ---------------------------------------------------------------------------

def check_r3(ecomap: dict) -> None:
    for c in ecomap.get("consume", {}).values():
        checkout = find_checkout(c["repo"])
        if checkout is None:
            continue
        declared = {dep["package"] for dep in c.get("deps", []) if dep["mode"] == "patch"}
        pdir = checkout / "patches"
        on_disk = {d.name for d in pdir.iterdir() if d.is_dir()} if pdir.exists() else set()
        for extra in sorted(on_disk - declared):
            fail("R3", f"{c['repo']}: patches/{extra} 在盘但地图无 patch mode 声明")
        for missing in sorted(declared - on_disk):
            fail("R3", f"{c['repo']}: 地图声明 {missing}(patch) 但 patches/ 无此目录")


# ---------------------------------------------------------------------------
# R4: 发布面落后巡检（需网络）
# ---------------------------------------------------------------------------

def crates_latest(name: str) -> str | None:
    req = urllib.request.Request(CRATES_API.format(name=name), headers={"User-Agent": "evorule-ecosystem-deps-check"})
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            data = json.load(resp)
        return data.get("crate", {}).get("max_version")
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return "UNPUBLISHED"
        note(f"R4 {name}: crates.io 查询失败 ({e})")
        return None
    except Exception as e:
        note(f"R4 {name}: crates.io 查询失败 ({e})")
        return None


def minor_pair(v: str) -> tuple:
    parts = re.findall(r"\d+", v)
    return (int(parts[0]), int(parts[1])) if len(parts) >= 2 else (0, 0)


def version_triple(v: str) -> tuple:
    parts = re.findall(r"\d+", v)
    while len(parts) < 3:
        parts.append("0")
    return tuple(int(p) for p in parts[:3])


def check_r4(ecomap: dict) -> None:
    latest = {}
    for key, p in ecomap.get("publish", {}).items():
        if p.get("channel") != "crates.io":
            continue
        for crate in p.get("crates", []):
            v = crates_latest(crate)
            if v == "UNPUBLISHED":
                note(f"R4 {crate}: 未发布 crates.io（跳过落后巡检）")
            elif v:
                latest[crate] = v
    for key, c in ecomap.get("consume", {}).items():
        for dep in c.get("deps", []):
            if dep["mode"] != "version":
                continue
            pkg, pinned = dep["package"], str(dep["pinned"])
            if pkg not in latest:
                continue
            lv = latest[pkg]
            # 完整三段（如 0.5.2）= 精确锁，全比（0.x 阶段 patch 位同样跟踪）；
            # 两段前缀（如 0.8）= caret 语义，同 minor 内 cargo update 即可获取，只比 minor
            if re.fullmatch(r"\d+\.\d+", pinned):
                stale = minor_pair(pinned) < minor_pair(lv)
            else:
                stale = version_triple(pinned) < version_triple(lv)
            if stale:
                fail("R4", f"{c['repo']}: {pkg} 锁 {pinned} 落后发布面 {lv}"
                           f"（https://crates.io/crates/{pkg}/changelog）")


# ---------------------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description="EvoRule 生态依赖地图校验器")
    ap.add_argument("--check-latest", action="store_true", help="启用 R4 发布面落后巡检（需网络）")
    ap.add_argument("--json", action="store_true", help="JSON 输出（CI 消费）")
    args = ap.parse_args()

    ecomap = load_map()
    check_r1(ecomap)
    check_r2(ecomap)
    check_r3(ecomap)
    if args.check_latest:
        check_r4(ecomap)

    if args.json:
        print(json.dumps({
            "ok": not violations,
            "violations": [{"rule": r, "message": m} for r, m in violations],
            "notices": notices,
        }, ensure_ascii=False, indent=2))
    else:
        for r, m in violations:
            print(f"FAIL [{r}] {m}")
        for n in notices:
            print(f"NOTE     {n}")
        print(f"\n{'FAIL' if violations else 'PASS'}: "
              f"{len(violations)} 违规, {len(notices)} 提示"
              f"{'（R4 已启用）' if args.check_latest else ''}")

    sys.exit(1 if violations else 0)


if __name__ == "__main__":
    main()
