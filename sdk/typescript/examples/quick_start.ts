/**
 * evorule TypeScript SDK 快速开始示例
 *
 * 前置条件：
 * 1. 安装依赖：cd sdk/typescript && npm install
 * 2. 启动 evorule-server：cargo run --bin evorule-server -- --addr 127.0.0.1:18080
 *
 * 运行：
 *   npx tsx sdk/typescript/examples/quick_start.ts
 *   或编译后：node dist/examples/quick_start.js
 */

import { EvoruleClient } from "../src/index.js";

async function main(): Promise<void> {
  console.log("=== evorule TypeScript SDK 快速开始 ===\n");

  const client = new EvoruleClient("http://127.0.0.1:18080");

  try {
    // 1. 健康检查
    const health = await client.health();
    console.log("健康检查:", health);

    // 2. 创建会话
    const session = await client.createSession();
    console.log(`已创建会话: Session(id=${session.sessionId})\n`);

    // 3. 启动 SSE 事件流订阅（后台任务）
    const sseEvents: string[] = [];

    const ssePromise = (async () => {
      for await (const event of session.events()) {
        sseEvents.push(event.toString());
        console.log(`  [SSE] ${event}`);
        if (event.type === "Stable" && sseEvents.length >= 8) {
          break;
        }
      }
    })();

    // 等待 SSE 连接建立
    await sleep(300);

    // 4. 提交命令 1：increment x=5
    console.log("--- 提交命令 1: increment x=5 ---");
    const result1 = await session.command({
      type: "increment",
      params: { attr: "x", delta: 5 },
    });
    console.log("响应:", result1, "\n");
    await sleep(300);

    // 5. 提交命令 2：sequence(increment y=3, increment x=10)
    console.log("--- 提交命令 2: sequence(increment y=3, increment x=10) ---");
    const result2 = await session.command({
      type: "sequence",
      params: {
        instructions: [
          { type: "increment", params: { attr: "y", delta: 3 } },
          { type: "increment", params: { attr: "x", delta: 10 } },
        ],
      },
    });
    console.log("响应:", result2, "\n");

    // 6. 等待 SSE 事件接收完成
    await ssePromise;

    // 7. 查询最终状态
    console.log("--- 查询最终状态 ---");
    const state = await session.state();
    console.log("状态:", JSON.stringify(state));

    const x = (state.payload as Record<string, number>).x ?? 0;
    const y = (state.payload as Record<string, number>).y ?? 0;
    console.log(`\n验证: x=${x} (期望 15), y=${y} (期望 3)`);
    if (x === 15 && y === 3) {
      console.log("\n✅ SDK 端到端验证通过！\n");
    } else {
      console.log("\n❌ 状态验证失败！");
      process.exit(1);
    }

    // 8. 演示时间旅行：replay / rewind / diff
    console.log("--- 演示时间旅行：replay / rewind / diff ---");
    const replayData = await session.replay();
    console.log(`replay: 共 ${replayData.facts.length} 个 Fact`);

    const rewindData = await session.rewind(1);
    console.log(
      `rewind(1): version=${rewindData.version}, payload=${JSON.stringify(rewindData.payload)}`,
    );

    const diffData = await session.diff(1, 2);
    console.log(
      `diff(1→2): version_a=${diffData.version_a}, version_b=${diffData.version_b}`,
    );
    console.log(
      `  added=${diffData.added.length} 项, removed=${diffData.removed.length} 项, changed=${diffData.changed.length} 项`,
    );

    // 9. 演示 debug 端点
    console.log("\n--- 演示 debug 端点 ---");
    const phase = await session.debugPhase();
    console.log(`debug_phase: ${phase}`);
    const queue = await session.debugQueue();
    console.log(`debug_queue: ${queue.length} 项`);
    const pendingIo = await session.debugPendingIo();
    console.log(`debug_pending_io: ${pendingIo.length} 项`);

    // 10. 演示 interrupt（最后一步，触发后会停止反应器）
    console.log("\n--- 演示 interrupt ---");
    const interruptResp = await session.interrupt();
    console.log(`interrupt 响应: ${JSON.stringify(interruptResp)}`);
    console.log("反应器已中断，后续命令将失败");

    // 11. 关闭会话
    await session.close();
    console.log(`\n会话已关闭`);
  } finally {
    await client.close();
  }

  console.log("\n=== 完成 ===");
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch((err) => {
  console.error("运行失败:", err);
  process.exit(1);
});
