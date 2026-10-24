#!/usr/bin/env python3
"""evorule 审计链独立验证器（单文件 · 零信任自算）

供第三方离线验证 evorule 审计链 WAL 数据。本脚本不属于 evorule 生态构建体系：
不需要安装/运行 evorule 任何软件，不需要访问任何服务器，唯一依赖为 pip 包 blake3。

用法：
    python verify_audit_chain.py --self-test            # 先验本脚本实现（11 组跨语言黄金向量）
    python verify_audit_chain.py FILE.wal [FILE2.wal …] # 验证一个或多个 WAL 文件
    python verify_audit_chain.py --dir WAL_DIR          # 验证目录下全部 .wal 文件

退出码（与生态哨兵 scan-wal-dir 协议对齐）：
    0 = 全部验证通过    2 = 用法错误    3 = 链断/哈希失配    4 = 文件缺失    5 = 格式坏

验证内容与公式（与白皮书公式一一对应）：
    fact_hash   = BLAKE3( fact对象的紧凑JSON序列化的UTF-8字节 )        # 白皮书: fact_hash = blake3(stable_json_utf8)
    chain_step  = BLAKE3( (上一链哈希串 ++ 本条fact_hash串) 的UTF-8 )   # 白皮书: chain_hash = blake3(prev_chain_hash_hex ++ content_hash_hex)
    链首 prev 为字面量 "genesis"，逐条折叠至链尾。

判定口径（诚实边界，第三方请注意）：
    [本验证证明的] 每条事实的原文与 content_hash 一致（未被篡改）；链逐条链接一致（无删除/乱序/中间插行）；给出链尾指纹。
    [本验证不证明的] 整条链是否出自真实系统——自洽的链可以被整体伪造。链尾指纹应对照
    可信外部发布锚定（公告/时间戳锚）方可构成完整证据链。本脚本不内置信任根。

本脚本自身先经 --self-test 自证：11 组黄金向量覆盖全部事实变体、中文/换行边界值、
大整数边界，向量与 evorule 主仓 scripts/g5_golden_vectors.json 同源。
"""

import argparse
import json
import os
import sys

try:
    import blake3
except ImportError:
    print("错误: 缺少依赖 blake3，请先执行: pip install blake3", file=sys.stderr)
    sys.exit(2)

GENESIS = "genesis"

# ---------------------------------------------------------------------------
# 公式实现（本文件的核心，逐行对应白皮书公式；改这里 = 改哈希口径，self-test 会红）
# ---------------------------------------------------------------------------

def canonical_fact_json(fact: dict) -> str:
    """serde_json 紧凑形态复刻：无空格分隔符、非 ASCII 直出、保留字段序。"""
    return json.dumps(fact, separators=(",", ":"), ensure_ascii=False)


def fact_hash(fact: dict) -> str:
    """fact_hash = BLAKE3(stable_json_utf8)"""
    return blake3.blake3(canonical_fact_json(fact).encode("utf-8")).hexdigest()


def chain_step(prev_hash_hex: str, content_hash_hex: str) -> str:
    """chain_step = BLAKE3(prev_chain_hash_hex ++ content_hash_hex)（hex 串拼接后 UTF-8）"""
    return blake3.blake3((prev_hash_hex + content_hash_hex).encode("utf-8")).hexdigest()


# ---------------------------------------------------------------------------
# 跨语言黄金向量（与主仓 scripts/g5_golden_vectors.json 同源；勿手改，随 SSOT 同步）
# ---------------------------------------------------------------------------

