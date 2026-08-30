// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
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

use std::sync::Arc;
use std::time::Duration;

use evorule_reactor::{EventReceiver, Fact, FactId, FactSender, IoType};
use evorule_tcb::JsonValue;

use crate::io_dispatcher::IoDispatcher;
use crate::metrics::{NoOpMetrics, SharedMetrics};

/// ID 起始偏移量，避免与反应器自身的 FactId 冲突
const ID_OFFSET: u64 = 10000;

/// 最大重试次数（P0-2：瞬时错误指数退避重试，最多 3 次，即总计 4 次尝试）
const MAX_RETRIES: u32 = 3;
/// 初始退避时长（P0-2：指数退避 200ms → 400ms → 800ms）
const INITIAL_BACKOFF: Duration = Duration::from_millis(200);

/// 判断 I/O 错误是否可重试（P0-2）
///
/// 仅瞬时错误重试：超时、连接问题、HTTP 5xx 服务端错误。
/// 客户端错误（4xx、参数缺失、工具未找到）不重试——重试不会改变结果。
fn is_retryable_error(err: &str) -> bool {
    let lower = err.to_lowercase();
    // 超时（reqwest tokio timeout、db timeout 等）
    if lower.contains("timeout") || lower.contains("timed out") {
        return true;
    }
    // 连接问题（connection refused / reset / closed）
    if lower.contains("connection") {
        return true;
    }
    // 瞬时服务端错误
    if lower.contains("temporarily") || lower.contains("temporary") {
        return true;
    }
    // HTTP 5xx 服务端错误
    for code in ["500", "502", "503", "504"] {
        if lower.contains(code) {
            return true;
        }
    }
    false
}

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

/// 跳过谓词：返回 `true` 时该 IoRequest **不由本订阅者自动应答**，
/// 留给外部执行者（如审计桥的浏览器/agent 侧本地 LLM 执行）处理。
pub type SkipPredicate = Arc<dyn Fn(&IoType, &JsonValue) -> bool + Send + Sync>;

/// I/O 订阅者
///
/// 订阅反应器的 event broadcast 通道，过滤出 `Fact::IoRequest`，交由
/// `IoDispatcher` 执行实际 I/O，并通过 command 通道回写 `Fact::IoResponse`。
///
/// # 设计要点
/// - 持有自己的 ID 计数器（从 `10000` 起），与反应器 ID 隔离
/// - 对 `Lagged` 容错（记录日志并继续），不中断订阅循环
/// - 非阻塞处理：忽略非 `IoRequest` 类型 Fact，不影响其他订阅者
/// - 可选 `skip` 谓词：命中时**不自动应答**（不回写任何 IoResponse），
///   留给外部执行者处理。用于 LLM 审计形态的 `call_external`（prompt 全文
///   经命令事实入审计链，结果由外部执行者经 `submit_io_response` 回写）——
///   若本订阅者抢先以"missing service_name"错误应答，外部执行者的
///   io_response 将被反应器忽略（Unknown IoResponse），审计回路永远失败
///
/// # 示例
/// ```ignore
/// use evorule_governance::{IoDispatcher, IoSubscriber};
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
    /// I/O 指标收集器（默认 NoOpMetrics，应用层可通过 `with_metrics()` 注入 Prometheus 实现）
    metrics: SharedMetrics,
    /// 可选跳过谓词（默认 None = 全部自动应答，行为与历史版本一致）
    skip: Option<SkipPredicate>,
}

impl IoSubscriber {
    /// 创建新的订阅者
    ///
    /// ID 计数器从 `10000` 起，避免与反应器的 `FactIdGenerator` 冲突。
    /// 默认使用 `NoOpMetrics`（不收集指标），应用层可通过 `with_metrics()` 注入实现。
    pub fn new(dispatcher: IoDispatcher) -> Self {
        Self {
            dispatcher,
            next_id: ID_OFFSET,
            metrics: Arc::new(NoOpMetrics),
            skip: None,
        }
    }

    /// 注入跳过谓词（builder 模式）
    ///
    /// 谓词命中的 IoRequest **不自动应答**（不回写任何 IoResponse），
    /// 留给外部执行者（审计桥）处理。
    pub fn with_skip(mut self, predicate: SkipPredicate) -> Self {
        self.skip = Some(predicate);
        self
    }

