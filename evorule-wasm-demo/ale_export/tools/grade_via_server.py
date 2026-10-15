#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""grade_via_server —— B-3 收尾交付物（ALE B 线批次表 B-3）

调用侧判卷 harness，流序对齐导入闸门一立法语义（「测试工作台跑完直接带入」）：

  Phase A  沙盒判卷（evorule-server）：bundle 内 grader_ruleset + grader_test
           variants → workspace→rule→test-datasets→sandbox→report；
           逐变体与官方 expect 对照（J-B1「同官方分」核心判据）。
  Phase B  机器背书组装：沙盒通过后 tests.verdict=pass +
           subset=["sandbox:<sandbox_id>", ...]（O-431 五闸 sandbox:-only 形态；
           rule 侧验形状，执行域深度校验 sandbox 存在性），content_hash 重算
           （复用 ale2bundle.dataset_bundle_hash 的 serde 双轨实现）。
  Phase C  治理导入（rule serve）：login→dry-run→import（201）。

判卷链形态继承 SR-14 件5 实测活体（b3_sandbox_path.py）：
  判卷 ruleset 走 workspace rule content；用例走 test-datasets cases_json；
  TCB 判定 + audit 链；mismatch 归因 = push eval_assert_failed 断言错误形态
  （M8-F1 候选 B，server Ignored→Error→组内 Failed）。

判分口径（诚实边界）：
  server report 按组态判分：passed→score_max（万分制 10000）；
  failed→0；与官方 expect 对照 |bps/10000 - expect| ≤ 1e-3
  （万分制定点 vs 官方 round6 浮点，判分口径继承纪律，显式声明容差）。

