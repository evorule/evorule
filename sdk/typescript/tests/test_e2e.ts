/**
 * evorule TypeScript SDK 端到端集成测试
 *
 * 覆盖所有 SDK 端点，验证与 evorule-server 的完整交互流程。
 *
 * 前置条件：
 *   evorule-server 已启动在 http://localhost:18080
 *
 * 运行：
 *   cd sdk/typescript
 *   npx tsx tests/test_e2e.ts
 */

import {
  EvoruleClient,
  Session,
  Event,
  CommandError,
  EvoruleError,
  SessionClosedError,
  type Json,
  type ApiResponse,
  type DiffResponse,
  type SharedFact,
  type ForkSessionResponse,
  type AuditVerifyResponse,
  type HistoryEntry,
  type UsedAtStartupResponse,
  type ClusterStatusResponse,
  type PendingIoInfo,
} from "../src/index.js";

const BASE_URL = process.env.EVORULE_BASE_URL || "http://localhost:18080";

interface TestResult {
  passed: number;
  failed: number;
  errors: string[];
}

function createTestResult(): TestResult {
  return { passed: 0, failed: 0, errors: [] };
}

function ok(tr: TestResult, name: string): void {
  tr.passed++;
  console.log(`  ✅ PASS: ${name}`);
}

function fail(tr: TestResult, name: string, reason: string): void {
  tr.failed++;
  tr.errors.push(`${name}: ${reason}`);
  console.log(`  ❌ FAIL: ${name} — ${reason}`);
}

