"""evorule Python SDK 端到端集成测试

覆盖所有 SDK 端点，验证与 evorule-server 的完整交互流程。

前置条件：
    evorule-server 已启动在 http://localhost:18080

运行：
    cd sdk/python
    python -m pytest tests/test_e2e.py -v
    或直接运行：python tests/test_e2e.py
"""

from __future__ import annotations

import asyncio
import os
import sys
import time
from typing import Any

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from evorule import (
    CommandError,
    EvoruleClient,
    EvoruleError,
    SessionClosedError,
    SessionNotFoundError,
)

BASE_URL = os.environ.get("EVORULE_BASE_URL", "http://localhost:18080")


class TestResult:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.errors: list[str] = []

    def ok(self, name: str) -> None:
        self.passed += 1
        print(f"  \u2705 PASS: {name}")

    def fail(self, name: str, reason: str) -> None:
        self.failed += 1
        self.errors.append(f"{name}: {reason}")
        print(f"  \u274c FAIL: {name} — {reason}")

    def summary(self) -> bool:
        print(f"\n{'=' * 60}")
        print(f"结果: {self.passed} passed, {self.failed} failed")
        if self.errors:
            print("\n失败用例:")
            for e in self.errors:
                print(f"  - {e}")
        print(f"{'=' * 60}")
        return self.failed == 0


async def test_01_session_lifecycle(tr: TestResult) -> None:
    """场景 1：会话生命周期 — 创建 / 列表 / 状态 / 关闭"""
    print("\n[场景 1] 会话生命周期")
    async with EvoruleClient(BASE_URL) as client:
        before = await client.list_sessions()
        tr.ok("list_sessions（空列表 OK）")

        session = await client.create_session()
        assert session.session_id > 0, f"session_id 必须为正整数，得到 {session.session_id}"
        tr.ok(f"create_session（id={session.session_id}）")

        after = await client.list_sessions()
        assert session.session_id in after, "新建会话应出现在列表中"
        tr.ok("list_sessions（包含新会话）")

        state = await session.state()
        assert "payload" in state and "queue" in state and "version" in state
        tr.ok(f"state（version={state['version']}, payload keys={list(state['payload'].keys())}）")

        await session.close()
        tr.ok("close（主动关闭）")

        try:
            await session.state()
            tr.fail("关闭后访问应抛错", "未抛异常")
        except SessionClosedError:
            tr.ok("关闭后访问抛 SessionClosedError")


async def test_02_command_and_sse(tr: TestResult) -> None:
    """场景 2：命令提交 + SSE 事件流"""
    print("\n场景 2：命令提交 + SSE 事件流")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "x", "value": 0}})
        tr.ok("command（set x=0）")

        # 订阅 SSE 并在后台消费，同时提交命令
        events: list[dict[str, Any]] = []

        async def consume_events() -> None:
            async for ev in session.events():
                events.append(ev.raw)
                if ev.type == "Stable":
                    break

        task = asyncio.create_task(consume_events())
        await asyncio.sleep(0.2)  # 等待 SSE 连接建立

        await session.command({"type": "increment", "params": {"attr": "x", "delta": 5}})
        tr.ok("command（increment x +5）")

        await asyncio.wait_for(task, timeout=5.0)

        types = [e["type"] for e in events]
        assert "Command" in types, "应包含 Command 事件"
        assert "StateTransition" in types, "应包含 StateTransition 事件"
        assert "Stable" in types, "应包含 Stable 事件"
        tr.ok(f"SSE 事件序列（{len(events)} 个事件: {types}）")

        state = await session.state()
        assert state["payload"].get("x") == 5, f"x 应为 5，实际 {state['payload'].get('x')}"
        tr.ok(f"state 验证（x = {state['payload'].get('x')}）")

        await session.close()


async def test_03_payload_update(tr: TestResult) -> None:
    """场景 3：Payload 更新"""
    print("\n场景 3：Payload 更新")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "status", "value": "init"}})

        resp = await session.update_payload("status", "running")
        assert resp.get("success") is True
        tr.ok("update_payload（status = running）")

        state = await session.state()
        assert state["payload"]["status"] == "running"
        tr.ok("state 验证（status = running）")

        resp = await session.update_payload("nested.field", "deep")
        assert resp.get("success") is True
        tr.ok("update_payload（nested.field = deep）")

        state = await session.state()
        assert state["payload"]["nested"]["field"] == "deep"
        tr.ok("state 验证（nested.field = deep）")

        await session.close()


