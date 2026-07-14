//! I/O 订阅者 - 订阅 event broadcast 通道，过滤 IoRequest 事实，执行 I/O，回写 IoResponse
//!
//! # 工作流程
//! 1. 从 EventReceiver 接收 Fact
//! 2. 过滤 `Fact::IoRequest { id, io_type, params, .. }`
//! 3. 调用 `IoDispatcher::dispatch` 执行实际 I/O
//! 4. 通过 FactSender 发送 `Fact::IoResponse { id, request_id, result, error }` 回反应器
//! 5. 其他 Fact 类型忽略（不阻塞，继续接收）
//!
//! # ID 分配策略
//! `IoSubscriber` 维护独立的 ID 计数器，从 `10000` 起步，避免与反应器自身的
//! `FactIdGenerator`（从 1 开始）发生 ID 冲突，便于审计追踪时区分事实来源。
//!
//! # 错误处理
//! - `RecvError::Lagged`：订阅者落后于 broadcast 通道容量，记录 `warn` 并继续
//! - `RecvError::Closed`：通道已关闭，正常退出循环
//! - `SendError`：command 通道关闭（反应器已退出），返回 `IoSubscriberError::CommandClosed`

use tier0_tcb::JsonValue;
use tier1_reactor::{EventReceiver, Fact, FactId, FactSender, IoType};

use crate::io_dispatcher::IoDispatcher;

/// ID 起始偏移量，避免与反应器自身的 FactId 冲突
const ID_OFFSET: u64 = 10000;

/// I/O 订阅者错误
#[derive(Debug, thiserror::Error)]
pub enum IoSubscriberError {
    /// Event broadcast 通道关闭（所有发送端已释放）
    ///
    /// 此错误在 `run()` 内部已被处理为正常退出（`Ok(())`），保留枚举变体以供
    /// 调用方在自定义流程中显式表达该语义。
    #[error("Event channel closed")]
    ChannelClosed,

    /// Command 通道关闭：反应器已退出，无法回写 `IoResponse`
    ///
    /// 携带错误上下文描述（如正在处理的 request_id）。
    #[error("Command channel closed: {0}")]
    CommandClosed(String),
}

/// I/O 订阅者
///
/// 订阅反应器的 event broadcast 通道，过滤出 `Fact::IoRequest`，交由
/// `IoDispatcher` 执行实际 I/O，并通过 command 通道回写 `Fact::IoResponse`。
///
/// # 设计要点
/// - 持有自己的 ID 计数器（从 `10000` 起），与反应器 ID 隔离
/// - 对 `Lagged` 容错（记录日志并继续），不中断订阅循环
/// - 非阻塞处理：忽略非 `IoRequest` 类型 Fact，不影响其他订阅者
///
/// # 示例
/// ```ignore
/// use tier2_governance::{IoDispatcher, IoSubscriber};
/// // dispatcher 由具体 handler 构造
/// let dispatcher: IoDispatcher = /* ... */;
/// let subscriber = IoSubscriber::new(dispatcher);
/// let event_rx = event_tx.subscribe();
/// subscriber.run(event_rx, command_tx).await.ok();
/// ```
pub struct IoSubscriber {
    /// I/O 分发器
    dispatcher: IoDispatcher,
    /// 下一个 FactId（独立计数器，从 10000 起）
    next_id: u64,
}

impl IoSubscriber {
    /// 创建新的订阅者
    ///
    /// ID 计数器从 `10000` 起，避免与反应器的 `FactIdGenerator` 冲突。
    pub fn new(dispatcher: IoDispatcher) -> Self {
        Self {
            dispatcher,
            next_id: ID_OFFSET,
        }
    }

    /// 生成下一个 FactId 并推进计数器
    fn next_fact_id(&mut self) -> FactId {
        let id = FactId(self.next_id);
        self.next_id += 1;
        id
    }

