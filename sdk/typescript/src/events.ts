/**
 * evorule SDK SSE 事件类
 *
 * 对应服务端 fact_to_sse_data() 序列化的 7 种 Fact 变体。
 */

import type { EventData, EventType } from "./types.js";

/** SSE 事件 */
export class Event {
  /** 事件类型 */
  readonly type: EventType;
  /** 事件 ID */
  readonly id: number;
  /** 完整事件 JSON 数据 */
  readonly raw: Readonly<EventData>;

  private constructor(data: EventData) {
    this.type = data.type;
    this.id = data.id;
    this.raw = Object.freeze({ ...data }) as Readonly<EventData>;
  }

  /** 从 JSON 对象构造 Event */
  static fromJson(data: Record<string, unknown>): Event {
    return new Event(data as unknown as EventData);
  }

  // ===== 便捷属性 =====

  /** StateTransition / IoRequest 的触发源 FactId */
  get cause(): number | undefined {
    return this.raw.cause;
  }

  /** Command 事件携带的指令 */
  get instruction(): unknown {
    return this.raw.instruction;
  }

  /** StateTransition 事件执行后的 payload 快照 */
  get newPayload(): Record<string, unknown> | undefined {
    return this.raw.new_payload;
  }

  /** StateTransition 事件执行后的队列快照 */
  get newQueue(): unknown[] | undefined {
    return this.raw.new_queue;
  }

  /** Stable 事件的稳定状态快照 */
  get finalSnapshot(): Record<string, unknown> | undefined {
    return this.raw.final_snapshot;
  }

  /** IoRequest 事件的 I/O 类型 */
  get ioType(): string | undefined {
    return this.raw.io_type;
  }

  /** IoRequest 事件的参数 */
  get params(): Record<string, unknown> | undefined {
    return this.raw.params;
  }

  /** IoResponse 事件对应的 IoRequest ID */
  get requestId(): number | undefined {
    return this.raw.request_id;
  }

  /** IoResponse 事件的 I/O 结果 */
  get result(): unknown {
    return this.raw.result;
  }

  /** IoResponse / Error 事件的错误信息 */
  get error(): string | undefined {
    return this.raw.error ?? this.raw.message;
  }

  /** PayloadUpdate 事件的字段路径 */
  get path(): string | undefined {
    return this.raw.path;
  }

  /** PayloadUpdate 事件的字段值 */
  get value(): unknown {
    return this.raw.value;
  }

  /** Error 事件的错误消息 */
  get message(): string | undefined {
    return this.raw.message;
  }

  toString(): string {
    const parts: string[] = [];
    if (this.cause !== undefined) parts.push(`cause=${this.cause}`);
    if (this.ioType !== undefined) parts.push(`io_type=${this.ioType}`);
    if (this.error !== undefined) parts.push(`error=${this.error}`);
    const suffix = parts.length > 0 ? `, ${parts.join(", ")}` : "";
    return `${this.type}(id=${this.id}${suffix})`;
  }
}
