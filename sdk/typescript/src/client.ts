/**
 * evorule SDK 主客户端
 *
 * 通过 HTTP API 与 evorule-server 交互。
 *
 * 使用示例：
 * ```typescript
 * import { EvoruleClient } from '@evorule/sdk';
 *
 * const client = new EvoruleClient('http://localhost:18080');
 * const session = await client.createSession();
 * await session.command({ type: 'increment', params: { attr: 'x', delta: 5 } });
 * const state = await session.state();
 * console.log(state);
 * await session.close();
 * ```
 */

import { Session } from "./session.js";
import {
  ApiResponse,
  AuthenticationError,
  ClientOptions,
  CreateSessionResponse,
  ListSessionsResponse,
} from "./types.js";

/** evorule-server 客户端 */
export class EvoruleClient {
  private readonly _baseUrl: string;
  private readonly _headers: Record<string, string>;
  private readonly _timeout: number;

  constructor(baseUrl: string, options?: ClientOptions) {
    // 去除尾部斜杠
    this._baseUrl = baseUrl.replace(/\/+$/, "");
    this._headers = {};
    if (options?.token) {
      this._headers["Authorization"] = `Bearer ${options.token}`;
    }
    this._timeout = options?.timeout ?? 30000;
  }

  private async _fetch(path: string, init?: RequestInit): Promise<Response> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this._timeout);
    try {
      const resp = await fetch(`${this._baseUrl}${path}`, {
        ...init,
        headers: { ...this._headers, ...init?.headers },
        signal: controller.signal,
      });
      if (resp.status === 401) {
        throw new AuthenticationError("Authentication failed");
      }
      return resp;
    } finally {
      clearTimeout(timer);
    }
  }

  /** 健康检查 */
  async health(): Promise<ApiResponse> {
    const resp = await this._fetch("/api/health");
    return (await resp.json()) as ApiResponse;
  }

  /** 创建会话 */
  async createSession(): Promise<Session> {
    const resp = await this._fetch("/api/sessions", { method: "POST" });
    const data = (await resp.json()) as CreateSessionResponse;
    return new Session(
      this._baseUrl,
      data.session_id,
      this._headers,
      this._timeout,
    );
  }

  /** 列出所有活跃会话 */
  async listSessions(): Promise<number[]> {
    const resp = await this._fetch("/api/sessions");
    const data = (await resp.json()) as ListSessionsResponse;
    return data.sessions;
  }

  /** 关闭客户端（释放资源） */
  async close(): Promise<void> {
    // fetch 无需显式关闭
  }
}
