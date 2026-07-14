/**
 * evorule SDK 会话管理
 *
 * 每个 Session 对应服务端一个独立的长驻反应器实例。
 */

import { Event } from "./events.js";
import {
  ApiResponse,
  CommandError,
  Instruction,
  Json,
  SessionNotFoundError,
  SessionState,
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
      throw new SessionNotFoundError(
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
        throw new SessionNotFoundError("Authentication failed");
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
    return (await resp.json()) as ApiResponse;
  }

  /**
   * 订阅 SSE 事件流
   *
   * 返回一个异步迭代器，持续产出 Event 对象。
   *
   * 使用示例：
   * ```typescript
   * for await (const event of session.events()) {
   *   if (event.type === "Stable") break;
   *   console.log(event);
   * }
   * ```
   */
  async *events(): AsyncGenerator<Event, void, unknown> {
    this._checkClosed();
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this._timeout);
    try {
      const resp = await fetch(`${this._url}/events`, {
        headers: this._headers,
        signal: controller.signal,
      });
      if (resp.status === 404) {
        throw new SessionNotFoundError(
          `Session ${this.sessionId} not found`,
        );
      }
      if (!resp.body) {
        throw new Error("Response body is null");
      }

      const reader = resp.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";

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
      clearTimeout(timer);
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
