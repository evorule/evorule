"""evorule SDK 异常定义"""

from __future__ import annotations


class EvoruleError(Exception):
    """evorule SDK 基础异常"""


class AuthenticationError(EvoruleError):
    """认证失败（HTTP 401）"""


class SessionNotFoundError(EvoruleError):
    """会话不存在（HTTP 404）"""


class CommandError(EvoruleError):
    """命令提交失败（channel closed 或其他错误）"""


class ConnectionError(EvoruleError):
    """连接服务器失败"""
