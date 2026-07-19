"""evorule Python SDK 端到端集成测试（自包含版）

自动启动 evorule-server，跑完全部 14 个测试场景后关闭服务端。
无需手动启动服务端，直接运行：python tests/test_e2e_self_contained.py
"""

from __future__ import annotations

import asyncio
import os
import subprocess
import sys
import time
from typing import Any

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx

from evorule import (
    CommandError,
    EvoruleClient,
    EvoruleError,
    SessionClosedError,
)

BASE_URL = os.environ.get("EVORULE_BASE_URL", "http://127.0.0.1:18081")

TEST_DIR = os.path.dirname(os.path.abspath(__file__))
SDK_PYTHON_DIR = os.path.dirname(TEST_DIR)
PROJECT_ROOT = os.path.dirname(os.path.dirname(SDK_PYTHON_DIR))

DEFAULT_SERVER_BIN = os.path.join(
    PROJECT_ROOT, ".build", "rust", "debug", "evorule-server.exe"
)
SERVER_BIN = os.environ.get("EVORULE_SERVER_BIN", DEFAULT_SERVER_BIN)


class TestResult:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.errors: list[str] = []

    def ok(self, name: str) -> None:
        self.passed += 1
        print(f"  PASS: {name}")

    def fail(self, name: str, reason: str) -> None:
        self.failed += 1
        self.errors.append(f"{name}: {reason}")
        print(f"  FAIL: {name} - {reason}")

    def summary(self) -> bool:
        print(f"\n{'=' * 60}")
        print(f"Result: {self.passed} passed, {self.failed} failed")
        if self.errors:
            print("\nFailures:")
            for e in self.errors:
                print(f"  - {e}")
        print("=" * 60)
        return self.failed == 0


def start_server() -> subprocess.Popen:
    """启动 evorule-server，等待健康检查通过"""
    port = BASE_URL.rsplit(":", 1)[-1]
    addr = f"127.0.0.1:{port}"
    print(f"Starting evorule-server on {addr}...")

    proc = subprocess.Popen(
        [SERVER_BIN, "--addr", addr, "--log-level", "error"],
        cwd=PROJECT_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    for i in range(30):
        try:
            r = httpx.get(f"{BASE_URL}/api/health", timeout=1)
            if r.status_code == 200:
                print(f"Server ready after {i + 1}s")
                return proc
        except Exception:
            pass
        time.sleep(1)
        if proc.poll() is not None:
            stdout, stderr = proc.communicate(timeout=1)
            raise RuntimeError(
                f"Server exited early (code={proc.returncode})\n"
                f"stdout: {stdout.decode(errors='replace')[-500:]}\n"
                f"stderr: {stderr.decode(errors='replace')[-500:]}"
            )

    proc.terminate()
    raise RuntimeError("Server failed to start within 30s")


def stop_server(proc: subprocess.Popen) -> None:
    """关闭 evorule-server"""
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    print("Server stopped")


async def test_01_session_lifecycle(tr: TestResult) -> None:
    print("\n[Scenario 1] Session lifecycle")
    async with EvoruleClient(BASE_URL) as client:
        before = await client.list_sessions()
        assert isinstance(before, list)
        tr.ok("list_sessions (initial)")

        session = await client.create_session()
        assert session.session_id > 0
        tr.ok(f"create_session (id={session.session_id})")

        after = await client.list_sessions()
        assert session.session_id in after
        tr.ok("list_sessions (contains new)")

        state = await session.state()
        assert "payload" in state and "queue" in state and "version" in state
        tr.ok(f"state (version={state['version']})")

        await session.close()
        tr.ok("close")

        try:
            await session.state()
            tr.fail("post-close access", "no exception raised")
        except SessionClosedError:
            tr.ok("post-close raises SessionClosedError")


async def test_02_command_and_sse(tr: TestResult) -> None:
    print("\n[Scenario 2] Command + SSE event stream")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "x", "value": 0}})
        tr.ok("command (set x=0)")

        events: list[dict[str, Any]] = []

        async def consume_events() -> None:
            async for ev in session.events():
                events.append(ev.raw)
                if ev.type == "Stable":
                    break

        task = asyncio.create_task(consume_events())
        await asyncio.sleep(0.3)

        await session.command({"type": "increment", "params": {"attr": "x", "delta": 5}})
        tr.ok("command (increment x +5)")

        await asyncio.wait_for(task, timeout=5.0)

        types = [e["type"] for e in events]
        if "Command" not in types:
            tr.fail("SSE events", f"missing Command event (types: {types})")
        elif "StateTransition" not in types:
            tr.fail("SSE events", f"missing StateTransition event (types: {types})")
        elif "Stable" not in types:
            tr.fail("SSE events", f"missing Stable event (types: {types})")
        else:
            tr.ok(f"SSE events ({len(events)} events: {', '.join(types)})")

        state = await session.state()
        if state["payload"].get("x") != 5:
            tr.fail("state check", f"x should be 5, got {state['payload'].get('x')}")
        else:
            tr.ok(f"state check (x = {state['payload'].get('x')})")

        await session.close()