GOLDEN_VECTORS = [
    {"name": "cmd_empty", "stable_json": {"id": 1, "instruction": {}, "type": "Command"}, "fact_hash": "56de9ce95d41896edf081f452b4ea5d3ad6508de3a0781a36fa5fbd13c586aa1", "chain_from_genesis": "a7d4cc10fdba7231ad28495030262821b92eb72b5f7e29ba3aa3497dfff7bfa4"},
    {"name": "payload", "stable_json": {"id": 2, "path": "test.path", "type": "PayloadUpdate", "value": "test_value"}, "fact_hash": "902f22869450a78851d5bf1396b64d4bc7be0666468a39d2dac53b7d176f8047", "chain_from_genesis": "9b08729a7c3d7a3e11309f8402c4078fa96c447b8a50a2af20ad4ed7599c475d"},
    {"name": "transition", "stable_json": {"cause": 1, "id": 3, "new_payload": {}, "new_queue": [], "type": "StateTransition"}, "fact_hash": "6c4a12ee89a692181a07ede80b0864f0774286427fce67156944f6f33290ddf7", "chain_from_genesis": "12c1202d35f236baa5e6f994a6d3c834a143f357bd60c54e5902c7fee706a9d7"},
    {"name": "io_req", "stable_json": {"cause": 3, "id": 4, "io_type": "http_get", "params": {}, "type": "IoRequest"}, "fact_hash": "4855b360063f11eeffd7b8e2bd5720c641451012a688ebec489b5f8f059058aa", "chain_from_genesis": "792cca884c1fe50a5643abd3cfdd95116e1aefef87c0569e847f05000436c0f2"},
    {"name": "io_resp", "stable_json": {"error": None, "id": 5, "request_id": 4, "result": "response", "type": "IoResponse"}, "fact_hash": "867c9f708b7469ffb25682f997792b8811f34ef61eb2ee4ba23964e1c20f7909", "chain_from_genesis": "3ec88f4fb86da87a13460d55bf41d037f12dca7b41905fff94972985a2a6ccae"},
    {"name": "stable", "stable_json": {"id": 6, "type": "Stable", "version": 0}, "fact_hash": "ac02d9e15a497b30c18ce5019a50aeced2d5d186a973ee3a0985ee728054a015", "chain_from_genesis": "95279cfc92c79e4c32ebafacef1d70f2159d4d1a65886ab8c449630b55fae7d5"},
    {"name": "error", "stable_json": {"id": 7, "message": "test error", "type": "Error"}, "fact_hash": "42ca4e53611616707514550f1d2ab8a89be5ef8bd1a5f6cbd07e016bdb931afb", "chain_from_genesis": "dabf50ada042c5f22d32c448b2920bfb523375a8ba501a05e001037b41eac64d"},
    {"name": "trace_no_attr", "stable_json": {"cause": 2, "id": 8, "rule_hits": [{"hit": False, "index": 0, "instr_type": "branch"}], "type": "TransitionTrace"}, "fact_hash": "1d58ba6140c005701c121bb18f9ab9d1ec770d586f45ec004ef995f5efec7472", "chain_from_genesis": "cab31a98f025f64a84e3535e81a3509c5e5aaa6172a45b3e7cf1e83c160174c1"},
    {"name": "trace_attr", "stable_json": {"cause": 2, "id": 9, "rule_hits": [{"domain_attr": {"missing_reason": "path_not_found", "on_missing": "unsat", "outcome": "missing"}, "hit": False, "index": 0, "instr_type": "branch"}], "type": "TransitionTrace"}, "fact_hash": "7e3a55427674480aaecf0da2e2658c776ffbc2c4791ecb2d429db203882f0820", "chain_from_genesis": "a056f0bf4f2607279469912e5e56d3f34c64e23006f4299b1ec2bea7d49f3990"},
    {"name": "cjk_newline", "stable_json": {"id": 100, "message": "中文消息\n换行", "type": "Error"}, "fact_hash": "a4592a6ea2d2f34e4bdd69e84a0db4d6b42afaa7ec66094eeb828b8c8fd7e2a6", "chain_from_genesis": "4fa3fffc9e2f745e4cd9b9336a080474c6c4429f97150fd32a4b9fd3bd44831b"},
    {"name": "bigint_max", "stable_json": {"id": 101, "path": "big", "type": "PayloadUpdate", "value": 9223372036854775807}, "fact_hash": "3fbe8909b491123be07f764a164c90e0718e58a1ed2c2ccdd3d17eb38e9af2b0", "chain_from_genesis": "61b03fe5677e45b83a4c160dce99f0b3942be1426692a709aae77b92324fcd81"},
]


