/**
 * evorule SDK 会话管理
 *
 * 每个 Session 对应服务端一个独立的长驻反应器实例。
 */

import { Event } from "./events.js";
import {
  ApiResponse,
  AuditVerifyResponse,
  AuthenticationError,
  ClusterStatusResponse,
  CommandError,
  DebugPendingIoResponse,
  DebugPhaseResponse,
  DebugQueueResponse,
  DiffResponse,
  HistoryEntry,
  Instruction,
  Json,
  PendingIoInfo,
  ReplayResponse,
  RewindResponse,
  SessionClosedError,
  SessionFactEntry,
  SessionNotFoundError,
  SessionState,
  SyncDirection,
  UsedAtStartupResponse,
} from "./types.js";

/** 会话客户端 */
export class Session {
  private readonly _baseUrl: string;
  private readonly _headers: Record<string, string>;
  private readonly _timeout: number;
  readonly sessionId: number;
  private _closed = false;

  constructor(
    baseUrl: string,
    sessionId: number,
    headers: Record<string, string>,
    timeout: number,
  ) {
    this._baseUrl = baseUrl;
    this.sessionId = sessionId;
    this._headers = headers;
    this._timeout = timeout;
  }

  get closed(): boolean {
    return this._closed;
  }

  private get _url(): string {
    return `${this._baseUrl}/api/sessions/${this.sessionId}`;
  }

  private _checkClosed(): void {
    if (this._closed) {
      throw new SessionClosedError(
        `Session ${this.sessionId} already closed`,
      );
    }
  }

