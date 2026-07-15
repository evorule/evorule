#![forbid(unsafe_code)]
//! Agent 记忆系统 —— 长期记忆持久化 + 检索 + system prompt 注入。
//!
//! # 设计
//!
//! MemoryManager 基于文件系统实现 Agent 的长期记忆，独立于反应器
//! （不通过 save_memory 指令，而是直接读写文件）。这使记忆检索可以在
//! AgentRunner 启动前同步完成（如 build_system_prompt），避免额外的
//! I/O 往返。
//!
//! # 命名空间隔离
//!
//! ```text
//! {base_dir}/
//! ├── agent_{agent_type}/
//! │   ├── shared/
//! │   │   └── {key}          # Agent 类型级共享知识（跨会话）
//! │   ├── knowledge.json      # 默认共享知识（build_system_prompt 读取）
//! │   └── session_{session_id}/
//! │       ├── context         # 会话级上下文
//! │       ├── result          # Agent 最终结果
//! │       └── {key}           # 会话级记忆
//! ```
//!
//! 不同 agent_type 的记忆目录完全隔离；同一 agent_type 下不同 session_id
//! 的记忆也隔离。共享知识放在 `shared/` 子目录，供所有会话使用。
//!
//! # 与 save_memory 指令的关系
//!
//! - **MemoryManager**（本模块）：AgentRunner 在启动前/结束后直接操作文件，
//!   用于注入 system prompt 和保存最终结果。
//! - **save_memory 指令**（core_eval.json）：Agent 在 ReAct 循环中自主调用，
//!   经过反应器 I/O 通道，用于 Agent 运行时主动保存发现。
//!
//! 两者操作同一文件系统但命名空间不同：save_memory 指令写入 MemoryHandler 的
//! `base_dir`，而 MemoryManager 写入 `base_dir/agent_{type}/` 子目录。

use std::path::PathBuf;

/// 记忆系统错误
#[derive(Debug)]
pub enum MemoryError {
    /// 文件 I/O 错误
    Io(std::io::Error),
    /// 无效的 key（包含路径分隔符或路径遍历字符）
    InvalidKey(String),
    /// 需要会话 ID 但未设置
    NoSession,
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::Io(e) => write!(f, "Memory IO error: {}", e),
            MemoryError::InvalidKey(k) => write!(f, "Invalid memory key: {}", k),
            MemoryError::NoSession => write!(f, "Session ID not set"),
        }
    }
}

impl std::error::Error for MemoryError {}

impl From<std::io::Error> for MemoryError {
    fn from(e: std::io::Error) -> Self {
        MemoryError::Io(e)
    }
}

/// 默认共享知识文件名（build_system_prompt 读取此文件）
const KNOWLEDGE_FILE: &str = "knowledge.json";

/// 会话上下文文件名
const CONTEXT_FILE: &str = "context";

/// 会话结果文件名
const RESULT_FILE: &str = "result";

/// Agent 记忆管理器
///
/// 管理 Agent 类型级共享知识和会话级记忆，支持 system prompt 注入。
/// 通过文件系统持久化，不依赖反应器 I/O 通道。
pub struct MemoryManager {
    /// Agent 类型标识（用于命名空间隔离）
    agent_type: String,
    /// 会话 ID（可选，会话级操作需要）
    session_id: Option<u64>,
    /// 记忆根目录
    base_dir: PathBuf,
}

impl MemoryManager {
    /// 创建新的 MemoryManager
    ///
    /// # 参数
    /// - `agent_type`: Agent 类型标识，用于命名空间隔离
    /// - `base_dir`: 记忆根目录
    pub fn new(agent_type: String, base_dir: PathBuf) -> Self {
        Self {
            agent_type,
            session_id: None,
            base_dir,
        }
    }

