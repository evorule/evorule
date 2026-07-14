//! 认证中间件
//!
//! 提供基于 token 的简单认证，用于 HTTP API 访问控制。

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;

/// 认证配置
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// 合法 token 列表
    valid_tokens: Arc<Vec<String>>,
    /// 是否启用认证（false 时跳过检查）
    enabled: bool,
}

impl AuthConfig {
    /// 创建新认证配置
    ///
    /// - `tokens`：合法 token 列表
    /// - `enabled`：是否启用认证
    pub fn new(tokens: Vec<String>, enabled: bool) -> Self {
        Self {
            valid_tokens: Arc::new(tokens),
            enabled,
        }
    }

    /// 禁用认证（开发模式）
    pub fn disabled() -> Self {
        Self {
            valid_tokens: Arc::new(Vec::new()),
            enabled: false,
        }
    }

    /// 验证 token 是否合法
    pub fn validate(&self, token: &str) -> bool {
        if !self.enabled {
            return true;
        }
        self.valid_tokens.iter().any(|t| t == token)
    }
}

/// 从请求头提取 Bearer token
fn extract_bearer_token(req: &Request) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// Axum 认证中间件
///
/// 从 `Authorization: Bearer <token>` 头提取 token，
/// 验证是否在合法 token 列表中。
pub async fn auth_middleware(
    State(auth_config): State<AuthConfig>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !auth_config.enabled {
        return Ok(next.run(req).await);
    }

    let token = extract_bearer_token(&req).ok_or(StatusCode::UNAUTHORIZED)?;

    if auth_config.validate(&token) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_disabled_auth_allows_all() {
        let config = AuthConfig::disabled();
        assert!(config.validate("anything"));
        assert!(config.validate(""));
    }

    #[test]
    fn test_enabled_auth_validates_token() {
        let config = AuthConfig::new(vec!["secret123".to_string()], true);
        assert!(config.validate("secret123"));
        assert!(!config.validate("wrong"));
        assert!(!config.validate(""));
    }

    #[test]
    fn test_enabled_with_empty_tokens_rejects_all() {
        let config = AuthConfig::new(vec![], true);
        assert!(!config.validate("anything"));
    }
}