  private async _fetch(
    path: string,
    init?: RequestInit,
  ): Promise<Response> {
    this._checkClosed();
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this._timeout);
    try {
      const resp = await fetch(`${this._url}${path}`, {
        ...init,
        headers: { ...this._headers, ...init?.headers },
        signal: controller.signal,
      });
      if (resp.status === 404) {
        throw new SessionNotFoundError(
          `Session ${this.sessionId} not found`,
        );
      }
      if (resp.status === 401) {
        throw new AuthenticationError("Authentication failed");
      }
      return resp;
    } finally {
      clearTimeout(timer);
    }
  }

  /** 提交命令到会话的反应器 */
  async command(instruction: Instruction): Promise<ApiResponse> {
    const resp = await this._fetch("/command", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ instruction }),
    });
    const data = (await resp.json()) as ApiResponse;
    if (!data.success) {
      throw new CommandError(data.message);
    }
    return data;
  }

  /** 查询会话当前状态快照 */
  async state(): Promise<SessionState> {
    const resp = await this._fetch("/state");
    return (await resp.json()) as SessionState;
  }

  /** 更新会话的 payload 字段 */
  async updatePayload(path: string, value: Json): Promise<ApiResponse> {
    const resp = await this._fetch("/payload", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path, value }),
    });
    const data = (await resp.json()) as ApiResponse;
    if (!data.success) {
      throw new CommandError(data.message);
    }
    return data;
  }

  /** 中断会话反应器执行（POST /api/sessions/{id}/interrupt） */
  async interrupt(): Promise<ApiResponse> {
    const resp = await this._fetch("/interrupt", { method: "POST" });
    return (await resp.json()) as ApiResponse;
  }

  /** 回放会话的完整 FactsLog（GET /api/sessions/{id}/replay） */
  async replay(): Promise<ReplayResponse> {
    const resp = await this._fetch("/replay");
    return (await resp.json()) as ReplayResponse;
  }

  /** 回滚到指定版本（GET /api/sessions/{id}/rewind/{version}） */
  async rewind(version: number): Promise<RewindResponse> {
    const resp = await this._fetch(`/rewind/${version}`);
    return (await resp.json()) as RewindResponse;
  }

  /**
   * 对比两个版本的 payload 差异（GET /api/sessions/{id}/diff）
   *
   * @param fromVersion 起始版本（对应服务端参数 a）
   * @param toVersion 目标版本（对应服务端参数 b）
   */
  async diff(fromVersion: number, toVersion: number): Promise<DiffResponse> {
    const resp = await this._fetch(
      `/diff?a=${fromVersion}&b=${toVersion}`,
    );
    return (await resp.json()) as DiffResponse;
  }

  /**
   * 提交 I/O 响应（POST /api/sessions/{id}/io_response）
   *
   * 用于回应 IoRequest 事件。result 与 error 二选一：成功时填 result，
   * 失败时填 error。
   */
  async submitIoResponse(
    requestId: number,
    result?: Json,
    error?: string,
  ): Promise<ApiResponse> {
    const body: Record<string, Json> = { request_id: requestId };
    if (error !== undefined) {
      body.error = error;
    } else {
      body.result = result ?? null;
    }
    const resp = await this._fetch("/io_response", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return (await resp.json()) as ApiResponse;
  }

  /** 记录会话启动时引用的共享 Fact（POST /api/sessions/{id}/used_at_startup） */
  async recordUsedAtStartup(factIds: number[]): Promise<ApiResponse> {
    const resp = await this._fetch("/used_at_startup", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ fact_ids: factIds }),
    });
    return (await resp.json()) as ApiResponse;
  }

  /** 查询反应器当前阶段（GET /api/sessions/{id}/debug/phase） */
  async debugPhase(): Promise<string> {
    const resp = await this._fetch("/debug/phase");
    const data = (await resp.json()) as DebugPhaseResponse;
    return data.phase;
  }

  /** 查询反应器待执行队列（GET /api/sessions/{id}/debug/queue） */
  async debugQueue(): Promise<Json[]> {
    const resp = await this._fetch("/debug/queue");
    const data = (await resp.json()) as DebugQueueResponse;
    return data.queue;
  }

  /** 查询挂起的 I/O 请求（GET /api/sessions/{id}/debug/pending_io） */
  async debugPendingIo(): Promise<PendingIoInfo[]> {
    const resp = await this._fetch("/debug/pending_io");
    const data = (await resp.json()) as DebugPendingIoResponse;
    return data.pending_io;
  }

  /** 查询会话审计报告（GET /api/sessions/{id}/audit） */
  async audit(): Promise<Record<string, Json>> {
    const resp = await this._fetch("/audit");
    return (await resp.json()) as Record<string, Json>;
  }

  /** 校验会话审计链完整性（GET /api/sessions/{id}/audit/verify） */
  async auditVerify(): Promise<AuditVerifyResponse> {
    const resp = await this._fetch("/audit/verify");
    return (await resp.json()) as AuditVerifyResponse;
  }

  /** 查询会话历史（GET /api/sessions/{id}/history） */
  async history(): Promise<HistoryEntry[]> {
    const resp = await this._fetch("/history");
    return (await resp.json()) as HistoryEntry[];
  }

  /** 按路径前缀查询会话内 Facts（GET /api/sessions/{id}/facts） */
  async factsByPrefix(prefix?: string): Promise<SessionFactEntry[]> {
    const path = prefix
      ? `/facts?prefix=${encodeURIComponent(prefix)}`
      : "/facts";
    const resp = await this._fetch(path);
    return (await resp.json()) as SessionFactEntry[];
  }

  /** 查询会话启动时引用的共享 Fact ID（GET /api/sessions/{id}/used_at_startup） */
  async getUsedAtStartup(): Promise<UsedAtStartupResponse> {
    const resp = await this._fetch("/used_at_startup");
    return (await resp.json()) as UsedAtStartupResponse;
  }

  /**
   * 加入集群协作（POST /api/sessions/{id}/join）
   *
   * @param targetId 目标会话 ID
   * @param direction 同步方向："atob" / "btoa" / "bidirectional"（默认双向）
   */
  async join(
    targetId: number,
    direction?: SyncDirection,
  ): Promise<ApiResponse> {
    const body: Record<string, Json> = { target_id: targetId };
    if (direction !== undefined) {
      body.direction = direction;
    }
    const resp = await this._fetch("/join", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return (await resp.json()) as ApiResponse;
  }

  /** 离开所有集群协作（POST /api/sessions/{id}/leave） */
  async leave(): Promise<ApiResponse> {
    const resp = await this._fetch("/leave", { method: "POST" });
    return (await resp.json()) as ApiResponse;
  }

  /** 查询会话集群成员（GET /api/sessions/{id}/cluster） */
  async clusterStatus(): Promise<ClusterStatusResponse> {
    const resp = await this._fetch("/cluster");
    return (await resp.json()) as ClusterStatusResponse;
  }

  /**
   * 订阅 SSE 事件流
   *
   * 返回一个异步迭代器，持续产出 Event 对象。
   * 流是长连接，不会因超时自动断开；调用方可通过 signal 主动取消。
   *
   * 使用示例：
   * ```typescript
   * for await (const event of session.events()) {
   *   if (event.type === "Stable") break;
   *   console.log(event);
   * }
   * ```
   *
   * 参数：
   * @param signal 可选的 AbortSignal，用于主动取消订阅
   */
  async *events(signal?: AbortSignal): AsyncGenerator<Event, void, unknown> {
    this._checkClosed();
    // SSE 是长连接流，不应用请求超时；仅响应调用方传入的 signal
    const resp = await fetch(`${this._url}/events`, {
      headers: this._headers,
      signal,
    });
    if (resp.status === 404) {
      throw new SessionNotFoundError(
        `Session ${this.sessionId} not found`,
      );
    }
    if (resp.status === 401) {
      throw new AuthenticationError("Authentication failed");
    }
    if (!resp.ok) {
      throw new CommandError(`SSE stream failed: ${resp.status}`);
    }
    if (!resp.body) {
      throw new Error("Response body is null");
    }

    const reader = resp.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });

        // SSE 事件以空行（\n\n）分隔
        const parts = buffer.split("\n\n");
        buffer = parts.pop() ?? "";

        for (const rawEvent of parts) {
          for (const line of rawEvent.split("\n")) {
            if (line.startsWith("data: ")) {
              try {
                const json = JSON.parse(line.slice(6));
                yield Event.fromJson(json);
              } catch {
                // JSON 解析失败，跳过
              }
            }
          }
        }
      }
    } finally {
      // 读取端结束（done/break/throw）时释放 reader，避免底层 socket 泄漏
      try {
        await reader.cancel();
      } catch {
        // 忽略取消时的错误
      }
    }
  }

  /** 关闭会话（幂等，重复调用安全） */
  async close(): Promise<void> {
    if (this._closed) return;
    this._closed = true;
    try {
      await fetch(this._url, {
        method: "DELETE",
        headers: this._headers,
      });
    } catch {
      // 忽略关闭时的网络错误
    }
  }
}
