#![forbid(unsafe_code)]
//! 调试器级控制 —— pause / resume / step（阶段6，第四组）
//!
//! # 设计依据
//!
//! 文档 14 §三 第四组：调试器级可观测性，提供 GDB 风格的 reactor 控制原语。
//! 文档 14 §五 阶段6：依赖阶段3（已交付的 ReactorStateSnapshot + 只读 API）。
//!
//! # 规范合规（tier1-reactor 特别规范）
//!
//! - ✅ 控制流操作（pause/resume/step）是机制，非业务判断
//! - ✅ AtomicBool / AtomicUsize / Notify 是标准同步原语，无业务语义
//! - ✅ 不引入业务术语字符串字面量
//! - ✅ 不修改 TCB（tier0-tcb 完全不动）
//! - ✅ 不影响确定性（控制标志是运行时机制，不进 Fact/FactsLog）
//!
//! # 设计要点
//!
//! ## pause 语义
//!
//! `pause` **只阻塞 Executing 阶段**，不阻塞：
//! - drain command（仍能接收新 Command/PayloadUpdate/IoResponse）
//! - stable 检测（仍能发射 Stable）
//! - I/O 超时扫描（仍能恢复卡死的 I/O）
//! - channel 关闭响应（仍能优雅退出）
//!
//! 这避免了 pause 期间的死锁风险。
//!
//! ## step(n) 语义
//!
//! `step(n)` 设置 n 条指令的执行配额，解除 pause。
//! 每执行一条指令调用 `consume_step`，配额耗尽时自动设 paused=true。
//! 下次 `check_and_wait` 检查点会阻塞，等待下一次 resume/step。
//!
//! ## 防死锁机制
//!
//! `check_and_wait` 用 `tokio::select!` 监听 `resume_signal` + 周期性 timeout。
//! timeout 后返回，让主循环继续走 drain/stable/await io，
//! 确保 channel 关闭、I/O 超时等事件能被处理。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tier0_tcb::path::resolve_path;
use tier0_tcb::JsonValue;
use tokio::sync::Notify;

/// 默认 check_and_wait 超时检查间隔（100ms）
///
/// pause 期间每 100ms 返回一次，让主循环继续走 drain/stable/await io，
/// 确保能响应 channel 关闭与 I/O 超时。
const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// 条件中断类型（阶段10.1问题2）
///
/// 支持在批量执行中根据条件自动中断，通知 LLM 处理异常。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakCondition {
    /// I/O 响应携带错误时中断
    IoError,
    /// payload 中指定路径存在时中断（如 "__io_error__"）
    PayloadPathExists(String),
    /// payload 中指定路径的值等于预期值时中断
    PayloadPathEquals(String, JsonValue),
    /// 执行步数超过阈值时中断
    StepCountExceeded(usize),
}

/// 调试控制状态（共享在 reactor 主循环与 ReactorHandle 之间）
///
/// 内部全部用 `Arc` 包裹，clone 是浅拷贝，ReactorHandle 与 reactor 主循环
/// 各持一份，通过原子操作 + Notify 通信。
///
/// # 阶段10.1问题2扩展：条件中断
///
/// 支持在批量执行中根据条件自动中断：
/// - IoError：I/O 响应携带错误时中断
/// - PayloadPathExists：payload 中指定路径存在时中断
/// - PayloadPathEquals：payload 中指定路径的值等于预期值时中断
/// - StepCountExceeded：执行步数超过阈值时中断
#[derive(Debug, Clone)]
pub struct DebugControl {
    /// pause 标志：true 时 reactor 在 Executing 入口阻塞等待 resume
    paused: Arc<AtomicBool>,
    /// step 配额：> 0 时允许执行 n 步，每步递减，归零后自动 pause
    /// 0 表示"无 step 限制"（resume 后的状态，无限执行直到下次 pause/step）
    step_quota: Arc<AtomicUsize>,
    /// resume 信号：pause 阻塞时等待此信号
    resume_signal: Arc<Notify>,
    /// check_and_wait 的超时检查间隔（防死锁，测试可缩短）
    check_interval: Duration,
    /// 条件中断列表（阶段10.1问题2）
    break_conditions: Arc<Mutex<Vec<BreakCondition>>>,
}

impl Default for DebugControl {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugControl {
    /// 创建默认配置的 DebugControl（check_interval = 100ms）
    pub fn new() -> Self {
        Self::new_with_interval(DEFAULT_CHECK_INTERVAL)
    }