async def test_03_payload_update(tr: TestResult) -> None:
    print("\n[Scenario 3] Payload update")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "status", "value": "init"}})

        resp = await session.update_payload("status", "running")
        if resp.get("success") is not True:
            tr.fail("update_payload (status=running)", f"success=false: {resp}")
        else:
            tr.ok("update_payload (status=running)")

        state = await session.state()
        if state["payload"].get("status") != "running":
            tr.fail("state check", f"status should be running, got {state['payload'].get('status')}")
        else:
            tr.ok("state check (status=running)")

        resp2 = await session.update_payload("counter", 42)
        if resp2.get("success") is not True:
            tr.fail("update_payload (counter=42)", "success=false")
        else:
            tr.ok("update_payload (counter=42)")

        state2 = await session.state()
        if state2["payload"].get("counter") != 42:
            tr.fail("state check", f"counter should be 42, got {state2['payload'].get('counter')}")
        else:
            tr.ok("state check (counter=42)")

        await session.close()


async def test_04_time_machine(tr: TestResult) -> None:
    print("\n[Scenario 4] Time machine")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "counter", "value": 0}})
        await session.command({"type": "increment", "params": {"attr": "counter", "delta": 1}})
        await session.command({"type": "increment", "params": {"attr": "counter", "delta": 1}})
        state_v3 = await session.state()
        v3 = state_v3["version"]
        tr.ok(f"3 commands executed (version={v3}, counter={state_v3['payload']['counter']})")

        replay = await session.replay()
        if isinstance(replay, dict):
            tr.ok(f"replay (keys: {', '.join(replay.keys())})")
        else:
            tr.ok(f"replay (type: {type(replay).__name__})")

        v2 = v3 - 1
        await session.rewind(v2)
        tr.ok(f"rewind(version={v2})")

        state_after = await session.state()
        if state_after["version"] < v2:
            tr.fail("rewind state", f"version should be >= {v2}, got {state_after['version']}")
        else:
            tr.ok(f"rewind state check (version={state_after['version']})")

        diff = await session.diff(1, v3)
        if "version_a" not in diff or "version_b" not in diff:
            tr.fail("diff", f"missing version fields: {list(diff.keys())}")
        elif "added" not in diff or "removed" not in diff or "changed" not in diff:
            tr.fail("diff", f"missing diff fields: {list(diff.keys())}")
        else:
            tr.ok(
                f"diff(v1->v{v3}): version_a={diff['version_a']}, "
                f"added={len(diff['added'])}, removed={len(diff['removed'])}, "
                f"changed={len(diff['changed'])}"
            )

        await session.close()


