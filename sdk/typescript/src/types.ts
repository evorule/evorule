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

/** 命令提交失败 */
export class CommandError extends EvoruleError {
  constructor(message: string) {
    super(message);
    this.name = "CommandError";
  }
}
