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

    async def liveness(self) -> dict[str, Any]:
        """Liveness 探针（GET /api/health/liveness）

        始终返回 200，只要进程在运行就算存活。

        返回：
            `{"success": true, "message": "alive", "fact_id": null}`
        """
        resp = await self._http.get("/api/health/liveness")
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json()

    async def readiness(self) -> dict[str, Any]:
        """Readiness 探针（GET /api/health/readiness）

        就绪时返回 200，未就绪（如优雅退出中）返回 503。

        返回：
            `{"success": true, "message": "ready", "fact_id": null}`
        异常：
            httpx.HTTPStatusError: 服务未就绪（503）
        """
        resp = await self._http.get("/api/health/readiness")
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json()

    async def fork_session(self, parent_id: int, version: int | None = None) -> dict[str, Any]:
        """从父会话的指定版本分叉新会话

        - 传 version 时使用 POST /api/sessions/fork/{parent_id}?version=X（需指定版本）
        - 不传 version 时使用 POST /api/sessions/from/{parent_id}（从最新版本分叉）

        参数：
            parent_id: 父会话 ID
            version: 分叉起点版本号（可选，不传则从最新版本分叉）

        返回：
            `{"session_id", "parent_session_id", "forked_from_version", "message"}`
        """
        if version is not None:
            resp = await self._http.post(
                f"/api/sessions/fork/{parent_id}",
                params={"version": version},
            )
        else:
            resp = await self._http.post(
                f"/api/sessions/from/{parent_id}",
            )
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json()

    async def shared_facts(self, prefix: str = "") -> list[dict[str, Any]]:
        """查询共享 Fact 列表（GET /api/shared/facts）

        参数：
            prefix: 可选的路径前缀过滤（如 "user.profile"）

        返回：
            共享 Fact 列表，每项包含 fact_id / path / value / source_session_id / version
        """
        params: dict[str, str] = {}
        if prefix:
            params["prefix"] = prefix
        resp = await self._http.get("/api/shared/facts", params=params)
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json()

    async def shared_fact_source(self, fact_id: int) -> dict[str, Any]:
        """查询共享 Fact 的来源信息（GET /api/shared/facts/{fact_id}/source）

        参数：
            fact_id: 共享 Fact ID

        返回：
            `{"fact_id", "path", "value", "source_session_id", "version"}`
        """
        resp = await self._http.get(f"/api/shared/facts/{fact_id}/source")
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json()

    async def shared_fact_used_by(self, fact_id: int) -> dict[str, Any]:
        """查询使用了指定共享 Fact 的会话列表（GET /api/shared/facts/{fact_id}/used_by）

        参数：
            fact_id: 共享 Fact ID

        返回：
            `{"fact_id": int, "sessions": [int, ...]}`
        """
        resp = await self._http.get(f"/api/shared/facts/{fact_id}/used_by")
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

    async def shared_facts(self, prefix: str = "") -> list[dict[str, Any]]:
        """查询共享 Fact 列表（GET /api/shared/facts）

        参数：
            prefix: 可选的路径前缀过滤（如 "user.profile"）

        返回：
            共享 Fact 列表，每项包含 fact_id / path / value / source_session_id / version
        """
        params: dict[str, str] = {}
        if prefix:
            params["prefix"] = prefix
        resp = await self._http.get("/api/shared/facts", params=params)
        if resp.status_code == 401:
            raise AuthenticationError("Authentication failed")
        resp.raise_for_status()
        return resp.json()

    async def close(self) -> None:
        """关闭客户端，释放底层 HTTP 连接"""
        await self._http.aclose()
