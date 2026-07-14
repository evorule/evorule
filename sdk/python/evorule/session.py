"""evorule SDK 会话管理

每个 Session 对应服务端一个独立的长驻反应器实例，
拥有独立的 state / FactsLog / command-event 通道。
"""

from __future__ import annotations

import json
from types import TracebackType
from typing import TYPE_CHECKING, Any, AsyncIterator

from .events import Event
from .exceptions import CommandError, SessionNotFoundError

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
            raise SessionNotFoundError(f"Session {self.session_id} already closed")

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
        """
        self._check_closed()
        resp = await self._client._http.post(
            self._url("/payload"),
            json={"path": path, "value": value},
        )
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