    /// 创建指定 check_interval 的 DebugControl（测试用 10ms 缩短等待）
    pub fn new_with_interval(check_interval: Duration) -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            step_quota: Arc::new(AtomicUsize::new(0)),
            resume_signal: Arc::new(Notify::new()),
            check_interval,
            break_conditions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// handle 端：请求暂停（异步生效，reactor 下次检查点响应）
    ///
    /// 同时清零 step 配额，确保即使之前有未消费的 step 也会暂停。
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
        self.step_quota.store(0, Ordering::Release);
    }

    /// handle 端：恢复执行（解除 pause，无限执行直到下次 pause/step）
    ///
    /// 设置 paused=false，唤醒所有等待者。step_quota 保持 0（表示无限制）。
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
        // step_quota 保持 0（resume 后无限制）
        self.resume_signal.notify_waiters();
    }

    // ========== 阶段10.1问题2：条件中断 API ==========

    /// 添加条件中断
    ///
    /// 当条件满足时，reactor 会自动暂停，等待 LLM 处理。
    pub fn add_break_condition(&self, condition: BreakCondition) {
        if let Ok(mut conditions) = self.break_conditions.lock() {
            conditions.push(condition);
        }
    }

    /// 移除所有条件中断
    pub fn clear_break_conditions(&self) {
        if let Ok(mut conditions) = self.break_conditions.lock() {
            conditions.clear();
        }
    }

    /// 获取当前条件中断列表（克隆）
    pub fn break_conditions(&self) -> Vec<BreakCondition> {
        self.break_conditions
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    /// reactor 端：检查条件中断是否触发
    ///
    /// 执行前检查所有条件，如果任意条件满足则设置 paused=true。
    /// 返回 true 表示条件已触发，reactor 应暂停。
    pub fn check_break_conditions(&self, payload: &JsonValue, steps: usize) -> bool {
        let conditions = match self.break_conditions.lock() {
            Ok(c) => c,
            Err(_) => return false,
        };

        for cond in conditions.iter() {
            if self.condition_matches(cond, payload, steps) {
                self.pause();
                return true;
            }
        }
        false
    }

    /// 标记 I/O 错误（触发 IoError 条件中断）
    ///
    /// IoSubscriber 检测到 I/O 错误时调用此方法。
    pub fn trigger_io_error(&self) {
        self.pause();
    }

    fn condition_matches(&self, cond: &BreakCondition, payload: &JsonValue, steps: usize) -> bool {
        match cond {
            BreakCondition::PayloadPathExists(path) => resolve_path(payload, path).is_some(),
            BreakCondition::PayloadPathEquals(path, expected) => {
                resolve_path(payload, path) == Some(expected)
            }
            BreakCondition::StepCountExceeded(threshold) => steps > *threshold,
            BreakCondition::IoError => false,
        }
    }

    /// handle 端：单步执行 n 条指令后自动暂停
    ///
    /// 设置 step_quota=n，paused=false，唤醒等待者。
    /// reactor 每执行一条指令调用 `consume_step` 递减配额，
    /// 配额耗尽时自动设 paused=true，下次检查点阻塞。
    pub fn step(&self, n: usize) {
        self.step_quota.store(n, Ordering::Release);
        self.paused.store(false, Ordering::Release);
        self.resume_signal.notify_waiters();
    }

    /// reactor 端：在执行点检查是否应暂停，若需要则阻塞等待 resume
    ///
    /// 阻塞条件：`paused == true` 或 `step_quota == 0`（且非初始状态）。
    /// 用 `tokio::select!` 监听 `resume_signal` + 周期性 timeout，
    /// timeout 后返回让主循环继续走 drain/stable/await io，
    /// 确保 channel 关闭、I/O 超时等事件能被处理。
    ///
    /// # 返回值
    ///
    /// - `Ok(())`：可以继续执行（paused=false 且 step_quota>0，或 timeout 返回）
    /// - `Err(())`：锁中毒等异常（理论上不会发生，原子操作无锁）
    pub async fn check_and_wait(&self) {
        loop {
            let paused = self.paused.load(Ordering::Acquire);
            // resume 后：paused=false, quota=0 → 允许执行（无限制）
            // step 后：paused=false, quota>0 → 允许执行（配额内）
            // pause 后：paused=true, quota=0 → 阻塞
            // 配额耗尽：paused=true（consume_step 已设置）→ 阻塞
            if !paused {
                // resume（quota=0 无限制）或 step（quota>0 配额内）都允许
                return;
            }
            // paused=true：等待 resume 信号，或 timeout 后返回让主循环处理其他事件
            tokio::select! {
                _ = self.resume_signal.notified() => {
                    // 被唤醒，重新检查状态（loop 顶部会重新 load）
                    continue;
                }
                _ = tokio::time::sleep(self.check_interval) => {
                    // timeout：返回让主循环继续（drain/stable/await io 会处理其他事件）
                    // 主循环下次进入 Executing 前会再次调用 check_and_wait
                    return;
                }
            }
        }
    }

    /// reactor 端：执行一条指令后递减 step 配额
    ///
    /// 在 `execute_transition` 返回后调用。
    /// - quota == 0：无 step 限制（resume 后的状态），返回 false
    /// - quota == 1：配额耗尽，设 paused=true，返回 true
    /// - quota > 1：递减，返回 false
    ///
    /// 返回 true 表示配额刚耗尽，下次 `check_and_wait` 检查点会阻塞。
    pub fn consume_step(&self) -> bool {
        let quota = self.step_quota.load(Ordering::Acquire);
        if quota == 0 {
            // resume 后的无限制模式，不递减
            return false;
        }
        if quota == 1 {
            // 配额耗尽，自动 pause
            self.paused.store(true, Ordering::Release);
            self.step_quota.store(0, Ordering::Release);
            return true;
        }
        // 配额 > 1，递减
        self.step_quota.store(quota - 1, Ordering::Release);
        false
    }

    /// 当前是否暂停（供 ReactorHandle 查询，也用于 snapshot 填充）
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    /// 当前 step 配额（供 ReactorHandle 查询，也用于 snapshot 填充）
    pub fn step_quota(&self) -> usize {
        self.step_quota.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_default_not_paused() {
        let ctrl = DebugControl::new();
        assert!(!ctrl.is_paused());
        assert_eq!(ctrl.step_quota(), 0);
    }

    #[test]
    fn test_pause_sets_paused_flag() {
        let ctrl = DebugControl::new();
        ctrl.pause();
        assert!(ctrl.is_paused());
        assert_eq!(ctrl.step_quota(), 0);
    }

    #[test]
    fn test_pause_clears_step_quota() {
        let ctrl = DebugControl::new();
        ctrl.step(10);
        assert_eq!(ctrl.step_quota(), 10);
        ctrl.pause();
        assert!(ctrl.is_paused());
        assert_eq!(ctrl.step_quota(), 0);
    }

    #[test]
    fn test_resume_clears_paused_flag() {
        let ctrl = DebugControl::new();
        ctrl.pause();
        assert!(ctrl.is_paused());
        ctrl.resume();
        assert!(!ctrl.is_paused());
        // resume 后 step_quota 保持 0（表示无限制）
        assert_eq!(ctrl.step_quota(), 0);
    }

    #[test]
    fn test_step_sets_quota_and_clears_pause() {
        let ctrl = DebugControl::new();
        ctrl.pause();
        assert!(ctrl.is_paused());
        ctrl.step(5);
        assert!(!ctrl.is_paused());
        assert_eq!(ctrl.step_quota(), 5);
    }

    #[test]
    fn test_consume_step_no_limit_when_quota_zero() {
        // resume 后 quota=0，consume_step 不递减，返回 false
        let ctrl = DebugControl::new();
        ctrl.resume();
        assert_eq!(ctrl.step_quota(), 0);
        assert!(!ctrl.consume_step());
        assert_eq!(ctrl.step_quota(), 0); // 未递减
        assert!(!ctrl.is_paused());
    }

    #[test]
    fn test_consume_step_decrements_quota() {
        let ctrl = DebugControl::new();
        ctrl.step(3);
        assert_eq!(ctrl.step_quota(), 3);

        assert!(!ctrl.consume_step()); // 3 -> 2
        assert_eq!(ctrl.step_quota(), 2);

        assert!(!ctrl.consume_step()); // 2 -> 1
        assert_eq!(ctrl.step_quota(), 1);

        // 1 -> 0，配额耗尽，自动 pause
        assert!(ctrl.consume_step());
        assert_eq!(ctrl.step_quota(), 0);
        assert!(ctrl.is_paused());
    }

    #[test]
    fn test_consume_step_after_pause_does_not_decrement() {
        // pause 后 quota=0，consume_step 不递减
        let ctrl = DebugControl::new();
        ctrl.step(5);
        ctrl.pause();
        assert_eq!(ctrl.step_quota(), 0);
        assert!(!ctrl.consume_step()); // quota=0，无限制模式语义，返回 false
        assert_eq!(ctrl.step_quota(), 0);
        assert!(ctrl.is_paused());
    }

    #[tokio::test]
    async fn test_check_and_wait_returns_immediately_when_not_paused() {
        // 默认未 pause，check_and_wait 应立即返回
        let ctrl = DebugControl::new();
        let result = tokio::time::timeout(Duration::from_millis(50), ctrl.check_and_wait()).await;
        assert!(
            result.is_ok(),
            "check_and_wait should return immediately when not paused"
        );
    }

    #[tokio::test]
    async fn test_check_and_wait_returns_immediately_after_resume() {
        let ctrl = DebugControl::new();
        ctrl.pause();
        ctrl.resume();
        let result = tokio::time::timeout(Duration::from_millis(50), ctrl.check_and_wait()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_wait_returns_after_timeout_when_paused() {
        // pause 后 check_and_wait 应在 check_interval 后 timeout 返回
        let ctrl = DebugControl::new_with_interval(Duration::from_millis(20));
        ctrl.pause();
        let start = std::time::Instant::now();
        ctrl.check_and_wait().await;
        let elapsed = start.elapsed();
        // 应该在 20ms 左右返回（允许一定误差）
        assert!(
            elapsed >= Duration::from_millis(15),
            "check_and_wait should wait at least check_interval, got {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "check_and_wait should not wait too long, got {:?}",
            elapsed
        );
        // pause 状态应保持（timeout 返回不改变 paused）
        assert!(ctrl.is_paused());
    }

    #[tokio::test]
    async fn test_check_and_wait_returns_when_resume_signal_arrives() {
        // pause 后，另一任务发 resume，check_and_wait 应被唤醒
        let ctrl = DebugControl::new_with_interval(Duration::from_secs(10));
        ctrl.pause();
        let ctrl_clone = ctrl.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            ctrl_clone.resume();
        });
        let start = std::time::Instant::now();
        ctrl.check_and_wait().await;
        let elapsed = start.elapsed();
        // 应该在 30ms 左右被唤醒（远小于 10s timeout）
        assert!(
            elapsed < Duration::from_secs(5),
            "check_and_wait should be woken by resume signal, got {:?}",
            elapsed
        );
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_check_and_wait_returns_when_step_arrives() {
        let ctrl = DebugControl::new_with_interval(Duration::from_secs(10));
        ctrl.pause();
        let ctrl_clone = ctrl.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            ctrl_clone.step(5);
        });
        ctrl.check_and_wait().await;
        // step 后应能继续
        assert!(!ctrl.is_paused());
        assert_eq!(ctrl.step_quota(), 5);
        handle.await.unwrap();
    }

    #[test]
    fn test_clone_shares_state() {
        // clone 后两个实例共享同一份状态
        let ctrl1 = DebugControl::new();
        let ctrl2 = ctrl1.clone();
        ctrl1.pause();
        assert!(ctrl2.is_paused(), "clone should share paused state");
        ctrl2.resume();
        assert!(!ctrl1.is_paused(), "clone should share resume");
    }

    #[tokio::test]
    async fn test_full_pause_step_resume_cycle() {
        // 模拟完整的 pause -> step(2) -> 执行 2 步后自动 pause -> resume 流程
        let ctrl = DebugControl::new_with_interval(Duration::from_millis(10));

        // 1. 初始未暂停
        assert!(!ctrl.is_paused());

        // 2. pause
        ctrl.pause();
        assert!(ctrl.is_paused());

        // 3. step(2)
        ctrl.step(2);
        assert!(!ctrl.is_paused());
        assert_eq!(ctrl.step_quota(), 2);

        // 4. 执行第 1 步：consume_step 递减，配额 2 -> 1
        let exhausted = ctrl.consume_step();
        assert!(!exhausted);
        assert_eq!(ctrl.step_quota(), 1);
        assert!(!ctrl.is_paused());

        // 5. 执行第 2 步：配额 1 -> 0，自动 pause
        let exhausted = ctrl.consume_step();
        assert!(exhausted);
        assert_eq!(ctrl.step_quota(), 0);
        assert!(ctrl.is_paused());

        // 6. resume，继续无限执行
        ctrl.resume();
        assert!(!ctrl.is_paused());
        assert_eq!(ctrl.step_quota(), 0);
        // resume 后 consume_step 不递减
        assert!(!ctrl.consume_step());
        assert_eq!(ctrl.step_quota(), 0);
    }
}