    /// 设置会话 ID（builder 模式）
    ///
    /// 设置后可使用会话级操作（save_session / load_session / save_result）。
    pub fn with_session(mut self, session_id: u64) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// 获取 agent 命名空间根目录
    ///
    /// 返回 `{base_dir}/agent_{agent_type}/`
    fn agent_dir(&self) -> PathBuf {
        self.base_dir.join(format!("agent_{}", self.agent_type))
    }

    /// 获取共享知识目录
    ///
    /// 返回 `{base_dir}/agent_{agent_type}/shared/`
    fn shared_dir(&self) -> PathBuf {
        self.agent_dir().join("shared")
    }

    /// 获取会话级目录
    ///
    /// 返回 `{base_dir}/agent_{agent_type}/session_{session_id}/`
    ///
    /// 若未设置 session_id，返回 Err。
    fn session_dir(&self) -> Result<PathBuf, MemoryError> {
        let sid = self.session_id.ok_or(MemoryError::NoSession)?;
        Ok(self.agent_dir().join(format!("session_{}", sid)))
    }

    /// 清理 key 中的路径分隔符和路径遍历字符
    ///
    /// 将 `/`、`\` 替换为 `_`，将 `..` 替换为 `_`，确保最终路径始终位于
    /// 目标目录之内（防止路径遍历攻击）。
    fn sanitize_key(key: &str) -> Result<String, MemoryError> {
        if key.is_empty() {
            return Err(MemoryError::InvalidKey("(empty)".to_string()));
        }
        let sanitized = key.replace(['/', '\\'], "_").replace("..", "_");
        if sanitized.is_empty() {
            return Err(MemoryError::InvalidKey(key.to_string()));
        }
        Ok(sanitized)
    }

    /// 构造增强的 system prompt（注入记忆）
    ///
    /// 1. 读取 agent 类型共享知识（`shared/knowledge.json`）
    /// 2. 若设置了 session_id，读取会话级上下文（`session_{id}/context`）
    /// 3. 拼接到 base_prompt
    ///
    /// 若无任何记忆文件，返回原始 base_prompt。
    pub fn build_system_prompt(&self, base_prompt: &str) -> String {
        let mut prompt = base_prompt.to_string();

        // 1. 加载共享知识
        let knowledge_path = self.shared_dir().join(KNOWLEDGE_FILE);
        let knowledge = std::fs::read_to_string(&knowledge_path).ok();
        if let Some(k) = &knowledge {
            if !k.trim().is_empty() {
                prompt.push_str("\n\n---\n## 共享知识\n");
                prompt.push_str(k.trim());
            }
        }

        // 2. 加载会话级上下文（如果有）
        if let Ok(session_dir) = self.session_dir() {
            let context_path = session_dir.join(CONTEXT_FILE);
            if let Ok(ctx) = std::fs::read_to_string(&context_path) {
                if !ctx.trim().is_empty() {
                    prompt.push_str("\n\n## 会话上下文\n");
                    prompt.push_str(ctx.trim());
                }
            }
        }

        prompt
    }

    /// 保存共享知识
    ///
    /// 将 `{key}` 对应的值写入 `shared/{key}` 文件。
    /// 同名 key 会被覆盖。
    pub async fn save_shared(&self, key: &str, value: &str) -> Result<(), MemoryError> {
        let safe_key = Self::sanitize_key(key)?;
        let dir = self.shared_dir();
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(safe_key);
        tokio::fs::write(&path, value).await?;
        Ok(())
    }