    /// 注入指标收集器（builder 模式）
    ///
    /// 注入后，`dispatch_and_respond` 将通过 trait object 分发调用具体的指标记录方法。
    /// 应用层通常注入 `PrometheusMetrics` 实现。
    pub fn with_metrics(mut self, metrics: SharedMetrics) -> Self {
        self.metrics = metrics;
        self
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
                // 跳过谓词命中 → 不自动应答，留给外部执行者（审计桥）。
                // 必须在 dispatch 之前判断：一旦回写（哪怕错误），外部执行者的
                // io_response 会被反应器忽略（Unknown IoResponse）。
                if let Some(skip) = &self.skip {
                    if skip(&io_type, &params) {
                        tracing::trace!(
                            fact_id = %id,
                            io_type = %io_type,
                            "IoSubscriber 命中跳过谓词，IoRequest 留待外部执行者应答"
                        );
                        return Ok(());
                    }
                }
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
    ///
    /// # P0-2 重试策略
    /// 对瞬时错误（超时/连接/5xx）执行指数退避重试，最多 `MAX_RETRIES` 次：
    /// 200ms → 400ms → 800ms。客户端错误（参数缺失/4xx/工具未找到）不重试。
    /// 重试耗尽后回写最终错误 `IoResponse`，让反应器恢复而非永久阻塞。
    // IO dispatch + 重试 + 错误回写多分支, 拆函数需共享 self/cmd 状态。详见 GATE_REFERENCE.md §六(豁免索引)
    #[allow(clippy::cognitive_complexity)]
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

        // 记录整体 I/O 耗时（包含重试）。通过 trait object 分发，
        // 默认 NoOpMetrics 空转，应用层注入的 PrometheusMetrics 会实际记录。
        let overall_start = std::time::Instant::now();
        let io_type_str: &str = io_type.as_str();
        let mut had_error = false;

        let mut attempt: u32 = 0;
        let response = loop {
            attempt += 1;
            match self.dispatcher.dispatch(&io_type, &params).await {
                Ok(result) => {
                    if attempt > 1 {
                        tracing::info!(
                            request_id = %request_id,
                            attempt,
                            "IoRequest 在重试后执行成功"
                        );
                    } else {
                        tracing::info!(
                            request_id = %request_id,
                            "IoRequest 执行成功，回写 IoResponse"
                        );
                    }
                    break Fact::IoResponse {
                        id: self.next_fact_id(),
                        request_id,
                        result,
                        error: None,
                    };
                }
                Err(err_msg) => {
                    // P0-2：瞬时错误指数退避重试
                    if attempt <= MAX_RETRIES && is_retryable_error(&err_msg) {
                        let backoff =
                            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(attempt - 1));
                        tracing::warn!(
                            request_id = %request_id,
                            attempt,
                            max_attempts = MAX_RETRIES + 1,
                            backoff_ms = backoff.as_millis() as u64,
                            error = %err_msg,
                            "IoRequest 瞬时错误，指数退避重试"
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    tracing::warn!(
                        request_id = %request_id,
                        attempt,
                        error = %err_msg,
                        "IoRequest 执行失败（最终），回写错误 IoResponse"
                    );
                    had_error = true;
                    break Fact::IoResponse {
                        id: self.next_fact_id(),
                        request_id,
                        result: JsonValue::Null,
                        error: Some(err_msg),
                    };
                }
            }
        };

        // 通过 trait object 分发记录指标（NoOpMetrics 空转，PrometheusMetrics 实际记录）
        self.metrics
            .observe_io_duration(io_type_str, overall_start.elapsed());
        if had_error {
            self.metrics.inc_io_errors(io_type_str);
        }

        command_tx
            .send(response)
            .map_err(|_| IoSubscriberError::CommandClosed(format!("request_id={request_id}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;
    use evorule_reactor::IoType;

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
        let t = IoType::call_external();
        assert_eq!(t.as_str(), "call_external");
    }

    #[test]
    fn test_is_retryable_error_timeout() {
        assert!(is_retryable_error(
            "http request failed: operation timed out"
        ));
        assert!(is_retryable_error("db query timed out after 5s"));
        assert!(is_retryable_error("request timeout"));
    }

    #[test]
    fn test_is_retryable_error_connection() {
        assert!(is_retryable_error("connection refused"));
        assert!(is_retryable_error("connection reset by peer"));
        assert!(is_retryable_error("Connection closed"));
    }

    #[test]
    fn test_is_retryable_error_http_5xx() {
        assert!(is_retryable_error(
            "LLM API returned 503: service unavailable"
        ));
        assert!(is_retryable_error("http request failed with status: 500"));
        assert!(is_retryable_error("gateway 502 bad gateway"));
    }

    #[test]
    fn test_is_retryable_error_temporary() {
        assert!(is_retryable_error("service temporarily unavailable"));
        assert!(is_retryable_error("Temporary failure in name resolution"));
    }

    #[test]
    fn test_is_not_retryable_error_client_errors() {
        // 参数缺失——bug，重试无意义
        assert!(!is_retryable_error("missing required param: prompt"));
        // 工具未找到——配置问题
        assert!(!is_retryable_error("tool not found: foo"));
        // HTTP 4xx 客户端错误
        assert!(!is_retryable_error("LLM API returned 401: unauthorized"));
        assert!(!is_retryable_error("http request failed with status: 404"));
        assert!(!is_retryable_error("bad request 400"));
    }

    #[test]
    fn test_retry_constants() {
        // 验证 P0-2 重试配置
        assert_eq!(MAX_RETRIES, 3);
        assert_eq!(INITIAL_BACKOFF, Duration::from_millis(200));
        // 退避序列：200ms, 400ms, 800ms
        assert_eq!(
            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(0)),
            Duration::from_millis(200)
        );
        assert_eq!(
            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(1)),
            Duration::from_millis(400)
        );
        assert_eq!(
            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(2)),
            Duration::from_millis(800)
        );
    }

    // ===== skip 谓词（审计桥 2026-08-30）：命中时不自动应答 =====

    /// 构造 LLM 审计形态的 call_external params（有 messages、无 service_name）
    fn llm_audit_params() -> JsonValue {
        JsonValue::object_from_pairs(&[(
            "messages",
            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                ("role", JsonValue::string("user")),
                ("content", JsonValue::string("hi")),
            ])]),
        )])
    }

    /// skip 命中 → handle_fact 返回 Ok 且**不回写任何 IoResponse**（留给外部执行者）
    #[tokio::test]
    async fn test_skip_predicate_leaves_io_request_unanswered() {
        // 空 dispatcher：若未跳过而走 dispatch，必然回写错误 IoResponse
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build()).with_skip(Arc::new(
            |io_type: &IoType, params: &JsonValue| {
                io_type.as_str() == "call_external"
                    && params.get("messages").is_some()
                    && params.get("service_name").is_none()
                    && params.get("name").is_none()
            },
        ));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let fact = Fact::IoRequest {
            id: FactId(1),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params: llm_audit_params(),
        };

        let result = subscriber.handle_fact(fact, &tx).await;
        assert!(result.is_ok());
        // 关键断言：command 通道无任何 IoResponse（外部执行者的 io_response 不会被抢答）
        assert!(
            rx.try_recv().is_err(),
            "skip 命中时不应回写任何 IoResponse"
        );
    }

    /// 对照组：默认（无 skip）→ dispatch 失败 → 回写错误 IoResponse（历史行为不变）
    #[tokio::test]
    async fn test_without_skip_error_response_is_written() {
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build());

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let fact = Fact::IoRequest {
            id: FactId(2),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params: llm_audit_params(),
        };

        let result = subscriber.handle_fact(fact, &tx).await;
        assert!(result.is_ok());
        // 空 dispatcher → dispatch Err → 错误 IoResponse 回写
        match rx.try_recv() {
            Ok(Fact::IoResponse { request_id, error, .. }) => {
                assert_eq!(request_id, FactId(2));
                assert!(error.is_some(), "未注册类型应回写错误 IoResponse");
            }
            other => panic!("应回写错误 IoResponse，实际: {other:?}"),
        }
    }

    /// 谓词只命中 LLM 审计形态：带 service_name 的 call_external 不跳过（照常分发）
    #[tokio::test]
    async fn test_skip_predicate_does_not_hit_service_calls() {
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build()).with_skip(Arc::new(
            |io_type: &IoType, params: &JsonValue| {
                io_type.as_str() == "call_external"
                    && params.get("messages").is_some()
                    && params.get("service_name").is_none()
                    && params.get("name").is_none()
            },
        ));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        // service 调用形态：service_name 存在、无 messages
        let params = JsonValue::object_from_pairs(&[(
            "service_name",
            JsonValue::string("inverse_kinematics_solver"),
        )]);
        let fact = Fact::IoRequest {
            id: FactId(3),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params,
        };

        let result = subscriber.handle_fact(fact, &tx).await;
        assert!(result.is_ok());
        // 未跳过 → dispatch（空 dispatcher）→ 错误 IoResponse 回写
        assert!(
            rx.try_recv().is_ok(),
            "带 service_name 的调用不应被跳过，应照常分发并回写"
        );
    }
}
