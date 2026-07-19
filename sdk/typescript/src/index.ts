/**
 * evorule TypeScript SDK
 *
 * evorule-server 的 HTTP API 薄封装，提供会话管理、命令提交和 SSE 事件流订阅。
 *
 * 安装：
 * ```bash
 * npm install @evorule/sdk
 * ```
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

export { EvoruleClient } from "./client.js";
export { Session } from "./session.js";
export { Event } from "./events.js";
export {
  type Json,
  type Instruction,
  type ApiResponse,
  type SessionState,
  type CreateSessionResponse,
  type ListSessionsResponse,
  type ReplayResponse,
  type RewindResponse,
  type DiffResponse,
  type DiffEntry,
  type DiffChangedEntry,
  type SharedFact,
  type SharedFactSourceResponse,
  type SharedFactUsedByResponse,
  type PendingIoInfo,
  type DebugPhaseResponse,
  type DebugQueueResponse,
  type DebugPendingIoResponse,
  type ForkSessionResponse,
  type AuditVerifyResponse,
  type HistoryEntry,
  type SessionFactEntry,
  type UsedAtStartupResponse,
  type ClusterStatusResponse,
  type SyncDirection,
  type EventType,
  type EventData,
  type ClientOptions,
  EvoruleError,
  AuthenticationError,
  SessionNotFoundError,
  SessionClosedError,
  CommandError,
} from "./types.js";

export const VERSION = "6.0.0";