def self_test() -> int:
    """先证尺子：11 组向量逐一重算 fact_hash 与 genesis 折叠链。任一失配即退出码 3。"""
    ok = 0
    for v in GOLDEN_VECTORS:
        fh = fact_hash(v["stable_json"])
        ch = chain_step(GENESIS, v["fact_hash"])
        if fh != v["fact_hash"] or ch != v["chain_from_genesis"]:
            print(f"[SELF-TEST] FAIL {v['name']}")
            if fh != v["fact_hash"]:
                print(f"    fact_hash  重算={fh} 期望={v['fact_hash']}")
            if ch != v["chain_from_genesis"]:
                print(f"    chain      重算={ch} 期望={v['chain_from_genesis']}")
        else:
            ok += 1
            print(f"[SELF-TEST] PASS {v['name']}")
    print(f"[SELF-TEST] {ok}/{len(GOLDEN_VECTORS)} 向量通过")
    return 0 if ok == len(GOLDEN_VECTORS) else 3


# ---------------------------------------------------------------------------
# WAL 文件验证
# ---------------------------------------------------------------------------

def verify_wal(path: str):
    """逐行验证。返回 (exit_code, n_records, tail_hash_or_reason)。"""
    if not os.path.isfile(path):
        return 4, 0, f"文件缺失: {path}"
    prev = GENESIS
    n = 0
    tail = GENESIS
    try:
        with open(path, "r", encoding="utf-8") as f:
            for lineno, raw in enumerate(f, 1):
                line = raw.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                except json.JSONDecodeError as e:
                    return 5, n, f"第 {lineno} 行 JSON 解析失败: {e}"
                try:
                    stored_ch = rec["content_hash"]
                    stored_chain = rec["chain_hash"]
                    rec_prev = rec["prev_hash"]
                    fact = rec["fact"]
                except KeyError as e:
                    return 5, n, f"第 {lineno} 行缺字段: {e}"
                # ① 链接完整性：本行声明的 prev 必须等于上一行链尾
                if rec_prev != prev:
                    return 3, n, f"第 {lineno} 行链接断裂: prev_hash={rec_prev[:16]}… 期望={prev[:16]}…"
                # ② 原文完整性：从原文重算 content_hash（白皮书公式 ①）
                rh = fact_hash(fact)
                if rh != stored_ch:
                    return 3, n, f"第 {lineno} 行 content_hash 失配: stored={stored_ch[:16]}… recomputed={rh[:16]}…"
                # ③ 链折叠完整性：从 prev 与 content_hash 重算链哈希（白皮书公式 ②）
                rc = chain_step(rec_prev, stored_ch)
                if rc != stored_chain:
                    return 3, n, f"第 {lineno} 行 chain_hash 失配: stored={stored_chain[:16]}… recomputed={rc[:16]}…"
                prev = stored_chain
                tail = stored_chain
                n += 1
    except (OSError, UnicodeDecodeError) as e:
        # 归类对齐哨兵协议: 字节层读取/解码失败 → 码 3（哨兵同场景 "WAL I/O error" 归 3）
        return 3, n, f"读取失败: {e}"
    return 0, n, tail


def main() -> int:
    ap = argparse.ArgumentParser(description="evorule 审计链独立验证器（单文件，零信任自算）")
    ap.add_argument("files", nargs="*", help="待验证的 .wal 文件")
    ap.add_argument("--self-test", action="store_true", help="内置黄金向量自检（先证尺子）")
    ap.add_argument("--dir", help="验证目录下全部 .wal 文件")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    paths = list(args.files)
    if args.dir:
        if not os.path.isdir(args.dir):
            print(f"错误: 目录不存在: {args.dir}", file=sys.stderr)
            return 4
        paths.extend(
            os.path.join(args.dir, f)
            for f in sorted(os.listdir(args.dir))
            if f.endswith(".wal")
        )
    if not paths:
        ap.print_help()
        return 2

    worst = 0
    for p in paths:
        code, n, detail = verify_wal(p)
        name = os.path.basename(p)
        if code == 0:
            print(f"[OK]   {name}: {n} records verified, 链尾指纹 {tail_str(detail)}")
        elif code == 4:
            print(f"[MISS] {name}: {detail}")
        elif code == 5:
            print(f"[BAD]  {name}: {detail}")
        else:
            print(f"[FAIL] {name}: {detail}")
        worst = max(worst, code)
    print("---")
    print("局限声明: 本验证证明链自洽且原文无损; 整链伪造需对照外部发布锚定方可排除。")
    return worst


def tail_str(h: str) -> str:
    return f"{h[:8]}..{h[-8:]}" if len(h) >= 16 else h


if __name__ == "__main__":
    sys.exit(main())
