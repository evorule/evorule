"""evorule SDK 会话管理

每个 Session 对应服务端一个独立的长驻反应器实例，
拥有独立的 state / FactsLog / command-event 通道。
"""

from __future__ import annotations

import json
from types import TracebackType
from typing import TYPE_CHECKING, Any, AsyncIterator

from .events import Event
from .exceptions import CommandError, SessionClosedError, SessionNotFoundError

if TYPE_CHECKING:
    from .client import EvoruleClient


class Session:
    """会话客户端

    通过 `EvoruleClient.create_session()` 创建，封装单会话的命令提交、
    状态查询、payload 更新和 SSE 事件流订阅。

    支持 `async with` 上下文管理器，退出时自动关闭会话。
    """

    def __init__(self, client: EvoruleClient, session_id: int) -> None:
        self._client = client
        self.session_id = session_id
        self._closed = False

    def __repr__(self) -> str:
        return f"Session(id={self.session_id}, closed={self._closed})"

    async def __aenter__(self) -> Session:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        await self.close()

    def _check_closed(self) -> None:
        if self._closed:
            raise SessionClosedError(f"Session {self.session_id} already closed")

    def _url(self, suffix: str = "") -> str:
        base = f"/api/sessions/{self.session_id}"
        return f"{base}{suffix}" if suffix else base

    async def command(self, instruction: dict[str, Any]) -> dict[str, Any]:
        """提交命令到会话的反应器

        参数：
            instruction: 指令 JSON，如 `{"type": "increment", "params": {"attr": "x", "delta": 5}}`

        返回：
            服务端响应 `{"success": bool, "message": str, "fact_id": int | None}`

        异常：
            CommandError: 命令提交失败（channel closed）
            SessionNotFoundError: 会话不存在
        """
        self._check_closed()
        resp = await self._client._http.post(
            self._url("/command"),
            json={"instruction": instruction},
        )
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        data = resp.json()
        if not data.get("success", False):
            raise CommandError(data.get("message", "Command submission failed"))
        return data

    async def state(self) -> dict[str, Any]:
        """查询会话当前状态快照

        返回：
            `{"payload": {...}, "queue": [...], "version": int}`
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/state"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def update_payload(self, path: str, value: Any) -> dict[str, Any]:
        """更新会话的 payload 字段

        参数：
            path: 字段路径（如 "status" 或 "nested.field"）
            value: 字段值

        返回：
            服务端响应

        异常：
            CommandError: 反应器通道已关闭（payload 提交失败）
            SessionNotFoundError: 会话不存在
        """
        self._check_closed()
        resp = await self._client._http.post(
            self._url("/payload"),
            json={"path": path, "value": value},
        )
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        data = resp.json()
        if not data.get("success", False):
            raise CommandError(data.get("message", "Payload update failed"))
        return data

    async def interrupt(self) -> dict[str, Any]:
        """中断会话反应器执行（POST /api/sessions/{id}/interrupt）

        触发服务端 AtomicBool 中断标志，反应器主循环在下个检查点
        会发出 Error + Stable 事件并停止当前指令序列。

        返回：
            服务端响应
        """
        self._check_closed()
        resp = await self._client._http.post(self._url("/interrupt"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def replay(self) -> dict[str, Any]:
        """回放会话的完整 FactsLog（GET /api/sessions/{id}/replay）

        返回：
            `{"facts": [...]}`，facts 为 Fact 列表
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/replay"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def rewind(self, version: int) -> dict[str, Any]:
        """回滚到指定版本（GET /api/sessions/{id}/rewind/{version}）

        参数：
            version: 目标版本号

        返回：
            `{"version": int, "payload": {...}, "queue": [...]}`
        """
        self._check_closed()
        resp = await self._client._http.get(self._url(f"/rewind/{version}"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def diff(self, from_version: int, to_version: int) -> dict[str, Any]:
        """对比两个版本的 payload 差异（GET /api/sessions/{id}/diff）

        参数：
            from_version: 起始版本（对应服务端参数 a）
            to_version: 目标版本（对应服务端参数 b）

        返回：
            `{"version_a": int, "version_b": int, "added": [...], "removed": [...], "changed": [...]}`
        """
        self._check_closed()
        resp = await self._client._http.get(
            self._url("/diff"),
            params={"a": from_version, "b": to_version},
        )
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def submit_io_response(
        self,
        request_id: int,
        result: Any = None,
        error: str | None = None,
    ) -> dict[str, Any]:
        """提交 I/O 响应（POST /api/sessions/{id}/io_response）

        用于回应 IoRequest 事件。result 与 error 二选一：成功时填 result，
        失败时填 error。

        参数：
            request_id: 对应 IoRequest 事件的 request_id
            result: I/O 成功结果（可选）
            error: I/O 错误信息（可选）

        返回：
            服务端响应
        """
        self._check_closed()
        body: dict[str, Any] = {"request_id": request_id}
        if error is not None:
            body["error"] = error
        else:
            body["result"] = result
        resp = await self._client._http.post(self._url("/io_response"), json=body)
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def record_used_at_startup(self, fact_ids: list[int]) -> dict[str, Any]:
        """记录会话启动时引用的共享 Fact（POST /api/sessions/{id}/used_at_startup）

        参数：
            fact_ids: 启动时引用的共享 Fact ID 列表

        返回：
            服务端响应
        """
        self._check_closed()
        resp = await self._client._http.post(
            self._url("/used_at_startup"),
            json={"fact_ids": fact_ids},
        )
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def debug_phase(self) -> dict[str, Any]:
        """查询反应器当前阶段（GET /api/sessions/{id}/debug/phase）

        返回：
            `{"session_id": int, "phase": str}`
            phase 取值: Idle/Draining/Executing/AwaitingIo/Stable/Error
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/debug/phase"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def debug_queue(self) -> dict[str, Any]:
        """查询反应器待执行队列（GET /api/sessions/{id}/debug/queue）

        返回：
            `{"session_id": int, "queue": [...]}`
            queue 为当前队列中的指令 JSON 列表
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/debug/queue"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def debug_pending_io(self) -> dict[str, Any]:
        """查询挂起的 I/O 请求（GET /api/sessions/{id}/debug/pending_io）

        返回：
            `{"session_id": int, "pending_io": [...]}`
            pending_io 列表中每项包含 fact_id / io_type / duration_ms
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/debug/pending_io"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def audit(self, limit: int | None = None) -> dict[str, Any]:
        """查询会话审计报告（GET /api/sessions/{id}/audit）

        参数：
            limit: 可选，返回条目数限制（客户端侧过滤）

        返回：
            审计报告 JSON（包含事实链、哈希等）
        """
        self._check_closed()
        params: dict[str, Any] = {}
        if limit is not None:
            params["limit"] = limit
        resp = await self._client._http.get(self._url("/audit"), params=params if params else None)
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def audit_verify(self) -> dict[str, Any]:
        """校验会话审计链完整性（GET /api/sessions/{id}/audit/verify）

        返回：
            `{"valid": bool, "session_id": int}`
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/audit/verify"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def history(self, limit: int | None = None) -> list[dict[str, Any]]:
        """查询会话历史（GET /api/sessions/{id}/history）

        参数：
            limit: 可选，返回条目数限制（客户端侧过滤）

        返回：
            `[{"version": int, "type": str}, ...]`
        """
        self._check_closed()
        params: dict[str, Any] = {}
        if limit is not None:
            params["limit"] = limit
        resp = await self._client._http.get(self._url("/history"), params=params if params else None)
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        result = resp.json()
        if isinstance(result, list) and limit is not None:
            return result[:limit]
        return result

    async def facts_by_prefix(self, prefix: str = "") -> list[dict[str, Any]]:
        """按路径前缀查询会话内 Facts（GET /api/sessions/{id}/facts）

        参数：
            prefix: 路径前缀（可选）

        返回：
            `[{"fact_id", "version", "path", "value"}, ...]`
        """
        self._check_closed()
        params: dict[str, str] = {}
        if prefix:
            params["prefix"] = prefix
        resp = await self._client._http.get(self._url("/facts"), params=params)
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def get_used_at_startup(self) -> list[int]:
        """查询会话启动时引用的共享 Fact ID（GET /api/sessions/{id}/used_at_startup）

        返回：
            Fact ID 列表
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/used_at_startup"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json().get("fact_ids", [])

    async def join(
        self,
        target_id: int | None = None,
        direction: str | None = None,
        target_session_id: int | None = None,
    ) -> dict[str, Any]:
        """加入集群协作（POST /api/sessions/{id}/join）

        参数：
            target_id: 目标会话 ID（推荐使用）
            target_session_id: 目标会话 ID（别名，向后兼容）
            direction: 同步方向，可选 "atob" / "btoa" / None（双向）

        返回：
            服务端响应
        """
        self._check_closed()
        tid = target_id if target_id is not None else target_session_id
        if tid is None:
            raise ValueError("target_id or target_session_id must be provided")
        body: dict[str, Any] = {"target_id": tid}
        if direction is not None:
            body["direction"] = direction
        resp = await self._client._http.post(self._url("/join"), json=body)
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def leave(self) -> dict[str, Any]:
        """离开所有集群协作（POST /api/sessions/{id}/leave）

        返回：
            服务端响应
        """
        self._check_closed()
        resp = await self._client._http.post(self._url("/leave"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def cluster_status(self) -> dict[str, Any]:
        """查询会话集群成员（GET /api/sessions/{id}/cluster）

        返回：
            `{"session_id": int, "cluster_members": [...]}`
        """
        self._check_closed()
        resp = await self._client._http.get(self._url("/cluster"))
        if resp.status_code == 404:
            raise SessionNotFoundError(f"Session {self.session_id} not found")
        resp.raise_for_status()
        return resp.json()

    async def events(self) -> AsyncIterator[Event]:
        """订阅 SSE 事件流

        返回一个异步迭代器，持续产出 Event 对象。
        流是长连接，反应器在长驻模式下持续推送事件。

        使用示例：
            async for event in session.events():
                if event.type == "Stable":
                    break
                print(event)

        注意：调用方负责在适当时机 break 退出循环，
        否则流将持续到会话关闭或连接断开。
        """
        self._check_closed()
        url = self._url("/events")
        async with self._client._http.stream("GET", url) as response:
            if response.status_code == 404:
                raise SessionNotFoundError(f"Session {self.session_id} not found")
            response.raise_for_status()
            async for line in response.aiter_lines():
                if not line:
                    continue
                if line.startswith("data: "):
                    data_str = line[len("data: "):]
                    try:
                        data = json.loads(data_str)
                        yield Event.from_dict(data)
                    except json.JSONDecodeError:
                        continue

    async def close(self) -> None:
        """关闭会话（DELETE /api/sessions/{id}）

        重复调用安全（幂等）。
        """
        if self._closed:
            return
        self._closed = True
        try:
            await self._client._http.delete(self._url())
        except Exception:
            pass
