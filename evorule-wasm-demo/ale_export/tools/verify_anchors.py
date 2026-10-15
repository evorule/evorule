# -*- coding: utf-8 -*-
"""verify_anchors: ALE run-record 独立验签 CLI（B-4，J-B4 验证闭合）。

接口（B-4 批次定义）：
    verify_anchors.py <run_record.json> [--pubkey <pk_hex>]

验证两项：
  ①run-record 内部自洽（机械复算，同 verify.html 判定层复算语义）：
    - score_bps == score_max iff server_status==passed（判分口径复算，
      failed→0 同样复算——错误路径也要可复算，J-B4 子款）；
    - match = |bps/score_max - official| <= 1e-3 重算；
    - verdict 汇总复算（all(match) → ALL_MATCH_OFFICIAL*，+IMPORTED 若
      governance.import 存在且 status=imported）；
    - anchors 与 sandboxes 的 tcb_session_id 一一对应；
  ②anchor 链串接验签（evorule-anchor/2）：
    - seq 连续（0 起严格 +1）；
    - 区间无缝（首锚 lo=0，相邻 prev.hi == next.lo）；
    - v2 载荷串接 ed25519 验签：
      evorule-anchor/2|{session_id}|{seq}|{lo}|{hi}|{chain_head}|{key_id}|{engine_id}|{logical_time}
      （与 governance signature_payload_v2 同构：'|' 分隔，字段序=声明序；
      签名 64 字节 128 hex）。

诚实边界（与 A 线 evorule-cli verify-anchors 同口径）：
  - blake3 anchor_hash 链重算不进本 CLI（零 blake3 依赖），由 evorule
    verify 端点背书——本 CLI 完成防抵赖面（ed25519 来源真实性）；
  - ed25519 为纯 Python RFC 8032 实现（仅 hashlib），CLI 零第三方依赖。

公钥分发：--pubkey <pk_hex>（64 位 hex，0.8.3 密钥 CLI anchor-pubkey
派生物）；多 key_id 链可用 --key <key_id>=<pk_hex> 重复传入。
未提供且 run-record 无内嵌 verifying_key 时验签面显式失败（不静默跳过）。

退出码：0=全部通过；1=任一校验失败或输入非法。
"""
from __future__ import annotations

import hashlib
import json
import sys

# ---------------------------------------------------------------------------
# 纯 Python ed25519（RFC 8032 验签侧；仅 hashlib，extended coordinates）
# ---------------------------------------------------------------------------

_P = 2 ** 255 - 19
_Q = 2 ** 252 + 27742317777372353535851937790883648493


def _inv(x: int) -> int:
    return pow(x, _P - 2, _P)


