/**
 * evorule SDK 类型定义
 */

/** 通用 JSON 值类型 */
export type Json =
  | null
  | boolean
  | number
  | string
  | Json[]
  | { [key: string]: Json };

/** 指令 JSON（业务规则指令） */
export interface Instruction {
  type: string;
  params?: Record<string, Json>;
  [key: string]: Json | undefined;
}

/** API 响应 */
export interface ApiResponse {
  success: boolean;
  message: string;
  fact_id: number | null;
}

/** 会话状态快照 */
export interface SessionState {
  payload: Record<string, Json>;
  queue: Json[];
  version: number;
}

/** 创建会话响应 */
export interface CreateSessionResponse {
  session_id: number;
  message: string;
}

/** 列出会话响应 */
export interface ListSessionsResponse {
  sessions: number[];
}

/** Replay 响应（Fact 列表） */
export interface ReplayResponse {
  facts: EventData[];
}

/** Rewind 响应（回滚后的状态快照） */
export interface RewindResponse {
  version: number;
  payload: Record<string, Json>;
  queue: Json[];
}

/** Diff 响应中 added/removed 项 */
export interface DiffEntry {
  key: string;
  value: Json;
}

/** Diff 响应中 changed 项 */
export interface DiffChangedEntry {
  key: string;
  old_value: Json;
  new_value: Json;
}

/** Diff 响应（两个版本的 payload 对比） */
export interface DiffResponse {
  version_a: number;
  version_b: number;
  added: DiffEntry[];
  removed: DiffEntry[];
  changed: DiffChangedEntry[];
}

/** 共享 Fact */
export interface SharedFact {
  fact_id: number;
  path: string;
  value: Json;
  source_session_id: number;
  version: number;
}

/** 挂起的 I/O 请求信息 */
export interface PendingIoInfo {
  fact_id: number;
  io_type: string;
  duration_ms: number;
}

/** debug/phase 响应 */
export interface DebugPhaseResponse {
  phase: string;
}

/** debug/queue 响应 */
export interface DebugQueueResponse {
  queue: Json[];
}

/** debug/pending_io 响应 */
export interface DebugPendingIoResponse {
  pending_io: PendingIoInfo[];
}

/** Fork 会话响应 */
export interface ForkSessionResponse {
  session_id: number;
  parent_session_id: number;
  forked_from_version: number;
  message: string;
}

/** audit/verify 响应 */
export interface AuditVerifyResponse {
  valid: boolean;
  session_id: number;
}

/** history 单项 */
export interface HistoryEntry {
  version: number;
  type: string;
}

/** 会话内 Fact 单项（按前缀查询） */
export interface SessionFactEntry {
  fact_id: number;
  version: number;
  path: string;
  value: Json;
}

/** GET used_at_startup 响应 */
export interface UsedAtStartupResponse {
  session_id: number;
  fact_ids: number[];
}

/** shared_fact_source 响应 */
export interface SharedFactSourceResponse {
  fact_id: number;
  path: string;
  value: Json;
  source_session_id: number;
  version: number;
}

/** shared_fact_used_by 响应 */
export interface SharedFactUsedByResponse {
  fact_id: number;
  sessions: number[];
}

/** cluster_status 响应 */
export interface ClusterStatusResponse {
  session_id: number;
  cluster_members: number[];
}

/** 集群同步方向 */
export type SyncDirection = "atob" | "btoa" | "bidirectional";

/** 事件类型枚举 */
export type EventType =
  | "Command"
  | "StateTransition"
  | "IoRequest"
  | "IoResponse"
  | "Stable"
  | "PayloadUpdate"
  | "Error";

/** SSE 事件原始 JSON */
export interface EventData {
  type: EventType;
  id: number;
  cause?: number;
  instruction?: Json;
  new_payload?: Record<string, Json>;
  new_queue?: Json[];
  final_snapshot?: Record<string, Json>;
  io_type?: string;
  params?: Record<string, Json>;
  request_id?: number;
  result?: Json;
  error?: string;
  path?: string;
  value?: Json;
  message?: string;
  [key: string]: Json | undefined;
}

/** 客户端配置 */
export interface ClientOptions {
  /** Bearer 认证 token */
  token?: string;
  /** 请求超时（毫秒），默认 30000 */
  timeout?: number;
}

// ===== 异常类 =====

/** evorule SDK 基础异常 */
export class EvoruleError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EvoruleError";
  }
}

/** 认证失败（HTTP 401） */
export class AuthenticationError extends EvoruleError {
  constructor(message: string) {
    super(message);
    this.name = "AuthenticationError";
  }
}

/** 会话不存在（HTTP 404） */
export class SessionNotFoundError extends EvoruleError {
  constructor(message: string) {
    super(message);
    this.name = "SessionNotFoundError";
  }
}

/** 会话已在客户端关闭 */
export class SessionClosedError extends EvoruleError {
  constructor(message: string) {
    super(message);
    this.name = "SessionClosedError";
  }
}

/** 命令提交失败 */
export class CommandError extends EvoruleError {
  constructor(message: string) {
    super(message);
    this.name = "CommandError";
  }
}
