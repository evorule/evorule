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
  ForkSessionResponse,
  ListSessionsResponse,
  SharedFact,
  SharedFactSourceResponse,
  SharedFactUsedByResponse,
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

  /** Liveness 探针（GET /api/health/liveness），始终返回 200 */
  async liveness(): Promise<ApiResponse> {
    const resp = await this._fetch("/api/health/liveness");
    return (await resp.json()) as ApiResponse;
  }

  /** Readiness 探针（GET /api/health/readiness），未就绪时返回 503 */
  async readiness(): Promise<ApiResponse> {
    const resp = await this._fetch("/api/health/readiness");
    return (await resp.json()) as ApiResponse;
  }

  /**
   * 从父会话的指定版本分叉新会话（POST /api/sessions/fork/{parent_id}?version=X）
   *
   * @param parentId 父会话 ID
   * @param version 分叉起点版本号
   */
  async forkSession(
    parentId: number,
    version: number,
  ): Promise<ForkSessionResponse> {
    const resp = await this._fetch(
      `/api/sessions/fork/${parentId}?version=${version}`,
      { method: "POST" },
    );
    return (await resp.json()) as ForkSessionResponse;
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

  /**
   * 查询共享 Fact 列表（GET /api/shared/facts）
   *
   * @param prefix 可选的路径前缀过滤（如 "user.profile"）
   */
  async sharedFacts(prefix?: string): Promise<SharedFact[]> {
    const path = prefix
      ? `/api/shared/facts?prefix=${encodeURIComponent(prefix)}`
      : "/api/shared/facts";
    const resp = await this._fetch(path);
    return (await resp.json()) as SharedFact[];
  }

  /** 查询共享 Fact 的来源信息（GET /api/shared/facts/{factId}/source） */
  async sharedFactSource(factId: number): Promise<SharedFactSourceResponse> {
    const resp = await this._fetch(`/api/shared/facts/${factId}/source`);
    return (await resp.json()) as SharedFactSourceResponse;
  }

  /** 查询使用了指定共享 Fact 的会话列表（GET /api/shared/facts/{factId}/used_by） */
  async sharedFactUsedBy(factId: number): Promise<SharedFactUsedByResponse> {
    const resp = await this._fetch(`/api/shared/facts/${factId}/used_by`);
    return (await resp.json()) as SharedFactUsedByResponse;
  }

  /** 关闭客户端（释放资源） */
  async close(): Promise<void> {
    // fetch 无需显式关闭
  }
}
