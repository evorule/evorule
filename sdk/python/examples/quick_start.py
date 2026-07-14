#!/usr/bin/env python3
"""evorule Python SDK 快速开始示例

前置条件：
1. 安装 SDK：pip install -e sdk/python
2. 启动 evorule-server：cargo run --bin evorule-server -- --addr 127.0.0.1:18080

运行：
    python sdk/python/examples/quick_start.py
"""

import asyncio
import json

from evorule import EvoruleClient


async def main() -> None:
    print("=== evorule Python SDK 快速开始 ===\n")

    # 1. 创建客户端（async with 自动管理连接生命周期）
    async with EvoruleClient("http://127.0.0.1:18080") as client:
        # 健康检查
        health = await client.health()
        print(f"健康检查: {health}")

        # 2. 创建会话（async with 自动关闭会话）
        async with await client.create_session() as session:
            print(f"已创建会话: {session}\n")

            # 3. 启动 SSE 事件流订阅（后台任务）
            sse_events: list = []

            async def consume_events() -> None:
                async for event in session.events():
                    sse_events.append(event)
                    print(f"  [SSE] {event}")
                    if event.type == "Stable" and len(sse_events) >= 8:
                        break

            sse_task = asyncio.create_task(consume_events())
            await asyncio.sleep(0.3)  # 等待 SSE 连接建立

            # 4. 提交命令 1：increment x=5
            print("--- 提交命令 1: increment x=5 ---")
            result = await session.command({
                "type": "increment",
                "params": {"attr": "x", "delta": 5},
            })
            print(f"响应: {result}\n")
            await asyncio.sleep(0.3)

            # 5. 提交命令 2：sequence(increment y=3, increment x=10)
            print("--- 提交命令 2: sequence(increment y=3, increment x=10) ---")
            result = await session.command({
                "type": "sequence",
                "params": {
                    "instructions": [
                        {"type": "increment", "params": {"attr": "y", "delta": 3}},
                        {"type": "increment", "params": {"attr": "x", "delta": 10}},
                    ]
                },
            })
            print(f"响应: {result}\n")

            # 6. 等待 SSE 事件接收完成
            await asyncio.wait_for(sse_task, timeout=5.0)

            # 7. 查询最终状态
            print("--- 查询最终状态 ---")
            state = await session.state()
            print(f"状态: {json.dumps(state, ensure_ascii=False)}")

            x = state.get("payload", {}).get("x", 0)
            y = state.get("payload", {}).get("y", 0)
            print(f"\n验证: x={x} (期望 15), y={y} (期望 3)")
            assert x == 15 and y == 3, f"状态验证失败: x={x}, y={y}"
            print("\n✅ SDK 端到端验证通过！")

    print("\n=== 完成 ===")


if __name__ == "__main__":
    asyncio.run(main())