    /// 启动订阅循环
    ///
    /// # 参数
    /// - `event_rx`：event broadcast 通道接收端（可通过 `event_tx.subscribe()` 创建）
    /// - `command_tx`：command 通道发送端（用于回写 `IoResponse`）
    ///
    /// # 返回
    /// - `Ok(())`：通道正常关闭，订阅循环结束
    /// - `Err(IoSubscriberError::CommandClosed)`：反应器已退出，无法回写 `IoResponse`
    ///
    /// # 行为
    /// - 接收 `Fact::IoRequest` → 调度执行 → 回写 `Fact::IoResponse`
    /// - 忽略其他 Fact 类型（仅记录 `trace` 日志）
    /// - `RecvError::Lagged` 记录 `warn` 并继续，不中断循环
    /// - `RecvError::Closed` 视为正常结束，返回 `Ok(())`
    pub async fn run(
        mut self,
        mut event_rx: EventReceiver,
        command_tx: FactSender,
    ) -> Result<(), IoSubscriberError> {
        tracing::info!(
            id_offset = ID_OFFSET,
            "IoSubscriber 启动，开始订阅 event broadcast 通道"
        );

        loop {
            match event_rx.recv().await {
                Ok(fact) => {
                    self.handle_fact(fact, &command_tx).await?;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        skipped = n,
                        "IoSubscriber 落后于 event 通道，已跳过 {} 条 Fact",
                        n
                    );
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Event 通道已关闭，IoSubscriber 正常退出");
                    return Ok(());
                }
            }
        }
    }

    /// 处理单个 Fact：若是 `IoRequest` 则调度执行并回写 `IoResponse`，否则忽略。
    ///
    /// 发送失败（反应器已退出）时返回 `CommandClosed` 错误。
    async fn handle_fact(
        &mut self,
        fact: Fact,
        command_tx: &FactSender,
    ) -> Result<(), IoSubscriberError> {
        match fact {
            Fact::IoRequest {
                id,
                io_type,
                params,
                ..
            } => {
                self.dispatch_and_respond(id, io_type, params, command_tx)
                    .await
            }
            other => {
                tracing::trace!(
                    fact_id = %other.id(),
                    fact_type = other.type_name(),
                    "IoSubscriber 忽略非 IoRequest 事实"
                );
                Ok(())
            }
        }
    }

    /// 执行 I/O 调度并回写 `IoResponse`
    ///
    /// - 成功：`result = JsonValue`，`error = None`
    /// - 失败：`result = JsonValue::Null`，`error = Some(msg)`
    async fn dispatch_and_respond(
        &mut self,
        request_id: FactId,
        io_type: IoType,
        params: JsonValue,
        command_tx: &FactSender,
    ) -> Result<(), IoSubscriberError> {
        tracing::info!(
            request_id = %request_id,
            io_type = %io_type,
            "处理 IoRequest"
        );

        let dispatch_result = self.dispatcher.dispatch(&io_type, &params).await;

        let response = match dispatch_result {
            Ok(result) => {
                tracing::info!(
                    request_id = %request_id,
                    "IoRequest 执行成功，回写 IoResponse"
                );
                Fact::IoResponse {
                    id: self.next_fact_id(),
                    request_id,
                    result,
                    error: None,
                }
            }
            Err(err_msg) => {
                tracing::warn!(
                    request_id = %request_id,
                    error = %err_msg,
                    "IoRequest 执行失败，回写错误 IoResponse"
                );
                Fact::IoResponse {
                    id: self.next_fact_id(),
                    request_id,
                    result: JsonValue::Null,
                    error: Some(err_msg),
                }
            }
        };

        command_tx
            .send(response)
            .map_err(|_| IoSubscriberError::CommandClosed(format!("request_id={request_id}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tier1_reactor::IoType;

    #[test]
    fn test_id_starts_at_offset() {
        // 仅验证 ID 计数器逻辑，不依赖 dispatcher 构造
        // （dispatcher 的实际构造需要 DB / LLM 等外部资源，留给集成测试）
        let next_id = ID_OFFSET;
        assert_eq!(FactId(next_id), FactId(10000));
        assert_eq!(FactId(next_id + 1), FactId(10001));
        assert_eq!(FactId(next_id + 2), FactId(10002));
    }

    #[test]
    fn test_io_subscriber_error_display() {
        let e = IoSubscriberError::ChannelClosed;
        assert_eq!(format!("{e}"), "Event channel closed");

        let e = IoSubscriberError::CommandClosed("request_id=F42".to_string());
        assert_eq!(format!("{e}"), "Command channel closed: request_id=F42");
    }

    #[test]
    fn test_io_type_import_available() {
        // 确保 IoType 在本模块内可见且可使用（防止 import 被意外删除）
        let t = IoType::CallLlm;
        assert_eq!(t.as_str(), "call_llm");
    }
}