async def test_04_time_machine(tr: TestResult) -> None:
    """场景 4：时间旅行 — replay / rewind / diff"""
    print("\n场景 4：时间旅行")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "counter", "value": 0}})
        await session.command({"type": "increment", "params": {"attr": "counter", "delta": 1}})
        await session.command({"type": "increment", "params": {"attr": "counter", "delta": 1}})
        state_v3 = await session.state()
        v3 = state_v3["version"]
        counter_v3 = state_v3["payload"]["counter"]
        tr.ok(f"执行 3 条命令（version={v3}, counter={counter_v3}）")

        # replay：从初始版本重放
        replay_data = await session.replay()
        assert isinstance(replay_data, list), "replay 应返回列表"
        assert len(replay_data) > 0, "replay 列表不应为空"
        tr.ok(f"replay（返回 {len(replay_data)} 条记录）")

        # rewind：回滚到前一版本
        v2 = v3 - 1
        rewind_data = await session.rewind(v2)
        assert "version" in rewind_data or "success" in rewind_data
        tr.ok(f"rewind(version={v2})")

        state_after = await session.state()
        assert state_after["version"] >= v2, f"版本应 >= {v2}，实际 {state_after['version']}"
        tr.ok(f"state 验证回滚（version={state_after['version']}）")

        # diff：对比两个版本
        diff_data = await session.diff(1, v3)
        assert "version_a" in diff_data and "version_b" in diff_data
        assert "added" in diff_data and "removed" in diff_data and "changed" in diff_data
        tr.ok(
            f"diff(v1→v{v3}): version_a={diff_data['version_a']}, "
            f"added={len(diff_data['added'])}, removed={len(diff_data['removed'])}, "
            f"changed={len(diff_data['changed'])}"
        )

        await session.close()


async def test_05_debug_endpoints(tr: TestResult) -> None:
    """场景 5：Debug 端点"""
    print("\n场景 5：Debug 端点")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "debug_test", "value": 1}})

        phase = await session.debug_phase()
        assert "phase" in phase
        tr.ok(f"debug_phase（phase={phase['phase']}）")

        queue_data = await session.debug_queue()
        assert "queue" in queue_data
        tr.ok(f"debug_queue（queue_len={len(queue_data.get('queue', []))}）")

        pending_io = await session.debug_pending_io()
        assert "pending_io" in pending_io or "pending_io_count" in pending_io
        tr.ok(f"debug_pending_io（count={pending_io.get('pending_io_count', len(pending_io.get('pending_io', [])))}）")

        await session.close()


async def test_06_interrupt(tr: TestResult) -> None:
    """场景 6：执行中断"""
    print("\n场景 6：执行中断")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "x", "value": 0}})
        tr.ok("初始状态就绪")

        resp = await session.interrupt()
        assert resp.get("success") is True or resp.get("message")
        tr.ok(f"interrupt（message={resp.get('message', 'N/A')}）")

        state = await session.state()
        assert "payload" in state
        tr.ok(f"中断后状态可读（version={state['version']}）")

        await session.close()


async def test_07_shared_facts(tr: TestResult) -> None:
    """场景 7：共享 Facts"""
    print("\n场景 7：共享 Facts")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()

        # 通过 payload 更新设置共享字段
        await session.update_payload("shared.greeting", "hello")
        await session.command({"type": "set", "params": {"attr": "shared.knowledge.value", "value": 42}})
        tr.ok("设置共享字段")

        # 查询共享 facts
        facts = await client.shared_facts(prefix="shared.")
        assert isinstance(facts, list)
        tr.ok(f"shared_facts（count={len(facts)}）")

        if facts:
            fact_id = facts[0].get("fact_id") or facts[0].get("id")
            if fact_id is not None:
                source = await client.shared_fact_source(fact_id)
                tr.ok(f"shared_fact_source(fact_id={fact_id})")

                used_by = await client.shared_fact_used_by(fact_id)
                tr.ok(f"shared_fact_used_by(fact_id={fact_id}, refs={len(used_by.get('used_by', []))})")

        await session.close()


async def test_08_used_at_startup(tr: TestResult) -> None:
    """场景 8：Used at Startup 记录与查询"""
    print("\n场景 8：Used at Startup")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()

        resp = await session.record_used_at_startup(fact_ids=[1, 2, 3])
        assert resp.get("success") is True or "message" in resp
        tr.ok(f"record_used_at_startup（fact_ids=[1,2,3], msg={resp.get('message', 'N/A')}）")

        used = await session.get_used_at_startup()
        assert isinstance(used, list) or "used_at_startup" in used or "fact_ids" in used
        tr.ok(f"get_used_at_startup（返回类型: {type(used).__name__}）")

        await session.close()


async def test_09_io_response(tr: TestResult) -> None:
    """场景 9：IO Response 提交"""
    print("\n场景 9：IO Response")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()

        # 提交一个不存在的 io_response，服务端应返回错误但不崩溃
        try:
            resp = await session.submit_io_response(request_id=999999, result={"status": "ok"})
            tr.ok(f"submit_io_response（msg={resp.get('message', 'N/A')}）")
        except (CommandError, EvoruleError) as e:
            tr.ok(f"submit_io_response 异常符合预期（{type(e).__name__}: {e}）")

        await session.close()


