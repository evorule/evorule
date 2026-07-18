"""evorule SDK SSE 事件类型定义

对应服务端 `fact_to_sse_data()` 序列化的 7 种 Fact 变体：
Command / StateTransition / IoRequest / IoResponse / Stable / PayloadUpdate / Error
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class Event:
    """SSE 事件

    服务端推送的每个 SSE `data:` 行解析为一个 Event。
    `raw` 字段保留完整的 JSON 字典，便捷属性从 raw 中提取常用字段。
    """

    type: str
    """事件类型：Command / StateTransition / IoRequest / IoResponse / Stable / PayloadUpdate / Error"""

    id: int
    """事件 ID（API 层 Command 从 30000 起，反应器内部 StateTransition/Stable 从 1 起）"""

    raw: dict[str, Any] = field(default_factory=dict, repr=False)
    """完整的事件 JSON 数据"""

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Event:
        """从 JSON 字典构造 Event"""
        return cls(
            type=data.get("type", "Unknown"),
            id=data.get("id", 0),
            raw=data,
        )

    # ===== 便捷属性（按事件类型可选存在）=====

    @property
    def cause(self) -> int | None:
        """StateTransition / IoRequest 的触发源 FactId"""
        return self.raw.get("cause")

    @property
    def instruction(self) -> dict[str, Any] | None:
        """Command 事件携带的指令"""
        return self.raw.get("instruction")

    @property
    def new_payload(self) -> dict[str, Any] | None:
        """StateTransition 事件执行后的 payload 快照"""
        return self.raw.get("new_payload")

    @property
    def new_queue(self) -> list[dict[str, Any]] | None:
        """StateTransition 事件执行后的队列快照"""
        return self.raw.get("new_queue")

    @property
    def final_snapshot(self) -> dict[str, Any] | None:
        """Stable 事件的稳定状态快照"""
        return self.raw.get("final_snapshot")

    @property
    def io_type(self) -> str | None:
        """IoRequest 事件的 I/O 类型（call_external / query_db / http_get / save_memory / call_service）"""
        return self.raw.get("io_type")

    @property
    def params(self) -> dict[str, Any] | None:
        """IoRequest 事件的参数"""
        return self.raw.get("params")

    @property
    def request_id(self) -> int | None:
        """IoResponse 事件对应的 IoRequest ID"""
        return self.raw.get("request_id")

    @property
    def result(self) -> Any | None:
        """IoResponse 事件的 I/O 结果"""
        return self.raw.get("result")

    @property
    def error(self) -> str | None:
        """IoResponse / Error 事件的错误信息"""
        return self.raw.get("error") or self.raw.get("message")

    @property
    def path(self) -> str | None:
        """PayloadUpdate 事件的字段路径"""
        return self.raw.get("path")

    @property
    def value(self) -> Any | None:
        """PayloadUpdate 事件的字段值"""
        return self.raw.get("value")

    @property
    def message(self) -> str | None:
        """Error 事件的错误消息"""
        return self.raw.get("message")

    def __str__(self) -> str:
        parts = [f"{self.type}(id={self.id}"]
        if self.cause is not None:
            parts.append(f", cause={self.cause}")
        if self.io_type is not None:
            parts.append(f", io_type={self.io_type}")
        if self.error is not None:
            parts.append(f", error={self.error}")
        parts.append(")")
        return "".join(parts)