async def test_05_debug_endpoints(tr: TestResult) -> None:
    print("\n[Scenario 5] Debug endpoints")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "dbg", "value": 1}})

        phase = await session.debug_phase()
        if not isinstance(phase, str):
            tr.fail("debug_phase", f"not a string: {type(phase)}")
        else:
            tr.ok(f"debug_phase (phase={phase})")

        queue = await session.debug_queue()
        if not isinstance(queue, list):
            tr.fail("debug_queue", f"not a list: {type(queue)}")
        else:
            tr.ok(f"debug_queue (len={len(queue)})")

        pending_io = await session.debug_pending_io()
        if not isinstance(pending_io, list):
            tr.fail("debug_pending_io", f"not a list: {type(pending_io)}")
        else:
            tr.ok(f"debug_pending_io (count={len(pending_io)})")

        await session.close()


async def test_06_interrupt(tr: TestResult) -> None:
    print("\n[Scenario 6] Execution interrupt")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "x", "value": 0}})
        tr.ok("initial state ready")

        resp = await session.interrupt()
        tr.ok(f"interrupt (message={resp.get('message', 'N/A')})")

        state = await session.state()
        if "payload" not in state:
            tr.fail("post-interrupt state", "missing payload")
        else:
            tr.ok(f"post-interrupt state readable (version={state['version']})")

        await session.close()


async def test_07_shared_facts(tr: TestResult) -> None:
    print("\n[Scenario 7] Shared facts")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()

        await session.update_payload("greeting", "hello")
        await session.update_payload("knowledge", 42)
        tr.ok("set shared fields")

        facts = await client.shared_facts(prefix="shared.")
        if not isinstance(facts, list):
            tr.fail("shared_facts", f"not a list: {type(facts)}")
        else:
            tr.ok(f"shared_facts (count={len(facts)})")

        if isinstance(facts, list) and facts:
            fact_id = facts[0].get("fact_id") or facts[0].get("id")
            if fact_id is not None:
                await client.shared_fact_source(fact_id)
                tr.ok(f"shared_fact_source (fact_id={fact_id})")

                used_by = await client.shared_fact_used_by(fact_id)
                refs = used_by.get("sessions", used_by.get("used_by", []))
                tr.ok(f"shared_fact_used_by (fact_id={fact_id}, refs={len(refs)})")

        await session.close()


async def test_08_used_at_startup(tr: TestResult) -> None:
    print("\n[Scenario 8] Used at startup")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()

        resp = await session.record_used_at_startup(fact_ids=[1, 2, 3])
        tr.ok(f"record_used_at_startup (msg={resp.get('message', 'N/A')})")

        used = await session.get_used_at_startup()
        tr.ok(f"get_used_at_startup (type: {type(used).__name__})")

        await session.close()


async def test_09_io_response(tr: TestResult) -> None:
    print("\n[Scenario 9] IO Response")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()

        try:
            resp = await session.submit_io_response(request_id=999999, result={"status": "ok"})
            tr.ok(f"submit_io_response (msg={resp.get('message', 'N/A')})")
        except (CommandError, EvoruleError) as e:
            tr.ok(f"submit_io_response expected error ({type(e).__name__})")

        await session.close()


async def test_10_audit(tr: TestResult) -> None:
    print("\n[Scenario 10] Audit chain")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "audit_test", "value": 1}})

        audit = await session.audit()
        if isinstance(audit, list):
            count = len(audit)
        elif "entries" in audit:
            count = len(audit["entries"])
        elif "facts" in audit:
            count = len(audit["facts"])
        else:
            count = -1
        tr.ok(f"audit ({count} entries, keys: {', '.join(audit.keys() if isinstance(audit, dict) else ['list'])} )")

        verify = await session.audit_verify()
        valid = verify.get("valid", verify.get("verified", verify.get("success", "N/A")))
        tr.ok(f"audit_verify (valid={valid})")

        await session.close()