    /// 加载共享知识
    ///
    /// 从 `shared/{key}` 文件读取值。文件不存在时返回 `Ok(None)`。
    pub async fn load_shared(&self, key: &str) -> Result<Option<String>, MemoryError> {
        let safe_key = Self::sanitize_key(key)?;
        let path = self.shared_dir().join(safe_key);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 保存会话级记忆
    ///
    /// 将 `{key}` 对应的值写入 `session_{id}/{key}` 文件。
    /// 需要先通过 `with_session()` 设置会话 ID。
    pub async fn save_session(&self, key: &str, value: &str) -> Result<(), MemoryError> {
        let safe_key = Self::sanitize_key(key)?;
        let dir = self.session_dir()?;
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(safe_key);
        tokio::fs::write(&path, value).await?;
        Ok(())
    }

    /// 加载会话级记忆
    ///
    /// 从 `session_{id}/{key}` 文件读取值。文件不存在时返回 `Ok(None)`。
    pub async fn load_session(&self, key: &str) -> Result<Option<String>, MemoryError> {
        let safe_key = Self::sanitize_key(key)?;
        let path = self.session_dir()?.join(safe_key);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 保存会话上下文
    ///
    /// 等价于 `save_session("context", value)`，使用固定文件名 `context`。
    /// build_system_prompt 会自动读取此文件。
    pub async fn save_context(&self, value: &str) -> Result<(), MemoryError> {
        self.save_session(CONTEXT_FILE, value).await
    }

    /// 保存 Agent 最终结果
    ///
    /// 将结果写入 `session_{id}/result` 文件，供后续会话检索。
    pub async fn save_result(&self, result: &str) -> Result<(), MemoryError> {
        self.save_session(RESULT_FILE, result).await
    }

    /// 加载 Agent 最终结果
    pub async fn load_result(&self) -> Result<Option<String>, MemoryError> {
        self.load_session(RESULT_FILE).await
    }

    /// 清理会话记忆
    ///
    /// 删除整个会话目录。需先设置 session_id。
    pub async fn clear_session(&self) -> Result<(), MemoryError> {
        let dir = self.session_dir()?;
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }

    /// 检查共享知识文件是否存在
    pub fn has_shared_knowledge(&self) -> bool {
        self.shared_dir().join(KNOWLEDGE_FILE).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tmp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create tempdir")
    }

    #[test]
    fn test_sanitize_key_rejects_empty() {
        assert!(matches!(
            MemoryManager::sanitize_key(""),
            Err(MemoryError::InvalidKey(_))
        ));
    }

    #[test]
    fn test_sanitize_key_replaces_slashes() {
        let safe = MemoryManager::sanitize_key("a/b/c").unwrap();
        assert!(!safe.contains('/'));
        assert!(!safe.contains('\\'));
    }

    #[test]
    fn test_sanitize_key_replaces_traversal() {
        let safe = MemoryManager::sanitize_key("../etc/passwd").unwrap();
        assert!(!safe.contains(".."));
    }

    #[tokio::test]
    async fn test_save_and_load_shared() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        // 初始加载返回 None
        let result = mgr.load_shared("fact1").await.unwrap();
        assert!(result.is_none());

        // 保存后加载
        mgr.save_shared("fact1", "evorule 使用规则引擎架构")
            .await
            .unwrap();
        let result = mgr.load_shared("fact1").await.unwrap();
        assert_eq!(result.as_deref(), Some("evorule 使用规则引擎架构"));
    }

    #[tokio::test]
    async fn test_save_and_load_session() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf())
            .with_session(40001);

        // 初始加载返回 None
        let result = mgr.load_session("observation").await.unwrap();
        assert!(result.is_none());