用法：
  python grade_via_server.py --bundle ../b_bundles/eval-task-demo-hello-v1 \
      [--out run-record.json] [--rule-base http://127.0.0.1:18086] \
      [--server-base http://127.0.0.1:18080] [--username admin] \
      [--password PW] [--tenant-id default] [--bootstrap]

退出码：全部变体同官方分且导入 201 → 0；否则 1。
"""
import argparse
import datetime
import json
import os
import sys
import time
import urllib.error
import urllib.request

# 同目录复用 ale2bundle 的 content_hash 复算（serde 双轨：顶层声明序+rule_body 字母序）
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ale2bundle import dataset_bundle_hash  # noqa: E402

TOLERANCE = 1e-3  # 万分制 vs 官方浮点对照容差（判分口径继承纪律，显式声明）


def http_json(method, url, body=None, token=None, timeout=20):
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        raw = e.read().decode(errors="replace")
        try:
            return e.code, json.loads(raw or "{}")
        except json.JSONDecodeError:
            return e.code, {"_raw": raw[:400]}


def load_bundle(bundle_arg):
    """输入= bundle 目录或 bundle.json 路径；返回 (bundle dict, bundle.json 绝对路径)。"""
    p = os.path.abspath(bundle_arg)
    if os.path.isdir(p):
        p = os.path.join(p, "bundle.json")
    with open(p, encoding="utf-8") as f:
        bundle = json.load(f)
    return bundle, p


def extract_entries(bundle):
    """从 bundle entries 提取判卷三元组：ruleset 载荷 / tests 载荷 / score_max。
    schema_ref 按 domain 命名域 URI（SR-17 B2：/schemas/domain/ale/eval-*/v1.json）。"""
    ruleset_body = test_body = None
    for e in bundle.get("entries", []):
        rb = e.get("rule_body") or {}
        ref = e.get("schema_ref", "")
        if ref.endswith("/eval-ruleset/v1.json"):
            ruleset_body = rb
        elif ref.endswith("/eval-test/v1.json"):
            test_body = rb
    if not ruleset_body or "ruleset" not in ruleset_body:
        raise SystemExit("bundle 缺 grader_ruleset entry（schema_ref */eval-ruleset/v1.json）")
    if not test_body or "variants" not in test_body:
        raise SystemExit("bundle 缺 grader_test entry（schema_ref */eval-test/v1.json）")
    return ruleset_body, test_body


def sandbox_grade(server_base, stamp, ruleset, variants, grade_type):
    """Phase A：server 沙盒判卷（SR-14 件5 活体链）——**逐变体独立判卷**：
    每变体单独 dataset+sandbox，归组零歧义。多 case 同沙盒时 push 断言的
    Error 归组存在时序竞态（中间 case 的 push 会被后续 case 的 Command
    抢位，SR-14 两用例形态未暴露），变体独立判卷=判分 harness 的正确形态。
    返回 (per-variant reports, evidence dict)。"""
    SB = server_base
    st, ws = http_json("POST", f"{SB}/api/workspaces", {
        "name": f"b3-grade-{stamp}", "owner_id": "ale-b3",
        "description": f"B-3 grade_via_server sandbox phase"})
    assert st in (200, 201), f"workspace fail {st}: {ws}"
    wid = ws["id"] if "id" in ws else ws.get("workspace", {}).get("id")
    http_json("POST", f"{SB}/api/workspaces/{wid}/members/join",
              {"user_id": "ale-b3"})

    st, rule = http_json("POST", f"{SB}/api/workspaces/{wid}/rules", {
        "name": f"grader-{stamp}",
        "content": json.dumps(ruleset, ensure_ascii=False),
        "created_by": "ale-b3",
        "description": "B-3 grader from bundle (grade_via_server)"})
    assert st in (200, 201), f"rule fail {st}: {rule}"
    rid, vid = rule["id"], rule.get("current_version_id")
    assert vid, f"no current_version_id: {rule}"

    reports, sandboxes, anchor_blocks = [], [], []
    for v in variants:
        st, ds = http_json("POST", f"{SB}/api/workspaces/{wid}/test-datasets", {
            "name": f"b3-case-{stamp}-{v['name']}",
            "cases_json": json.dumps(
                [{"type": grade_type, "name": v["name"], "facts": v["facts"]}],
                ensure_ascii=False),
            "created_by": "ale-b3", "workspace_id": wid})
        assert st in (200, 201), f"dataset fail {st}: {ds}"
        did = ds["id"]

        st, sb = http_json("POST", f"{SB}/api/workspaces/{wid}/sandboxes", {
            "rule_version_ids": [vid], "test_dataset_id": did,
            "started_by": "ale-b3"})
        assert st in (200, 201), f"sandbox fail {st}: {sb}"
        sid = sb["sandbox_id"]

        report, last_st = None, 0
        for _ in range(30):
            st, rep = http_json(
                "GET", f"{SB}/api/workspaces/{wid}/sandboxes/{sid}/report")
            last_st = st
            if st == 200:
                report = rep
                if rep.get("summary", {}).get("total_cases", 0) >= 1:
                    break
            else:
                http_json("POST", f"{SB}/api/workspaces/{wid}/sandboxes/{sid}/close",
                          {"closed_by": "ale-b3"})
                st2, rep2 = http_json(
                    "GET", f"{SB}/api/workspaces/{wid}/sandboxes/{sid}/report")
                if st2 == 200:
                    report, last_st = rep2, st2
                    break
            time.sleep(1)
        assert report, f"report unavailable for {v['name']} (last status {last_st})"

        # ---- 锚点封条+导出（B-4：G-A2 evorule-anchor/2）----
        # 手动 seal（判卷 session 事实数少，INTERVAL 自动封不触发）；
        # 409=锚点链未启用（EVORULE_ANCHOR_SEED 未配置）——显式失败不静默
        # 降级（run-record 无锚点则 J-B4 验签面空转）。
        tcb_sid = sb.get("tcb_session_id")
        st_a, sealed = http_json(
            "POST", f"{SB}/api/sessions/{tcb_sid}/anchors/seal", {})
        if st_a != 200:
            raise SystemExit(
                f"[b4] anchor seal fail {st_a}: {sealed}——server 须以 "
                f"EVORULE_ANCHOR_SEED 启动（锚点链启用）")
        st_a, exp = http_json("GET", f"{SB}/api/sessions/{tcb_sid}/anchors")
        assert st_a == 200 and exp.get("count", 0) >= 1, \
            f"anchors export fail {st_a}: {exp}"

        reports.append((v["name"], report))
        sandboxes.append({
            "variant": v["name"], "dataset_id": did, "sandbox_id": sid,
            "tcb_session_id": tcb_sid,
            "draft_ruleset_hash": sb.get("draft_ruleset_hash")})
        anchor_blocks.append({
            "variant": v["name"], "tcb_session_id": tcb_sid,
            "count": exp.get("count"), "anchors": exp.get("anchors", [])})

    evidence = {
        "server_base": SB,
        "workspace_id": wid, "rule_id": rid, "rule_version_id": vid,
        "mode": "per-variant-isolated-sandbox",
        "audit_chain_verified": all(
            (r.get("audit_info") or {}).get("audit_chain_verified")
            for _, r in reports),
        "sandboxes": sandboxes,
        "report_summaries": [{"variant": n, "summary": r.get("summary")}
                             for n, r in reports],
        "report_cases": [
            {"variant": n,
             "cases": [{k: c.get(k) for k in ("case_name", "status",
                                              "error_message")}
                       for c in r.get("cases", [])]}
            for n, r in reports],
        "anchors": anchor_blocks,
    }
    return reports, evidence


def main():
    ap = argparse.ArgumentParser(description="ALE B-3 server 判卷链 harness")
    ap.add_argument("--bundle", required=True, help="bundle 目录或 bundle.json 路径")
    ap.add_argument("--out", default=None, help="run-record 输出路径（缺省 stdout）")
    ap.add_argument("--rule-base", default="http://127.0.0.1:18086")
    ap.add_argument("--server-base", default="http://127.0.0.1:18080")
    ap.add_argument("--username", default="admin")
    ap.add_argument("--password", default=None, help="缺省读 RULE_ADMIN_PASSWORD 环境变量")
    ap.add_argument("--tenant-id", default=None, help="缺省 RULE_TENANT_ID 或 default")
    ap.add_argument("--agent-ref", default="ale-demo-agent")
    ap.add_argument("--keep-prefix", default=None, help="资源名前缀（缺省自动时间戳）")
    ap.add_argument("--bootstrap", action="store_true",
                    help="测试区新库：先 register admin 再 login")
    ap.add_argument("--skip-import", action="store_true",
                    help="只跑 Phase A 沙盒判卷，不组装背书不导入")
    args = ap.parse_args()

    password = args.password or os.environ.get("RULE_ADMIN_PASSWORD")
    tenant_id = args.tenant_id or os.environ.get("RULE_TENANT_ID", "default")
    # 凭据归边：本 harness 不内置任何凭证文件路径——凭据只经
    # --password / RULE_ADMIN_PASSWORD 传入（调用方自行负责提供）。
    assert password, "admin password 未提供（--password 或 RULE_ADMIN_PASSWORD）"

    stamp = args.keep_prefix or datetime.datetime.now().strftime("%H%M%S")
    bundle, bundle_path = load_bundle(args.bundle)
    bundle_id = bundle.get("bundle_id", "?")
    content_hash = (bundle.get("audit") or {}).get("content_hash", "")
    ruleset_body, test_body = extract_entries(bundle)
    score_max = int(ruleset_body.get("score_max", 10000))
    variants = test_body["variants"]
    grade_type = (test_body.get("grade_command") or {}).get("type")
    if not grade_type:
        r0 = ruleset_body["ruleset"][0]["params"]["domain"]
        grade_type = r0.get("instruction_type", "eval_exact_match")
    print(f"[b] bundle={bundle_id} variants={len(variants)} "
          f"grade_type={grade_type} score_max={score_max}")

    # ---- Phase A：server 沙盒判卷（逐变体独立沙盒）----
    reports, ev = sandbox_grade(args.server_base, stamp,
                                ruleset_body["ruleset"], variants, grade_type)
    status_of = {}
    for name, rep in reports:
        cs = rep.get("cases", [])
        c0 = cs[0] if cs else {}
        status_of[name] = c0.get("status", "missing")

    scores, all_match = [], True
    for v in variants:
        expect = float(v["expect"][0])
        status = status_of.get(v["name"], "missing")
        bps = score_max if status == "passed" else 0
        match = abs(bps / score_max - expect) <= TOLERANCE
        all_match &= match
        scores.append({
            "variant": v["name"], "expect": v["expect"],
            "server_status": status,
            "server_error": next(
                (c.get("error_message")
                 for n, r in reports if n == v["name"]
                 for c in r.get("cases", [])
                 if c.get("case_name") == v["name"]),
                None),
            "score_bps": bps, "official_score": expect, "match": match})

    print("[r] per-variant:")
    for s in scores:
        print(f"    {s['variant']:<16} server={s['server_status']:<8} "
              f"bps={s['score_bps']:<6} official={s['official_score']} "
              f"match={s['match']}")
    print(f"[r] audit_chain_verified={ev['audit_chain_verified']}")
    print(f"[r] 同官方分（J-B1 核心判据）: {'PASS' if all_match else 'FAIL'}")

    governance = None
    if not args.skip_import:
        # ---- Phase B：机器背书组装（verdict=pass + sandbox:<id>）----
        assert all_match, "判卷未全对官方分，不为 import 组装 pass 背书（不伪造证据）"
        endorsed = json.loads(json.dumps(bundle))
        # BundleTests 声明序 = subset → fixtures → verdict（evorule-bundle 0.5.2）：
        # 服务端防篡改走 serde 往返重序列化（struct 声明序），背书 dict 须按
        # 同一序构造，否则 content_hash 字节序错位（fixtures 空 → skip 不出键）。
        endorsed["tests"] = {
            "subset": [f"sandbox:{sid['sandbox_id']}" for sid in ev["sandboxes"]],
            "verdict": "pass",
        }
        endorsed["audit"]["content_hash"] = ""
        endorsed["audit"]["content_hash"] = dataset_bundle_hash(endorsed)
        print(f"[e] 背书组装: verdict=pass subset={[s[:24]+'…' for s in endorsed['tests']['subset']]} "
              f"content_hash 重算={endorsed['audit']['content_hash'][:20]}…")

        # ---- Phase C：治理导入（rule serve）----
        st, login = http_json("POST", f"{args.rule_base}/v1/auth/login",
                              {"username": args.username, "password": password,
                               "tenant_id": tenant_id})
        assert st == 200, f"rule login fail {st}: {login}"
        token = login.get("access_token") or login.get("token")
        assert token, f"no token in login response: {list(login)}"

        st, dry = http_json("POST", f"{args.rule_base}/v1/bundles/import/dry-run",
                            {"bundle": endorsed}, token=token)
        assert st == 200 and dry.get("valid"), f"dry-run fail {st}: {dry}"
        print(f"[g] dry-run valid=true verdict={dry.get('verdict')} "
              f"entries={dry.get('entry_count')}")

        st, imp = http_json("POST", f"{args.rule_base}/v1/bundles/import",
                            {"bundle": endorsed}, token=token)
        assert st == 201, f"import fail {st}: {imp}"
        print(f"[g] import 201 bundle={imp.get('bundle_id')} "
              f"dataset={imp.get('dataset_id')} v={imp.get('activated_version')}")
        governance = {"rule_base": args.rule_base, "dry_run": dry, "import": imp,
                      "endorsement": {"verdict": "pass",
                                      "subset": endorsed["tests"]["subset"],
                                      "note": "sandbox:<id> 机器背书=Phase A 沙盒判卷真实产物；content_hash 按闸面前置重算"}}

    run_record = {
        "schema_ref": "ale-run-v1",
        "grade_path": "server-sandbox",
        "task_bundle_ref": bundle_id,
        "content_hash": content_hash,
        "agent_ref": args.agent_ref,
        "score_max": score_max,
        "scores": scores,
        "anchors": ev.get("anchors", []),
        "evidence": {
            "server": ev,
            "governance": governance,
            "scoring_rule": (
                f"passed→{score_max} bps；failed→0；"
                f"|bps/{score_max}-expect|≤{TOLERANCE}（万分制定点 vs 官方浮点，"
                "判分口径继承纪律容差显式声明）"),
        },
        "verdict": ("ALL_MATCH_OFFICIAL" if all_match else "MISMATCH")
                   + ("" if args.skip_import or governance else "（未导入）")
                   + ("+IMPORTED" if governance else ""),
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "generator": "grade_via_server v0.3 (ALE B-3/B-4)",
    }

    ok = all_match and (args.skip_import or governance is not None)
    print(f"[r] J-B1 收口判据（判卷同官方分+治理导入闭合）: "
          f"{'PASS' if ok else 'FAIL'}")

    if args.out:
        out = os.path.abspath(args.out)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        with open(out, "w", encoding="utf-8") as f:
            json.dump(run_record, f, ensure_ascii=False, indent=2)
        print(f"[r] run-record → {out}")
    else:
        print(json.dumps(run_record, ensure_ascii=False, indent=2))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