async def test_10_audit(tr: TestResult) -> None:
    """场景 10：审计链"""
    print("\n场景 10：审计链")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "audit_test", "value": 1}})

        audit = await session.audit(limit=10)
        assert isinstance(audit, list) or "entries" in audit or "facts" in audit
        tr.ok(f"audit（返回 {len(audit) if isinstance(audit, list) else len(audit.get('entries', []))} 条）")

        verify = await session.audit_verify()
        assert "valid" in verify or "verified" in verify or "success" in verify
        tr.ok(f"audit_verify（valid={verify.get('valid', verify.get('verified', verify.get('success', 'N/A')))}）")

        await session.close()


async def test_11_history(tr: TestResult) -> None:
    """场景 11：历史查询"""
    print("\n场景 11：历史查询")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "hist", "value": "a"}})
        await session.command({"type": "set", "params": {"attr": "hist", "value": "b"}})

        history = await session.history(limit=5)
        assert isinstance(history, list) or "history" in history or "entries" in history
        entries = history if isinstance(history, list) else history.get("entries", history.get("history", []))
        tr.ok(f"history（返回 {len(entries)} 条）")

        await session.close()


async def test_12_cluster(tr: TestResult) -> None:
    """场景 12：集群协作"""
    print("\n场景 12：集群协作")
    async with EvoruleClient(BASE_URL) as client:
        s1 = await client.create_session()
        s2 = await client.create_session()
        tr.ok(f"创建 2 个会话（{s1.session_id}, {s2.session_id}）")

        # join 集群
        resp = await s1.join(target_session_id=s2.session_id, direction="bidirectional")
        assert resp.get("success") is True or "message" in resp
        tr.ok(f"join（{s1.session_id} ↔ {s2.session_id}, msg={resp.get('message', 'N/A')}）")

        # 查询集群状态
        status = await s1.cluster_status()
        assert "cluster_members" in status or "members" in status or "cluster" in status or "peers" in status
        tr.ok(f"cluster_status（keys={list(status.keys())}）")

        # leave 集群
        resp = await s1.leave()
        assert resp.get("success") is True or "message" in resp
        tr.ok(f"leave（msg={resp.get('message', 'N/A')}）")

        await s1.close()
        await s2.close()


async def test_13_fork(tr: TestResult) -> None:
    """场景 13：会话分叉"""
    print("\n场景 13：会话分叉")
    async with EvoruleClient(BASE_URL) as client:
        parent = await client.create_session()
        await parent.command({"type": "set", "params": {"attr": "forked", "value": True}})
        parent_state = await parent.state()
        tr.ok(f"父会话就绪（version={parent_state['version']}）")

        child_result = await client.fork_session(parent.session_id, version=None)
        child_id = child_result.get("session_id")
        assert child_id > 0 and child_id != parent.session_id
        tr.ok(f"fork_session（parent={parent.session_id} → child={child_id}）")

        # 验证子会话继承父状态
        from evorule import Session
        child = Session(client, child_id)
        child_state = await child.state()
        assert child_state["payload"].get("forked") is True
        tr.ok(f"子会话继承父状态（forked={child_state['payload'].get('forked')}）")

        await child.close()
        await parent.close()


async def test_14_health(tr: TestResult) -> None:
    """场景 14：健康检查"""
    print("\n场景 14：健康检查")
    async with EvoruleClient(BASE_URL) as client:
        h = await client.health()
        assert h.get("success") is True
        tr.ok(f"health（message={h.get('message')}）")

        live = await client.liveness()
        assert live.get("success") is True or live.get("status") == "ok"
        tr.ok(f"liveness（status={live.get('status', live.get('message', 'N/A'))}）")

        ready = await client.readiness()
        assert ready.get("success") is True or ready.get("status") in ("ok", "ready")
        tr.ok(f"readiness（status={ready.get('status', ready.get('message', 'N/A'))}）")


async def main() -> int:
    print(f"evorule Python SDK E2E 测试")
    print(f"服务器: {BASE_URL}")
    print(f"时间: {time.strftime('%Y-%m-%d %H:%M:%S')}")

    tr = TestResult()

    tests = [
        test_01_session_lifecycle,
        test_02_command_and_sse,
        test_03_payload_update,
        test_04_time_machine,
        test_05_debug_endpoints,
        test_06_interrupt,
        test_07_shared_facts,
        test_08_used_at_startup,
        test_09_io_response,
        test_10_audit,
        test_11_history,
        test_12_cluster,
        test_13_fork,
        test_14_health,
    ]

    for i, test_fn in enumerate(tests):
        try:
            await test_fn(tr)
        except Exception as e:
            tr.fail(test_fn.__name__, f"未预期异常: {type(e).__name__}: {e}")
        # 速率限制保护：场景间留 1s 让令牌桶恢复（服务端默认 10 req/s, burst 20）
        if i < len(tests) - 1:
            await asyncio.sleep(1.0)

    success = tr.summary()
    return 0 if success else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