_D = -121665 * _inv(121666) % _P
_I = pow(2, (_P - 1) // 4, _P)


def _xrecover(y: int) -> int:
    xx = (y * y - 1) * _inv(_D * y * y + 1)
    x = pow(xx, (_P + 3) // 8, _P)
    if (x * x - xx) % _P != 0:
        x = (x * _I) % _P
    if x % 2 != 0:
        x = _P - x
    return x


_By = 4 * _inv(5) % _P
_Bx = _xrecover(_By)
_B = (_Bx % _P, _By % _P, 1, (_Bx * _By) % _P)
_IDENT = (0, 1, 1, 0)


def _edwards_add(P, Q):
    (x1, y1, z1, t1), (x2, y2, z2, t2) = P, Q
    a = (y1 - x1) * (y2 - x2) % _P
    b = (y1 + x1) * (y2 + x2) % _P
    c = t1 * 2 * _D * t2 % _P
    dd = z1 * 2 * z2 % _P
    e = b - a
    f = dd - c
    g = dd + c
    h = b + a
    return (e * f % _P, g * h % _P, f * g % _P, e * h % _P)


def _point_mul(s: int, P):
    Q = _IDENT
    while s > 0:
        if s & 1:
            Q = _edwards_add(Q, P)
        P = _edwards_add(P, P)
        s >>= 1
    return Q


def _point_compress(P) -> bytes:
    (x, y, z, _t) = P
    zi = _inv(z)
    x = x * zi % _P
    y = y * zi % _P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _point_decompress(s: bytes):
    if len(s) != 32:
        raise ValueError("点编码长度须为 32 字节")
    y = int.from_bytes(s, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _xrecover(y)
    if x & 1 != sign:
        x = _P - x
    if (-x * x + y * y - 1 - _D * x * x * y * y) % _P != 0:
        raise ValueError("点不在曲线上")
    return (x, y, 1, (x * y) % _P)


def ed25519_verify(pubkey32: bytes, sig64: bytes, msg: bytes) -> bool:
    """RFC 8032 验签（cofactorless，与 ed25519 crate 默认 verify 语义一致）。"""
    if len(sig64) != 64:
        return False
    try:
        A = _point_decompress(pubkey32)
        R = _point_decompress(sig64[:32])
    except ValueError:
        return False
    S = int.from_bytes(sig64[32:], "little")
    if S >= _Q:
        return False
    h = int.from_bytes(
        hashlib.sha512(sig64[:32] + pubkey32 + msg).digest(), "little"
    ) % _Q
    sB = _point_mul(S, _B)
    hA = _point_mul(h, A)
    return _point_compress(sB) == _point_compress(_edwards_add(R, hA))


# ---------------------------------------------------------------------------
# evorule-anchor/2 锚链验签（与 governance anchor.rs 同构）
# ---------------------------------------------------------------------------

ANCHOR_FORMAT = "evorule-anchor/2"
ANCHOR_GENESIS = "genesis"


def signature_payload_v2(a: dict) -> str:
    """v2 载荷串接：'|' 分隔，字段序=governance 声明序。"""
    return "|".join([
        ANCHOR_FORMAT,
        str(a["session_id"]),
        str(a["seq"]),
        str(a["fact_range"]["lo"]),
        str(a["fact_range"]["hi"]),
        str(a["chain_head"]),
        str(a["key_id"]),
        str(a["engine_id"]),
        str(a["logical_time"]),
    ])


def verify_anchor_chain(anchors: list, keys: dict, report) -> bool:
    """evorule-anchor/2 三步验签；明细写入 report 列表。返回整体通过。"""
    ok_all = True
    prev_hi = None
    for i, a in enumerate(anchors):
        tag = f"anchor#{i}(seq={a.get('seq')})"
        # 1. 序号连续（0 起严格 +1）
        if a.get("seq") != i:
            report.append(f"[FAIL] {tag} 序号不连续: 期望 {i} 实际 {a.get('seq')}")
            ok_all = False
            continue
        # 2. 区间无缝
        if a.get("anchor_format") != ANCHOR_FORMAT:
            report.append(f"[FAIL] {tag} anchor_format={a.get('anchor_format')!r} 非 {ANCHOR_FORMAT}（不静默跨格式验过）")
            ok_all = False
            continue
        lo = a["fact_range"]["lo"]
        hi = a["fact_range"]["hi"]
        if prev_hi is None:
            if lo != 0:
                report.append(f"[FAIL] {tag} 首锚 lo={lo} != 0（前缀截断）")
                ok_all = False
                continue
        elif lo != prev_hi:
            report.append(f"[FAIL] {tag} 区间断裂: prev.hi={prev_hi} != lo={lo}")
            ok_all = False
            continue
        # 3. v2 载荷串接 ed25519 验签
        pk_hex = keys.get(str(a.get("key_id")))
        if pk_hex is None:
            report.append(f"[FAIL] {tag} key_id={a.get('key_id')!r} 无对应公钥（--pubkey/--key 提供或内嵌 verifying_key）")
            ok_all = False
            continue
        try:
            pk = bytes.fromhex(pk_hex)
        except ValueError:
            report.append(f"[FAIL] {tag} 公钥非合法 hex")
            ok_all = False
            continue
        if len(pk) != 32:
            report.append(f"[FAIL] {tag} 公钥长度 != 32 字节")
            ok_all = False
            continue
        try:
            sig = bytes.fromhex(a["signature"])
        except (KeyError, ValueError):
            report.append(f"[FAIL] {tag} 签名缺失或非合法 hex")
            ok_all = False
            continue
        payload = signature_payload_v2(a).encode("utf-8")
        if ed25519_verify(pk, sig, payload):
            report.append(
                f"[OK] {tag} range=[{lo},{hi}) key={a.get('key_id')} "
                f"engine={a.get('engine_id')} 签名有效"
            )
        else:
            report.append(f"[FAIL] {tag} 签名校验失败（被篡改或非本公钥签名）")
            ok_all = False
        prev_hi = hi
    return ok_all


# ---------------------------------------------------------------------------
# run-record 内部自洽复算（①项；同 verify.html 判定层复算语义）
# ---------------------------------------------------------------------------

TOLERANCE = 1e-3


def verify_self_consistency(rr: dict, report) -> bool:
    """run-record 机械复算：判分口径/match/verdict/anchors 对应性。

    错误路径同可复算（J-B4 子款）：failed→0 与 passed→score_max 同口径复算。
    """
    ok_all = True
    evidence = rr.get("evidence", {})
    # score_max：run-record 顶层显式携带（grade_via_server v0.3+）；
    # 旧形态缺省 10000（ale-run-v1 v1 万分制定点约定）。
    score_max = int(rr.get("score_max", 10000))

    for s in rr.get("scores", []):
        name = s.get("variant")
        bps = s.get("score_bps")
        status = s.get("server_status")
        expect = float(s["official_score"])
        # 判分口径复算（passed→score_max / failed→0）
        expect_bps = score_max if status == "passed" else 0
        if bps != expect_bps:
            report.append(
                f"[FAIL] variant={name} 判分口径不符: server_status={status} "
                f"应得 {expect_bps}bps 实得 {bps}bps"
            )
            ok_all = False
        # match 复算
        expect_match = abs(bps / score_max - expect) <= TOLERANCE
        if bool(s.get("match")) != expect_match:
            report.append(
                f"[FAIL] variant={name} match 复算不符: 应为 {expect_match} "
                f"记录为 {s.get('match')}"
            )
            ok_all = False
        else:
            report.append(
                f"[OK] variant={name} status={status} bps={bps} "
                f"official={expect} match={expect_match}（复算一致）"
            )
    # verdict 汇总复算
    all_match = all(bool(s.get("match")) for s in rr.get("scores", [])) \
        if rr.get("scores") else False
    imported = (rr.get("evidence", {}).get("governance", {}) or {}).get("import", {}) \
        .get("status") == "imported"
    expect_verdict = ("ALL_MATCH_OFFICIAL" + ("+IMPORTED" if imported else "")) \
        if all_match else "MISMATCH"
    if rr.get("verdict") != expect_verdict:
        report.append(
            f"[FAIL] verdict 汇总复算不符: 应为 {expect_verdict} "
            f"记录为 {rr.get('verdict')}"
        )
        ok_all = False
    else:
        report.append(f"[OK] verdict={rr.get('verdict')}（汇总复算一致）")
    # anchors 与 sandboxes 对应性
    sandboxes = evidence.get("server", {}).get("sandboxes", [])
    anchor_blocks = rr.get("anchors", [])
    if len(anchor_blocks) != len(sandboxes):
        report.append(
            f"[FAIL] anchors 块数={len(anchor_blocks)} 与 sandboxes 数="
            f"{len(sandboxes)} 不一致"
        )
        ok_all = False
    else:
        for blk, sb in zip(anchor_blocks, sandboxes):
            if blk.get("tcb_session_id") != sb.get("tcb_session_id"):
                report.append(
                    f"[FAIL] anchors 块 session={blk.get('tcb_session_id')} 与 "
                    f"sandbox variant={sb.get('variant')} session="
                    f"{sb.get('tcb_session_id')} 错位"
                )
                ok_all = False
    return ok_all


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> int:
    args = sys.argv[1:]
    if not args or args[0] in ("-h", "--help"):
        print(__doc__)
        return 0 if args else 1
    path = args[0]
    pubkey = None
    keys: dict = {}
    i = 1
    while i < len(args):
        if args[i] == "--pubkey":
            pubkey = args[i + 1]
            i += 2
        elif args[i] == "--key":
            k, _, v = args[i + 1].partition("=")
            keys[k] = v
            i += 2
        else:
            print(f"[FAIL] 未知参数: {args[i]}")
            return 1

    try:
        rr = json.loads(open(path, encoding="utf-8").read())
    except (OSError, json.JSONDecodeError) as e:
        print(f"[FAIL] run-record 读取/解析失败: {e}")
        return 1

    print(f"=== Verify Anchors: {path} ===")
    print(f"Algorithm: ed25519 (RFC 8032) pure-python verify; "
          f"anchor format: {ANCHOR_FORMAT}")
    report: list = []

    # ①内部自洽
    print("--- ① run-record 内部自洽复算 ---")
    ok_self = verify_self_consistency(rr, report)
    for line in report:
        print(line)

    # ②锚链验签——**逐块独立验**：每块=一个独立 TCB session 的锚链
    # （per-variant isolated sandbox），各块 seq 各自从 0 起、区间各自
    # 从 lo=0 起；跨块平铺验签是口径错误。
    blocks = rr.get("anchors", [])
    flat_count = sum(len(blk.get("anchors", [])) for blk in blocks)
    print(f"--- ② anchor 链串接验签（{len(blocks)} 块 / {flat_count} 锚点，"
          f"逐 session 独立验链）---")
    # 公钥优先级：--pubkey/--key > run-record 内嵌 verifying_key
    keymap = dict(keys)
    for blk in blocks:
        vk = blk.get("verifying_key")
        if vk:
            default_key = str((blk.get("anchors") or [{}])[0].get("key_id", "anchor-key-1"))
            keymap.setdefault(default_key, vk)
    if pubkey:
        default_key = "anchor-key-1"
        for blk in blocks:
            if blk.get("anchors"):
                default_key = str(blk["anchors"][0].get("key_id", default_key))
                break
        keymap.setdefault(default_key, pubkey)

    if flat_count == 0:
        print("[WARN] 无审计锚点——仅有内部自洽，无来源真实性证据")
        ok_chain = True
    else:
        ok_chain = True
        for blk in blocks:
            chain = blk.get("anchors", [])
            print(f"[block] variant={blk.get('variant')} "
                  f"session={blk.get('tcb_session_id')} 锚点数={len(chain)}")
            if not chain:
                print("[WARN] 本块无锚点（无来源真实性证据）")
                continue
            rep2: list = []
            ok_b = verify_anchor_chain(chain, keymap, rep2)
            for line in rep2:
                print(line)
            ok_chain &= ok_b

    ok = ok_self and ok_chain
    print(f"=== 结果: {'PASS' if ok else 'FAIL'}"
          f"（自洽={'OK' if ok_self else 'FAIL'} 锚链={'OK' if ok_chain else 'FAIL'}）===")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
