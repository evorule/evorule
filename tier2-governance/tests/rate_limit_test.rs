//! 速率限制集成测试（030 文档回归测试）
//!
//! 验证 `--no-rate-limit` 标志的端到端行为：
//! - 测试 A：`per_sec=0` 时，300 次请求全部 200，零 429（限速真正禁用）
//! - 测试 B：`per_sec=1, burst=5` 时，30 次快速请求中出现 429（限速正常生效）
//!
//! 测试策略：构建最小路由（只有 /api/health），复用 `resolve_governor_config`
//! 决定是否加 `GovernorLayer`，用临时端口 + `reqwest` 发真实 HTTP 请求。
//! 不构造完整 `AppState`，聚焦限速行为本身。

use std::net::SocketAddr;

use axum::{routing::get, Router};
use tower_governor::GovernorLayer;

/// 构建带限速配置的最小测试路由
///
/// 复用生产代码的 `resolve_governor_config` 决策函数，
/// 确保测试路径与 `build_router()` 完全一致。
fn build_test_router(per_sec: u64, burst: u32) -> Router {
    let router = Router::new().route("/api/health", get(|| async { "ok" }));

    match tier2_governance::api::server::resolve_governor_config(per_sec, burst) {
        None => router.with_state(()),
        Some(cfg) => router.layer(GovernorLayer::new(cfg)).with_state(()),
    }
}

/// 启动测试服务器，返回 (地址, 关闭句柄)
///
/// 绑定 `127.0.0.1:0` 获取临时端口，避免端口冲突。
/// 返回的 `JoinHandle` 可用 `.abort()` 停止服务器。
async fn spawn_test_server(per_sec: u64, burst: u32) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let router = build_test_router(per_sec, burst);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });
    (addr, handle)
}

/// 发送 N 次快速请求，统计各状态码出现次数
async fn burst_requests(addr: SocketAddr, count: usize) -> std::collections::HashMap<u16, usize> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://{addr}/api/health");
    let mut stats = std::collections::HashMap::new();

    for _ in 0..count {
        let status = client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().as_u16())
            .unwrap_or(0);
        *stats.entry(status).or_insert(0) += 1;
    }
    stats
}

#[tokio::test]
async fn test_no_rate_limit_allows_burst() {
    // 场景：--no-rate-limit → per_sec=0
    // 期望：300 次请求全部 200，零 429
    // 历史：旧代码在此场景下第 386 次起触发 429（详见 030 文档）
    let (addr, handle) = spawn_test_server(0, 200).await;

    let stats = burst_requests(addr, 300).await;
    let ok = *stats.get(&200).unwrap_or(&0);
    let too_many = *stats.get(&429).unwrap_or(&0);

    assert_eq!(
        ok, 300,
        "per_sec=0 时所有请求应返回 200，实际 200 仅 {ok} 次"
    );
    assert_eq!(
        too_many, 0,
        "per_sec=0 时不应出现 429，实际出现 {too_many} 次（限速未真正禁用）"
    );

    handle.abort();
}

#[tokio::test]
async fn test_rate_limit_enforced() {
    // 场景：正常模式 per_sec=1, burst=5
    // 期望：30 次快速请求中至少出现 1 次 429（证明 GovernorLayer 已添加且生效）
    //       且至少 1 次 200（证明有请求成功通过）
    // 不断言精确数量，因为令牌桶时序可能导致边界值波动
    let (addr, handle) = spawn_test_server(1, 5).await;

    let stats = burst_requests(addr, 30).await;
    let ok = *stats.get(&200).unwrap_or(&0);
    let too_many = *stats.get(&429).unwrap_or(&0);

    assert!(
        ok >= 1,
        "应至少有 1 次请求成功（burst=5 初始令牌），实际 200 仅 {ok} 次"
    );
    assert!(
        too_many >= 1,
        "应至少出现 1 次 429（burst 耗尽后限速），实际 429 仅 {too_many} 次——限速可能未生效"
    );

    handle.abort();
}
