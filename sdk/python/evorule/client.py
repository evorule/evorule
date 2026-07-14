"""evorule SDK 主客户端

通过 HTTP API 与 evorule-server 交互，提供会话管理、健康检查等接口。

使用示例：
    import asyncio
    from evorule import EvoruleClient

    async def main():
        async with EvoruleClient("http://localhost:18080") as client:
            async with await client.create_session() as session:
                await session.command({"type": "increment", "params": {"attr": "x", "delta": 5}})
                state = await session.state()
                print(state)

    asyncio.run(main())
"""

from __future__ import annotations

from types import TracebackType
from typing import Any

import httpx

from .exceptions import AuthenticationError
from .session import Session


class EvoruleClient:
    """evorule-server 客户端

    参数：
        base_url: 服务器地址，如 "http://localhost:18080"
        token: Bearer 认证 token（可选，未提供时服务器需禁用认证）
        timeout: 请求超时时间（秒），默认 30

    支持 `async with` 上下文管理器，退出时自动关闭底层 HTTP 连接。
    """

    def __init__(
        self,
        base_url: str,
        token: str | None = None,
        timeout: float = 30.0,
    ) -> None:
        headers: dict[str, str] = {}
        if token:
            headers["Authorization"] = f"Bearer {token}"
        self._http = httpx.AsyncClient(
            base_url=base_url,
            headers=headers,
            timeout=timeout,
        )
        self._base_url = base_url

    def __repr__(self) -> str:
        return f"EvoruleClient(base_url={self._base_url!r})"

    async def __aenter__(self) -> EvoruleClient:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        await self.close()

    async def health(self) -> dict[str, Any]:
        """健康检查（GET /api/health）

        返回：
            `{"success": true, "message": "ok", "fact_id": null}`
        """
        resp = await self._http.get("/api/health")
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json()

    async def create_session(self) -> Session:
        """创建会话（POST /api/sessions）

        返回：
            Session 实例，封装单会话的操作接口
        """
        resp = await self._http.post("/api/sessions")
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        data = resp.json()
        session_id = data["session_id"]
        return Session(self, session_id)

    async def list_sessions(self) -> list[int]:
        """列出所有活跃会话（GET /api/sessions）

        返回：
            会话 ID 列表
        """
        resp = await self._http.get("/api/sessions")
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json().get("sessions", [])

    async def close(self) -> None:
        """关闭客户端，释放底层 HTTP 连接"""
        await self._http.aclose()