function summary(tr: TestResult): boolean {
  console.log(`\n${"=".repeat(60)}`);
  console.log(`结果: ${tr.passed} passed, ${tr.failed} failed`);
  if (tr.errors.length > 0) {
    console.log("\n失败用例:");
    for (const e of tr.errors) {
      console.log(`  - ${e}`);
    }
  }
  console.log("=".repeat(60));
  return tr.failed === 0;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ===== 场景 1：会话生命周期 =====
async function test01SessionLifecycle(tr: TestResult): Promise<void> {
  console.log("\n[场景 1] 会话生命周期");
  const client = new EvoruleClient(BASE_URL);
  try {
    const before = await client.listSessions();
    ok(tr, `listSessions（当前 ${before.length} 个会话）`);

    const session = await client.createSession();
    if (session.sessionId <= 0) {
      fail(tr, "createSession", `sessionId 必须为正整数，得到 ${session.sessionId}`);
      return;
    }
    ok(tr, `createSession（id=${session.sessionId}）`);

    const after = await client.listSessions();
    if (!after.includes(session.sessionId)) {
      fail(tr, "listSessions", "新建会话未出现在列表中");
    } else {
      ok(tr, "listSessions（包含新会话）");
    }

    const state = await session.state();
    if (!("payload" in state) || !("queue" in state) || !("version" in state)) {
      fail(tr, "state", "缺少必要字段");
    } else {
      ok(tr, `state（version=${state.version}, payload keys=${Object.keys(state.payload).join(",") || "空"}）`);
    }

    await session.close();
    ok(tr, "close（主动关闭）");

    try {
      await session.state();
      fail(tr, "关闭后访问应抛错", "未抛异常");
    } catch (e) {
      if (e instanceof SessionClosedError) {
        ok(tr, "关闭后访问抛 SessionClosedError");
      } else {
        fail(tr, "关闭后访问应抛 SessionClosedError", `实际抛 ${e}`);
      }
    }
  } finally {
    await client.close();
  }
}

// ===== 场景 2：命令提交 + SSE 事件流 =====
async function test02CommandAndSse(tr: TestResult): Promise<void> {
  console.log("\n[场景 2] 命令提交 + SSE 事件流");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();

    const events: Event[] = [];
    let consumerStarted = false;

    const consumeEvents = async (): Promise<void> => {
      consumerStarted = true;
      for await (const ev of session.events()) {
        events.push(ev);
        if (ev.type === "Stable") break;
      }
    };

    const consumer = consumeEvents();
    while (!consumerStarted) {
      await sleep(50);
    }

    const setResp = await session.command({ type: "set", params: { attr: "x", value: 0 } });
    ok(tr, `command（set x=0, fact_id=${setResp.fact_id}）`);

    await sleep(500);

    const incResp = await session.command({
      type: "increment",
      params: { attr: "x", delta: 5 },
    });
    ok(tr, `command（increment x +5, fact_id=${incResp.fact_id}）`);

    await consumer;

    const types = events.map((e) => e.type);
    if (!types.includes("Command")) {
      fail(tr, "SSE 事件", "缺少 Command 事件");
    } else if (!types.includes("StateTransition")) {
      fail(tr, "SSE 事件", "缺少 StateTransition 事件");
    } else if (!types.includes("Stable")) {
      fail(tr, "SSE 事件", "缺少 Stable 事件");
    } else {
      ok(tr, `SSE 事件序列（${events.length} 个事件: ${types.join(", ")}）`);
    }

    const state = await session.state();
    if (state.payload["x"] !== 5) {
      fail(tr, "state 验证", `x 应为 5，实际 ${state.payload["x"]}`);
    } else {
      ok(tr, `state 验证（x = ${state.payload["x"]}）`);
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 3：Payload 更新 =====
async function test03PayloadUpdate(tr: TestResult): Promise<void> {
  console.log("\n[场景 3] Payload 更新");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();
    await session.command({ type: "set", params: { attr: "status", value: "init" } });

    const resp1 = await session.updatePayload("status", "running");
    if (resp1.success !== true) {
      fail(tr, "updatePayload（status = running）", `success=false, msg=${resp1.message}`);
    } else {
      ok(tr, "updatePayload（status = running）");
    }

    const state1 = await session.state();
    if (state1.payload["status"] !== "running") {
      fail(tr, "state 验证", `status 应为 running，实际 ${state1.payload["status"]}`);
    } else {
      ok(tr, "state 验证（status = running）");
    }

    const resp2 = await session.updatePayload("nested.field", "deep");
    if (resp2.success !== true) {
      fail(tr, "updatePayload（nested.field = deep）", `success=false`);
    } else {
      ok(tr, "updatePayload（nested.field = deep）");
    }

    const state2 = await session.state();
    const nested = state2.payload["nested"] as Record<string, Json> | undefined;
    if (!nested || nested["field"] !== "deep") {
      fail(tr, "state 验证", `nested.field 应为 deep，实际 ${JSON.stringify(nested)}`);
    } else {
      ok(tr, "state 验证（nested.field = deep）");
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 4：时间旅行 =====
async function test04TimeMachine(tr: TestResult): Promise<void> {
  console.log("\n[场景 4] 时间旅行");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();
    await session.command({ type: "set", params: { attr: "counter", value: 0 } });
    await session.command({ type: "increment", params: { attr: "counter", delta: 1 } });
    await session.command({ type: "increment", params: { attr: "counter", delta: 1 } });
    const stateV3 = await session.state();
    const v3 = stateV3.version;
    ok(tr, `执行 3 条命令（version=${v3}, counter=${stateV3.payload["counter"]}）`);

    const replay = await session.replay();
    ok(tr, `replay（返回字段: ${Object.keys(replay).join(", ")}）`);

    const v2 = v3 - 1;
    const rewind = await session.rewind(v2);
    ok(tr, `rewind(version=${v2})`);

    const stateAfter = await session.state();
    if (stateAfter.version < v2) {
      fail(tr, "state 验证回滚", `版本应 >= ${v2}，实际 ${stateAfter.version}`);
    } else {
      ok(tr, `state 验证回滚（version=${stateAfter.version}）`);
    }

    const diff = (await session.diff(1, v3)) as DiffResponse;
    if (
      diff.version_a === undefined ||
      diff.version_b === undefined ||
      !Array.isArray(diff.added) ||
      !Array.isArray(diff.removed) ||
      !Array.isArray(diff.changed)
    ) {
      fail(tr, "diff", `缺少字段，返回: ${JSON.stringify(diff).slice(0, 100)}`);
    } else {
      ok(
        tr,
        `diff(v1→v${v3}): version_a=${diff.version_a}, ` +
          `added=${diff.added.length}, removed=${diff.removed.length}, changed=${diff.changed.length}`,
      );
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 5：Debug 端点 =====
async function test05DebugEndpoints(tr: TestResult): Promise<void> {
  console.log("\n[场景 5] Debug 端点");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();
    await session.command({ type: "set", params: { attr: "debug_test", value: 1 } });

    const phase = await session.debugPhase();
    if (typeof phase !== "string") {
      fail(tr, "debug_phase", `返回不是 string: ${typeof phase}`);
    } else {
      ok(tr, `debug_phase（phase=${phase}）`);
    }

    const queue = await session.debugQueue();
    if (!Array.isArray(queue)) {
      fail(tr, "debug_queue", `返回不是数组: ${typeof queue}`);
    } else {
      ok(tr, `debug_queue（queue_len=${queue.length}）`);
    }

    const pendingIo = (await session.debugPendingIo()) as PendingIoInfo[];
    if (!Array.isArray(pendingIo)) {
      fail(tr, "debug_pending_io", `返回不是数组`);
    } else {
      ok(tr, `debug_pending_io（count=${pendingIo.length}）`);
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 6：执行中断 =====
async function test06Interrupt(tr: TestResult): Promise<void> {
  console.log("\n[场景 6] 执行中断");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();
    await session.command({ type: "set", params: { attr: "x", value: 0 } });
    ok(tr, "初始状态就绪");

    const resp = await session.interrupt();
    ok(tr, `interrupt（message=${resp.message || "N/A"}）`);

    const state = await session.state();
    if (!("payload" in state)) {
      fail(tr, "中断后状态", "缺少 payload");
    } else {
      ok(tr, `中断后状态可读（version=${state.version}）`);
    }

    await session.close();
    await sleep(500);
  } finally {
    await client.close();
  }
}

// ===== 场景 7：共享 Facts =====
async function test07SharedFacts(tr: TestResult): Promise<void> {
  console.log("\n[场景 7] 共享 Facts");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();
    await sleep(200);

    const payloadResp = await session.updatePayload("shared.greeting", "hello");
    ok(tr, `updatePayload（shared.greeting = hello, success=${payloadResp.success}）`);

    await sleep(200);

    const cmdResp = await session.command({
      type: "set",
      params: { attr: "shared.knowledge.value", value: 42 },
    });
    ok(tr, `command（set shared.knowledge.value=42, fact_id=${cmdResp.fact_id}）`);

    const facts = await client.sharedFacts("shared.");
    if (!Array.isArray(facts)) {
      fail(tr, "shared_facts", `返回不是数组: ${typeof facts}`);
    } else {
      ok(tr, `shared_facts（count=${facts.length}）`);
    }

    if (facts.length > 0) {
      const factId = facts[0].fact_id;
      const source = await client.sharedFactSource(factId);
      ok(tr, `shared_fact_source(fact_id=${factId}, path=${source.path})`);

      const usedBy = await client.sharedFactUsedBy(factId);
      ok(tr, `shared_fact_used_by(fact_id=${factId}, sessions=${usedBy.sessions.length})`);
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 8：Used at Startup =====
async function test08UsedAtStartup(tr: TestResult): Promise<void> {
  console.log("\n[场景 8] Used at Startup");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();

    const resp = await session.recordUsedAtStartup([1, 2, 3]);
    ok(tr, `recordUsedAtStartup（fact_ids=[1,2,3], msg=${resp.message || "N/A"}）`);

    const used = (await session.getUsedAtStartup()) as UsedAtStartupResponse;
    if (!("fact_ids" in used) && !("session_id" in used)) {
      fail(tr, "getUsedAtStartup", `返回缺少字段: ${JSON.stringify(used)}`);
    } else {
      ok(tr, `getUsedAtStartup（fact_ids=${JSON.stringify(used.fact_ids)}）`);
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 9：IO Response =====
async function test09IoResponse(tr: TestResult): Promise<void> {
  console.log("\n[场景 9] IO Response");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();

    try {
      const resp = await session.submitIoResponse(999999, { status: "ok" });
      ok(tr, `submitIoResponse（msg=${resp.message || "N/A"}）`);
    } catch (e) {
      if (e instanceof CommandError || e instanceof EvoruleError) {
        ok(tr, `submitIoResponse 异常符合预期（${e.constructor.name}: ${e.message}）`);
      } else {
        throw e;
      }
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 10：审计链 =====
async function test10Audit(tr: TestResult): Promise<void> {
  console.log("\n[场景 10] 审计链");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();
    await session.command({ type: "set", params: { attr: "audit_test", value: 1 } });

    const audit = await session.audit();
    const keys = Object.keys(audit);
    ok(tr, `audit（返回字段: ${keys.join(", ")}）`);

    const verify = (await session.auditVerify()) as AuditVerifyResponse;
    if (typeof verify.valid !== "boolean") {
      fail(tr, "audit_verify", `缺少 valid 字段: ${JSON.stringify(verify)}`);
    } else {
      ok(tr, `audit_verify（valid=${verify.valid}, session_id=${verify.session_id}）`);
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 11：历史查询 =====
async function test11History(tr: TestResult): Promise<void> {
  console.log("\n[场景 11] 历史查询");
  const client = new EvoruleClient(BASE_URL);
  try {
    const session = await client.createSession();
    await session.command({ type: "set", params: { attr: "hist", value: "a" } });
    await session.command({ type: "set", params: { attr: "hist", value: "b" } });

    const history = (await session.history()) as HistoryEntry[];
    if (!Array.isArray(history)) {
      fail(tr, "history", `返回不是数组: ${typeof history}`);
    } else {
      ok(tr, `history（返回 ${history.length} 条）`);
    }

    await session.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 12：集群协作 =====
async function test12Cluster(tr: TestResult): Promise<void> {
  console.log("\n[场景 12] 集群协作");
  const client = new EvoruleClient(BASE_URL);
  try {
    const s1 = await client.createSession();
    const s2 = await client.createSession();
    ok(tr, `创建 2 个会话（${s1.sessionId}, ${s2.sessionId}）`);

    const joinResp = await s1.join(s2.sessionId, "bidirectional");
    ok(tr, `join（${s1.sessionId} ↔ ${s2.sessionId}, msg=${joinResp.message || "N/A"}）`);

    const status = (await s1.clusterStatus()) as ClusterStatusResponse;
    const keys = Object.keys(status as unknown as Record<string, unknown>);
    ok(tr, `cluster_status（keys=${keys.join(", ")}）`);

    const leaveResp = await s1.leave();
    ok(tr, `leave（msg=${leaveResp.message || "N/A"}）`);

    await s1.close();
    await s2.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 13：会话分叉 =====
async function test13Fork(tr: TestResult): Promise<void> {
  console.log("\n[场景 13] 会话分叉");
  const client = new EvoruleClient(BASE_URL);
  try {
    const parent = await client.createSession();
    await parent.command({ type: "set", params: { attr: "forked", value: true } });
    const parentState = await parent.state();
    ok(tr, `父会话就绪（version=${parentState.version}）`);

    const forkResp = (await client.forkSession(
      parent.sessionId,
      parentState.version,
    )) as ForkSessionResponse;
    const childId = forkResp.session_id;
    if (childId <= 0 || childId === parent.sessionId) {
      fail(tr, "fork_session", `child_id 无效: ${childId}`);
    } else {
      ok(
        tr,
        `fork_session（parent=${parent.sessionId} → child=${childId}, from_version=${forkResp.forked_from_version}）`,
      );
    }

    const child = new Session(BASE_URL, childId, {}, 30000);
    const childState = await child.state();
    if (childState.payload["forked"] !== true) {
      fail(tr, "子会话继承父状态", `forked 应为 true，实际 ${childState.payload["forked"]}`);
    } else {
      ok(tr, `子会话继承父状态（forked=${childState.payload["forked"]}）`);
    }

    await child.close();
    await parent.close();
  } finally {
    await client.close();
  }
}

// ===== 场景 14：健康检查 =====
async function test14Health(tr: TestResult): Promise<void> {
  console.log("\n[场景 14] 健康检查");
  const client = new EvoruleClient(BASE_URL);
  try {
    const h = await client.health();
    if (h.success !== true) {
      fail(tr, "health", `success=false`);
    } else {
      ok(tr, `health（message=${h.message}）`);
    }

    const live = await client.liveness();
    if (live.success !== true) {
      fail(tr, "liveness", `success=false`);
    } else {
      ok(tr, `liveness（message=${live.message}）`);
    }

    const ready = await client.readiness();
    if (ready.success !== true) {
      fail(tr, "readiness", `success=false`);
    } else {
      ok(tr, `readiness（message=${ready.message}）`);
    }
  } finally {
    await client.close();
  }
}

// ===== 主入口 =====
async function main(): Promise<number> {
  console.log("evorule TypeScript SDK E2E 测试");
  console.log(`服务器: ${BASE_URL}`);
  console.log(`时间: ${new Date().toISOString()}`);

  const tr = createTestResult();

  const tests: Array<(tr: TestResult) => Promise<void>> = [
    test01SessionLifecycle,
    test02CommandAndSse,
    test03PayloadUpdate,
    test04TimeMachine,
    test05DebugEndpoints,
    test06Interrupt,
    test07SharedFacts,
    test08UsedAtStartup,
    test09IoResponse,
    test10Audit,
    test11History,
    test12Cluster,
    test13Fork,
    test14Health,
  ];

  for (const testFn of tests) {
    try {
      await testFn(tr);
    } catch (e) {
      fail(
        tr,
        testFn.name,
        `未预期异常: ${e instanceof Error ? `${e.constructor.name}: ${e.message}` : String(e)}`,
      );
    }
  }

  const success = summary(tr);
  return success ? 0 : 1;
}

main().then((code) => {
  process.exit(code);
}).catch((e) => {
  console.error("Fatal error:", e);
  process.exit(2);
});