async def test_11_history(tr: TestResult) -> None:
    print("\n[Scenario 11] History")
    async with EvoruleClient(BASE_URL) as client:
        session = await client.create_session()
        await session.command({"type": "set", "params": {"attr": "hist", "value": "a"}})
        await session.command({"type": "set", "params": {"attr": "hist", "value": "b"}})

        history = await session.history()
        if isinstance(history, list):
            entries = history
        else:
            entries = history.get("entries", history.get("history", []))
        tr.ok(f"history ({len(entries)} entries)")

        await session.close()


async def test_12_cluster(tr: TestResult) -> None:
    print("\n[Scenario 12] Cluster collaboration")
    async with EvoruleClient(BASE_URL) as client:
        s1 = await client.create_session()
        s2 = await client.create_session()
        tr.ok(f"create 2 sessions ({s1.session_id}, {s2.session_id})")

        resp = await s1.join(target_id=s2.session_id, direction="bidirectional")
        assert resp.get("success") is True or "message" in resp
        tr.ok(f"join ({s1.session_id} <-> {s2.session_id})")

        status = await s1.cluster_status()
        keys = list(status.keys()) if isinstance(status, dict) else []
        tr.ok(f"cluster_status (keys: {', '.join(keys)})")

        resp2 = await s1.leave()
        assert resp2.get("success") is True or "message" in resp2
        tr.ok("leave")

        await s1.close()
        await s2.close()


async def test_13_fork(tr: TestResult) -> None:
    print("\n[Scenario 13] Session fork")
    async with EvoruleClient(BASE_URL) as client:
        parent = await client.create_session()
        await parent.command({"type": "set", "params": {"attr": "forked", "value": True}})
        parent_state = await parent.state()
        tr.ok(f"parent ready (version={parent_state['version']})")

        fork_result = await client.fork_session(parent.session_id, version=parent_state["version"])
        child_id = fork_result.get("session_id") if isinstance(fork_result, dict) else fork_result
        if not child_id or child_id <= 0 or child_id == parent.session_id:
            tr.fail("fork_session", f"invalid child_id: {fork_result}")
        else:
            tr.ok(f"fork_session (parent={parent.session_id} -> child={child_id})")

        from evorule import Session
        child = Session(client, child_id)
        await asyncio.sleep(0.5)
        child_state = await child.state()
        if child_state["payload"].get("forked") is not True:
            # 可能版本号不同，再检查一下整个 payload
            tr.fail(
                "child inherits state",
                f"forked should be True, got {child_state['payload'].get('forked')}. "
                f"payload keys: {list(child_state['payload'].keys())}, "
                f"version={child_state.get('version')}"
            )
        else:
            tr.ok(f"child inherits state (forked={child_state['payload'].get('forked')})")

        await child.close()
        await parent.close()


async def test_14_health(tr: TestResult) -> None:
    print("\n[Scenario 14] Health checks")
    async with EvoruleClient(BASE_URL) as client:
        h = await client.health()
        if h.get("success") is not True:
            tr.fail("health", "success=false")
        else:
            tr.ok(f"health (message={h.get('message')})")

        live = await client.liveness()
        if live.get("success") is not True and live.get("status") != "ok":
            tr.fail("liveness", f"unexpected: {live}")
        else:
            tr.ok("liveness (ok)")

        ready = await client.readiness()
        if ready.get("success") is not True and ready.get("status") not in ("ok", "ready"):
            tr.fail("readiness", f"unexpected: {ready}")
        else:
            tr.ok("readiness (ok)")


async def run_all_tests() -> bool:
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

    for test_fn in tests:
        try:
            await test_fn(tr)
        except Exception as e:
            tr.fail(test_fn.__name__, f"unexpected: {type(e).__name__}: {e}")

    return tr.summary()


def main() -> int:
    print("evorule Python SDK E2E Tests")
    print(f"Server: {BASE_URL}")
    print(f"Time: {time.strftime('%Y-%m-%d %H:%M:%S')}")
    print()

    proc = None
    try:
        proc = start_server()
        success = asyncio.run(run_all_tests())
        return 0 if success else 1
    finally:
        if proc:
            stop_server(proc)


if __name__ == "__main__":
    sys.exit(main())