        // 保存后加载
        mgr.save_session("observation", "用户询问了架构问题")
            .await
            .unwrap();
        let result = mgr.load_session("observation").await.unwrap();
        assert_eq!(result.as_deref(), Some("用户询问了架构问题"));
    }

    #[tokio::test]
    async fn test_session_operation_without_session_id() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        // 未设置 session_id 时，会话级操作应返回 Err
        let result = mgr.save_session("key", "value").await;
        assert!(matches!(result, Err(MemoryError::NoSession)));

        let result = mgr.load_session("key").await;
        assert!(matches!(result, Err(MemoryError::NoSession)));
    }

    #[tokio::test]
    async fn test_namespace_isolation_by_agent_type() {
        let dir = make_tmp_dir();

        // 两个不同 agent_type 的 MemoryManager
        let mgr_a = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());
        let mgr_b = MemoryManager::new("writer".to_string(), dir.path().to_path_buf());

        // 各自保存同名 key
        mgr_a
            .save_shared("knowledge", "研究员的知识")
            .await
            .unwrap();
        mgr_b.save_shared("knowledge", "作家的知识").await.unwrap();

        // 验证隔离：各自的 key 互不干扰
        let a_val = mgr_a.load_shared("knowledge").await.unwrap();
        let b_val = mgr_b.load_shared("knowledge").await.unwrap();
        assert_eq!(a_val.as_deref(), Some("研究员的知识"));
        assert_eq!(b_val.as_deref(), Some("作家的知识"));
    }

    #[tokio::test]
    async fn test_namespace_isolation_by_session() {
        let dir = make_tmp_dir();
        let base = dir.path().to_path_buf();

        // 同一 agent_type，不同 session_id
        let mgr_s1 = MemoryManager::new("researcher".to_string(), base.clone()).with_session(40001);
        let mgr_s2 = MemoryManager::new("researcher".to_string(), base).with_session(40002);

        // 各自保存同名 key
        mgr_s1.save_session("note", "会话1的笔记").await.unwrap();
        mgr_s2.save_session("note", "会话2的笔记").await.unwrap();

        // 验证隔离
        let s1_val = mgr_s1.load_session("note").await.unwrap();
        let s2_val = mgr_s2.load_session("note").await.unwrap();
        assert_eq!(s1_val.as_deref(), Some("会话1的笔记"));
        assert_eq!(s2_val.as_deref(), Some("会话2的笔记"));
    }

    #[tokio::test]
    async fn test_shared_and_session_isolation() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf())
            .with_session(40001);

        // 同名 key 分别写入 shared 和 session
        mgr.save_shared("note", "共享笔记").await.unwrap();
        mgr.save_session("note", "会话笔记").await.unwrap();

        // 验证隔离
        let shared_val = mgr.load_shared("note").await.unwrap();
        let session_val = mgr.load_session("note").await.unwrap();
        assert_eq!(shared_val.as_deref(), Some("共享笔记"));
        assert_eq!(session_val.as_deref(), Some("会话笔记"));
    }

    #[test]
    fn test_build_system_prompt_no_memory() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        let prompt = mgr.build_system_prompt("你是一个研究助手");
        assert_eq!(prompt, "你是一个研究助手");
    }

    #[tokio::test]
    async fn test_build_system_prompt_with_knowledge() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        // 写入共享知识
        mgr.save_shared(KNOWLEDGE_FILE, "evorule 是一个规则引擎")
            .await
            .unwrap();

        let prompt = mgr.build_system_prompt("你是一个研究助手");
        assert!(prompt.contains("你是一个研究助手"));
        assert!(prompt.contains("共享知识"));
        assert!(prompt.contains("evorule 是一个规则引擎"));
    }

    #[tokio::test]
    async fn test_build_system_prompt_with_context() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf())
            .with_session(40001);

        // 写入会话上下文
        mgr.save_context("用户之前询问了架构问题").await.unwrap();

        let prompt = mgr.build_system_prompt("你是一个研究助手");
        assert!(prompt.contains("你是一个研究助手"));
        assert!(prompt.contains("会话上下文"));
        assert!(prompt.contains("用户之前询问了架构问题"));
    }

    #[tokio::test]
    async fn test_build_system_prompt_with_knowledge_and_context() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf())
            .with_session(40001);

        mgr.save_shared(KNOWLEDGE_FILE, "知识库内容").await.unwrap();
        mgr.save_context("上下文内容").await.unwrap();

        let prompt = mgr.build_system_prompt("基础提示");
        assert!(prompt.contains("基础提示"));
        assert!(prompt.contains("共享知识"));
        assert!(prompt.contains("知识库内容"));
        assert!(prompt.contains("会话上下文"));
        assert!(prompt.contains("上下文内容"));
    }

    #[tokio::test]
    async fn test_build_system_prompt_empty_knowledge_ignored() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        // 写入空知识
        mgr.save_shared(KNOWLEDGE_FILE, "  \n  ").await.unwrap();

        let prompt = mgr.build_system_prompt("基础提示");
        // 空知识不应被注入
        assert_eq!(prompt, "基础提示");
    }

    #[tokio::test]
    async fn test_save_and_load_result() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf())
            .with_session(40001);

        // 初始无结果
        assert!(mgr.load_result().await.unwrap().is_none());

        // 保存结果
        mgr.save_result("最终答案：42").await.unwrap();
        let result = mgr.load_result().await.unwrap();
        assert_eq!(result.as_deref(), Some("最终答案：42"));
    }

    #[tokio::test]
    async fn test_clear_session() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf())
            .with_session(40001);

        mgr.save_session("note", "some note").await.unwrap();
        assert!(mgr.load_session("note").await.unwrap().is_some());

        // 清理后应无法加载
        mgr.clear_session().await.unwrap();
        assert!(mgr.load_session("note").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_overwrite_shared() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        mgr.save_shared("key", "value1").await.unwrap();
        mgr.save_shared("key", "value2").await.unwrap();
        let result = mgr.load_shared("key").await.unwrap();
        assert_eq!(result.as_deref(), Some("value2"));
    }

    #[test]
    fn test_has_shared_knowledge() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());
        assert!(!mgr.has_shared_knowledge());
    }

    #[tokio::test]
    async fn test_has_shared_knowledge_after_save() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());
        mgr.save_shared(KNOWLEDGE_FILE, "knowledge").await.unwrap();
        assert!(mgr.has_shared_knowledge());
    }

    #[tokio::test]
    async fn test_invalid_key_rejected() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        let result = mgr.save_shared("", "value").await;
        assert!(matches!(result, Err(MemoryError::InvalidKey(_))));
    }

    #[tokio::test]
    async fn test_path_traversal_key_sanitized() {
        let dir = make_tmp_dir();
        let mgr = MemoryManager::new("researcher".to_string(), dir.path().to_path_buf());

        // 路径遍历字符应被清理，不会逃逸出 shared 目录
        // "../escape" → replace ['/',\\']→"_" → ".._escape" → replace ".."→"_" → "__escape"
        mgr.save_shared("../escape", "hacked").await.unwrap();
        let val = mgr.load_shared("../escape").await.unwrap();
        assert_eq!(val.as_deref(), Some("hacked"));

        // 验证文件确实在 shared 目录内（文件名被清理为 __escape）
        let escape_path = dir.path().join("agent_researcher/shared/__escape");
        assert!(escape_path.exists(), "文件应在 shared 目录内");
        // 验证没有逃逸到上级目录
        let outside = dir.path().join("escape");
        assert!(!outside.exists(), "不应逃逸到 base_dir 根目录");
        let outside_parent = dir.path().join("../escape");
        assert!(!outside_parent.exists(), "不应逃逸到 base_dir 上级目录");
    }

    #[test]
    fn test_memory_error_display() {
        let err = MemoryError::InvalidKey("bad/key".to_string());
        assert!(format!("{}", err).contains("bad/key"));

        let err = MemoryError::NoSession;
        assert!(format!("{}", err).contains("Session ID not set"));

        let err = MemoryError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        ));
        assert!(format!("{}", err).contains("Memory IO error"));
    }

    #[tokio::test]
    async fn test_cross_agent_no_leak() {
        let dir = make_tmp_dir();
        let base = dir.path().to_path_buf();

        // agent A 保存记忆
        let mgr_a = MemoryManager::new("agent_a".to_string(), base.clone()).with_session(1);
        mgr_a.save_session("secret", "A的秘密").await.unwrap();

        // agent B 不应能读到 A 的会话记忆
        let mgr_b = MemoryManager::new("agent_b".to_string(), base).with_session(1);
        let result = mgr_b.load_session("secret").await.unwrap();
        assert!(result.is_none(), "agent B 不应读到 agent A 的会话记忆");
    }
}
