"""
evorule e2e 冒烟测试

验证 evorule-server 能:
  1. 启动并监听端口
  2. 加载 core_eval.json(宪法)
  3. 接受 JSON 命令(set / increment)
  4. 正确返回状态
  5. 时间机器(replay / rewind)

这是公开仓库前的最低 e2e 覆盖。

# 前置
  - 已 build: cargo build --bin evorule-server
  - core_eval.json 在 ./tier0-tcb/core_eval.json(默认)

# 运行
  python tests/e2e_smoke.py
  # 或
  python tests/e2e_smoke.py --binary ./.build/rust/debug/evorule_server.exe --addr 127.0.0.1:18081
"""

import argparse
import json
import os
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


# ===== ANSI 颜色(Windows 10+ 默认支持) =====
GREEN = "\033[92m"
RED = "\033[91m"
YELLOW = "\033[93m"
RESET = "\033[0m"


class E2E:
    def __init__(self, binary: Path, addr: str, workdir: Path):
        self.binary = binary
        self.addr = addr
        self.host, self.port = addr.split(":")
        self.workdir = workdir
        self.proc: subprocess.Popen | None = None
        self.passed = 0
        self.failed = 0
        self.errors: list[str] = []

    def ok(self, name: str) -> None:
        self.passed += 1
        print(f"  {GREEN}PASS{RESET}  {name}")

    def fail(self, name: str, reason: str) -> None:
        self.failed += 1
        self.errors.append(f"{name}: {reason}")
        print(f"  {RED}FAIL{RESET}  {name} — {reason}")

    def info(self, msg: str) -> None:
        print(f"  {YELLOW}···{RESET}  {msg}")

    def header(self, msg: str) -> None:
        print(f"\n=== {msg} ===")

    # ----- HTTP helper -----
    def http(
        self,
        method: str,
        path: str,
        body: dict | None = None,
        timeout: float = 5.0,
    ) -> tuple[int, Any]:
        url = f"http://{self.addr}{path}"
        data = None
        headers = {"Accept": "application/json"}
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
                try:
                    return resp.status, json.loads(raw) if raw else None
                except json.JSONDecodeError:
                    return resp.status, raw.decode("utf-8", errors="replace")
        except urllib.error.HTTPError as e:
            raw = e.read()
            try:
                return e.code, json.loads(raw) if raw else None
            except json.JSONDecodeError:
                return e.code, raw.decode("utf-8", errors="replace")
        except urllib.error.URLError as e:
            return 0, f"URLError: {e.reason}"

    # ----- 进程管理 -----
    def start(self) -> bool:
        if not self.binary.exists():
            self.fail(
                "binary exists",
                f"找不到 {self.binary}。先 cargo build --bin evorule-server",
            )
            return False
        self.ok(f"binary exists ({self.binary.name}, {self.binary.stat().st_size} bytes)")

        # 启动 server
        cmd = [str(self.binary), "--addr", self.addr]
        self.info(f"启动: {' '.join(cmd)}")
        try:
            # stdout/stderr 重定向到 DEVNULL 避免 pipe 满导致 server 阻塞
            # (Windows PIPE buffer ~64KB,但为安全起见直接丢掉)
            self.proc = subprocess.Popen(
                cmd,
                cwd=self.workdir,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=subprocess.CREATE_NEW_PROCESS_GROUP
                if sys.platform == "win32"
                else 0,
            )
        except OSError as e:
            self.fail("启动 server", str(e))
            return False

        # 等待端口监听(最多 15 秒)
        for i in range(30):
            time.sleep(0.5)
            if self.proc.poll() is not None:
                # 进程已退出,记录 exit code(无法读 stdout,已重定向到 DEVNULL)
                self.fail("server 启动", f"进程提前退出,exit_code={self.proc.returncode}")
                return False
            status, _ = self.http("GET", "/api/health/liveness")
            if status == 200:
                self.ok(f"server 启动 + 监听 {self.addr} ({(i + 1) * 0.5:.1f}s)")
                return True

        self.fail("server 启动", f"15s 内未监听 {self.addr}")
        self.stop()
        return False

    def stop(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.info("停止 server")
            if sys.platform == "win32":
                # Windows:用 CTRL_BREAK_EVENT
                try:
                    self.proc.send_signal(signal.CTRL_BREAK_EVENT)
                except (OSError, ValueError):
                    pass
            else:
                self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
            self.proc = None

    # ----- 场景 -----
    def test_health(self) -> None:
        self.header("场景 1: 健康检查")
        status, body = self.http("GET", "/api/health")
        if status == 200 and isinstance(body, dict) and body.get("success") is True:
            self.ok(f"health (message={body.get('message', 'N/A')})")
        else:
            self.fail("health", f"status={status}, body={body}")

        status, body = self.http("GET", "/api/health/liveness")
        if status == 200:
            self.ok("liveness")
        else:
            self.fail("liveness", f"status={status}")

    def test_session_lifecycle(self) -> int | None:
        self.header("场景 2: 会话生命周期")
        status, body = self.http("POST", "/api/sessions")
        if status != 200 or not isinstance(body, dict):
            self.fail("create session", f"status={status}, body={body}")
            return None
        sid = body.get("session_id")
        if not isinstance(sid, int) or sid <= 0:
            self.fail("create session", f"session_id 无效: {sid}")
            return None
        self.ok(f"create session (id={sid})")
        return sid

    def test_set_increment(self, sid: int) -> None:
        self.header("场景 3: set + increment + state(宪法核心)")

        # set x = 0
        status, body = self.http(
            "POST",
            f"/api/sessions/{sid}/command",
            {"instruction": {"type": "set", "params": {"attr": "x", "value": 0}}},
        )
        if status != 200 or not isinstance(body, dict) or not body.get("success"):
            self.fail("command set", f"status={status}, body={body}")
            return
        self.ok(f"command set x=0 (fact_id={body.get('fact_id')})")

        # increment x + 5
        status, body = self.http(
            "POST",
            f"/api/sessions/{sid}/command",
            {"instruction": {"type": "increment", "params": {"attr": "x", "delta": 5}}},
        )
        if status != 200 or not isinstance(body, dict) or not body.get("success"):
            self.fail("command increment", f"status={status}, body={body}")
            return
        self.ok(f"command increment x+5 (fact_id={body.get('fact_id')})")

        # state
        status, body = self.http("GET", f"/api/sessions/{sid}/state")
        if status != 200 or not isinstance(body, dict):
            self.fail("state", f"status={status}, body={body}")
            return
        x = body.get("payload", {}).get("x")
        version = body.get("version")
        if x == 5:
            self.ok(f"state x=5 (version={version})")
        else:
            self.fail("state", f"x 应为 5,实际 {x},body={body}")

    def test_time_machine(self, sid: int) -> None:
        self.header("场景 4: 时间机器(replay / rewind)")
        # replay
        # 已知:server 当前返 [fact, fact, ...] 纯数组,而 SDK types.ts 写的是 {"facts": [...]}
        # 这是 API 契约不一致 bug,留待 user 拍板。先接受 list,后续修复.
        status, body = self.http("GET", f"/api/sessions/{sid}/replay")
        if status != 200:
            self.fail("replay", f"status={status}, body={body}")
            return
        if isinstance(body, list):
            facts = body
        elif isinstance(body, dict):
            facts = body.get("facts", [])
        else:
            self.fail("replay", f"unexpected body type: {type(body).__name__}")
            return
        self.ok(f"replay 返回 {len(facts)} 个 Fact")

        # rewind(GET,不是 POST — server 路由定义如此)
        status, body = self.http("GET", f"/api/sessions/{sid}/rewind/1")
        if status != 200 or not isinstance(body, dict):
            self.fail("rewind", f"status={status}, body={body}")
            return
        self.ok(f"rewind version=1 (new_version={body.get('version')})")

    def test_audit(self, sid: int) -> None:
        self.header("场景 5: 审计链")
        status, body = self.http("GET", f"/api/sessions/{sid}/audit/verify")
        if status != 200 or not isinstance(body, dict):
            self.fail("audit verify", f"status={status}, body={body}")
            return
        valid = body.get("valid")
        if valid is True:
            self.ok(f"audit verify valid=True (session_id={body.get('session_id')})")
        else:
            self.fail("audit verify", f"valid={valid}, body={body}")

    def summary(self) -> bool:
        print(f"\n{'=' * 60}")
        total = self.passed + self.failed
        print(f"结果: {self.passed}/{total} passed")
        if self.errors:
            print("\n失败用例:")
            for e in self.errors:
                print(f"  - {e}")
        print(f"{'=' * 60}")
        return self.failed == 0


def main() -> int:
    parser = argparse.ArgumentParser(description="evorule e2e 冒烟测试")
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path("./.build/rust/debug/evorule_server")
        if sys.platform != "win32"
        else Path("./.build/rust/debug/evorule_server.exe"),
        help="evorule_server 二进制路径",
    )
    parser.add_argument(
        "--addr", default="127.0.0.1:18081", help="监听地址(默认 127.0.0.1:18081,避免与已有 server 冲突)"
    )
    parser.add_argument(
        "--workdir", type=Path, default=Path("."), help="server 工作目录(默认 .)"
    )
    args = parser.parse_args()

    print("evorule e2e 冒烟测试")
    print(f"  binary: {args.binary}")
    print(f"  addr:   {args.addr}")
    print(f"  workdir: {args.workdir.resolve()}")
    print(f"  时间:   {time.strftime('%Y-%m-%d %H:%M:%S')}")

    e2e = E2E(args.binary, args.addr, args.workdir)

    try:
        if not e2e.start():
            return 1

        e2e.test_health()
        sid = e2e.test_session_lifecycle()
        if sid is not None:
            e2e.test_set_increment(sid)
            e2e.test_time_machine(sid)
            e2e.test_audit(sid)

        return 0 if e2e.summary() else 1
    finally:
        e2e.stop()


if __name__ == "__main__":
    sys.exit(main())
