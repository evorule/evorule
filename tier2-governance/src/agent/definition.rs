//! Agent 定义 —— 从 agent.json 加载 Agent 配置
//!
//! agent.json 描述 Agent 的静态配置（system prompt、工具、模型等），
//! 运行时由 AgentRunner 读取并执行 ReAct 循环。
//!
//! # agent.json 格式
//!
//! ```json
//! {
//!   "agent_type": "researcher",
//!   "version": "1.0.0",
//!   "description": "研究型 Agent",
//!   "system_prompt": "你是一个研究型 Agent...",
//!   "model": "gpt-4o-mini",
//!   "temperature": 0.3,
//!   "max_steps": 20,
//!   "step_timeout_secs": 60,
//!   "tools": ["search_web", "read_file"],
//!   "memory": { "type": "file", "namespace": "researcher" },
//!   "output_format": null
//! }
//! ```

use std::path::{Path, PathBuf};

use crate::agent::runner::AgentConfig;

/// Agent 定义错误
#[derive(Debug)]
pub enum AgentDefinitionError {
    /// 文件 I/O 错误
    Io(std::io::Error),
    /// JSON 解析错误
    Json(serde_json::Error),
    /// Agent 类型不存在
    NotFound(String),
}

impl std::fmt::Display for AgentDefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentDefinitionError::Io(e) => write!(f, "IO error: {}", e),
            AgentDefinitionError::Json(e) => write!(f, "JSON parse error: {}", e),
            AgentDefinitionError::NotFound(t) => write!(f, "Agent type not found: {}", t),
        }
    }
}

impl std::error::Error for AgentDefinitionError {}

impl From<std::io::Error> for AgentDefinitionError {
    fn from(e: std::io::Error) -> Self {
        AgentDefinitionError::Io(e)
    }
}

impl From<serde_json::Error> for AgentDefinitionError {
    fn from(e: serde_json::Error) -> Self {
        AgentDefinitionError::Json(e)
    }
}

/// 记忆系统配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryConfig {
    /// 记忆类型（"file" / "vector" / "none"）
    #[serde(rename = "type")]
    pub memory_type: String,
    /// 命名空间（隔离不同 Agent 的记忆）
    pub namespace: String,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            memory_type: "none".to_string(),
            namespace: String::new(),
        }
    }
}

/// 输出格式定义
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutputFormat {
    /// 输出类型（"json" / "text" / "markdown"）
    #[serde(rename = "type")]
    pub format_type: String,
    /// JSON Schema（当 type=json 时使用）
    pub schema: Option<serde_json::Value>,
}

/// Agent 定义（对应 agent.json）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentDefinition {
    /// Agent 类型标识
    pub agent_type: String,
    /// 定义版本
    pub version: String,
    /// 人类可读描述
    pub description: String,
    /// System prompt
    pub system_prompt: String,
    /// LLM 模型名称
    pub model: String,
    /// 采样温度
    pub temperature: f32,
    /// 最大推理步数
    pub max_steps: usize,
    /// 单步超时（秒）
    pub step_timeout_secs: u64,
    /// 可用工具列表
    pub tools: Vec<String>,
    /// 记忆配置
    #[serde(default)]
    pub memory: MemoryConfig,
    /// 输出格式（可选）
    pub output_format: Option<OutputFormat>,
}

impl AgentDefinition {
    /// 从目录加载指定 Agent 类型的定义
    ///
    /// 在 `dir` 目录下查找 `{agent_type}.json` 文件。
    pub fn load_from_dir(dir: &Path, agent_type: &str) -> Result<Self, AgentDefinitionError> {
        let path = dir.join(format!("{}.json", agent_type));
        if !path.exists() {
            return Err(AgentDefinitionError::NotFound(agent_type.to_string()));
        }
        let content = std::fs::read_to_string(&path)?;
        let def: AgentDefinition = serde_json::from_str(&content)?;
        Ok(def)
    }

    /// 列出目录下所有可用的 Agent 类型
    ///
    /// 扫描 `dir` 下的 `*.json` 文件，返回文件名（不含扩展名）列表。
    pub fn list_available(dir: &Path) -> Result<Vec<String>, AgentDefinitionError> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut types = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    types.push(stem.to_string());
                }
            }
        }
        types.sort();
        Ok(types)
    }

    /// 转换为 AgentConfig（运行时配置）
    pub fn to_agent_config(&self) -> AgentConfig {
        AgentConfig {
            agent_type: self.agent_type.clone(),
            system_prompt: self.system_prompt.clone(),
            model: self.model.clone(),
            temperature: self.temperature,
            max_steps: self.max_steps,
            step_timeout: std::time::Duration::from_secs(self.step_timeout_secs),
            tool_names: self.tools.clone(),
        }
    }
}

/// Agent 定义管理器
///
/// 持有 agents 目录路径，提供 Agent 定义的加载和列举功能。
/// 在 HTTP API 中作为共享状态使用。
#[derive(Debug, Clone)]
pub struct AgentDefinitionManager {
    /// agent.json 所在目录
    agents_dir: PathBuf,
}

impl AgentDefinitionManager {
    /// 创建定义管理器
    pub fn new(agents_dir: PathBuf) -> Self {
        Self { agents_dir }
    }

    /// 使用默认目录创建（rules/agents/）
    pub fn with_default_dir() -> Self {
        let dir = PathBuf::from("rules/agents");
        Self::new(dir)
    }

    /// 获取 agents 目录路径
    pub fn agents_dir(&self) -> &Path {
        &self.agents_dir
    }

    /// 加载指定 Agent 类型定义
    pub fn load(&self, agent_type: &str) -> Result<AgentDefinition, AgentDefinitionError> {
        AgentDefinition::load_from_dir(&self.agents_dir, agent_type)
    }

    /// 列出所有可用 Agent 类型
    pub fn list_types(&self) -> Result<Vec<String>, AgentDefinitionError> {
        AgentDefinition::list_available(&self.agents_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_tmp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create tempdir")
    }

    fn write_json(dir: &Path, name: &str, json: &str) {
        let path = dir.join(format!("{}.json", name));
        let mut f = std::fs::File::create(&path).expect("create file");
        f.write_all(json.as_bytes()).expect("write file");
    }

    #[test]
    fn test_agent_definition_deserialize() {
        let json = r#"{
            "agent_type": "researcher",
            "version": "1.0.0",
            "description": "研究型 Agent",
            "system_prompt": "你是一个研究型 Agent",
            "model": "gpt-4o-mini",
            "temperature": 0.3,
            "max_steps": 20,
            "step_timeout_secs": 60,
            "tools": ["search_web", "read_file"],
            "memory": { "type": "file", "namespace": "researcher" },
            "output_format": { "type": "json", "schema": {"summary": "string"} }
        }"#;
        let def: AgentDefinition = serde_json::from_str(json).expect("parse");
        assert_eq!(def.agent_type, "researcher");
        assert_eq!(def.version, "1.0.0");
        assert_eq!(def.model, "gpt-4o-mini");
        assert!((def.temperature - 0.3).abs() < 0.01);
        assert_eq!(def.max_steps, 20);
        assert_eq!(def.step_timeout_secs, 60);
        assert_eq!(def.tools, vec!["search_web", "read_file"]);
        assert_eq!(def.memory.memory_type, "file");
        assert_eq!(def.memory.namespace, "researcher");
        assert!(def.output_format.is_some());
        assert_eq!(def.output_format.as_ref().unwrap().format_type, "json");
    }

    #[test]
    fn test_agent_definition_default_memory() {
        // memory 字段缺失时使用默认值
        let json = r#"{
            "agent_type": "simple",
            "version": "1.0.0",
            "description": "简单 Agent",
            "system_prompt": "你好",
            "model": "gpt-4o-mini",
            "temperature": 0.7,
            "max_steps": 10,
            "step_timeout_secs": 30,
            "tools": [],
            "output_format": null
        }"#;
        let def: AgentDefinition = serde_json::from_str(json).expect("parse");
        assert_eq!(def.memory.memory_type, "none");
        assert!(def.output_format.is_none());
    }

    #[test]
    fn test_load_from_dir() {
        let dir = make_tmp_dir();
        let json = r#"{
            "agent_type": "test_agent",
            "version": "2.0.0",
            "description": "测试",
            "system_prompt": "test",
            "model": "gpt-4",
            "temperature": 0.5,
            "max_steps": 5,
            "step_timeout_secs": 10,
            "tools": ["echo"],
            "output_format": null
        }"#;
        write_json(dir.path(), "test_agent", json);

        let def = AgentDefinition::load_from_dir(dir.path(), "test_agent").expect("load");
        assert_eq!(def.agent_type, "test_agent");
        assert_eq!(def.version, "2.0.0");
    }

    #[test]
    fn test_load_from_dir_not_found() {
        let dir = make_tmp_dir();
        let result = AgentDefinition::load_from_dir(dir.path(), "nonexistent");
        assert!(matches!(result, Err(AgentDefinitionError::NotFound(_))));
    }

    #[test]
    fn test_list_available() {
        let dir = make_tmp_dir();
        write_json(
            dir.path(),
            "alpha",
            r#"{"agent_type":"alpha","version":"1","description":"","system_prompt":"","model":"","temperature":0.5,"max_steps":1,"step_timeout_secs":1,"tools":[],"output_format":null}"#,
        );
        write_json(
            dir.path(),
            "beta",
            r#"{"agent_type":"beta","version":"1","description":"","system_prompt":"","model":"","temperature":0.5,"max_steps":1,"step_timeout_secs":1,"tools":[],"output_format":null}"#,
        );
        // 非 json 文件应被忽略
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();

        let types = AgentDefinition::list_available(dir.path()).expect("list");
        assert_eq!(types.len(), 2);
        assert_eq!(types[0], "alpha");
        assert_eq!(types[1], "beta");
    }

    #[test]
    fn test_list_available_empty_dir() {
        let dir = make_tmp_dir();
        let types = AgentDefinition::list_available(dir.path()).expect("list");
        assert!(types.is_empty());
    }

    #[test]
    fn test_list_available_nonexistent_dir() {
        let types = AgentDefinition::list_available(Path::new("/nonexistent/path/xyz"))
            .expect("nonexistent dir returns empty");
        assert!(types.is_empty());
    }

    #[test]
    fn test_to_agent_config() {
        let def = AgentDefinition {
            agent_type: "writer".to_string(),
            version: "1.0.0".to_string(),
            description: "写作 Agent".to_string(),
            system_prompt: "你是写作助手".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.8,
            max_steps: 15,
            step_timeout_secs: 45,
            tools: vec!["write_file".to_string()],
            memory: MemoryConfig::default(),
            output_format: None,
        };
        let config = def.to_agent_config();
        assert_eq!(config.agent_type, "writer");
        assert_eq!(config.system_prompt, "你是写作助手");
        assert_eq!(config.model, "gpt-4o");
        assert!((config.temperature - 0.8).abs() < 0.01);
        assert_eq!(config.max_steps, 15);
        assert_eq!(config.step_timeout, std::time::Duration::from_secs(45));
        assert_eq!(config.tool_names, vec!["write_file"]);
    }

    #[test]
    fn test_definition_manager() {
        let dir = make_tmp_dir();
        write_json(
            dir.path(),
            "researcher",
            r#"{"agent_type":"researcher","version":"1","description":"","system_prompt":"test","model":"gpt-4","temperature":0.3,"max_steps":10,"step_timeout_secs":30,"tools":[],"output_format":null}"#,
        );
        let mgr = AgentDefinitionManager::new(dir.path().to_path_buf());
        let types = mgr.list_types().expect("list");
        assert_eq!(types, vec!["researcher"]);
        let def = mgr.load("researcher").expect("load");
        assert_eq!(def.agent_type, "researcher");
    }

    #[test]
    fn test_definition_error_display() {
        let err = AgentDefinitionError::NotFound("foo".to_string());
        assert!(format!("{}", err).contains("foo"));

        let err = AgentDefinitionError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        ));
        assert!(format!("{}", err).contains("IO error"));

        let err = AgentDefinitionError::Json(
            serde_json::from_str::<serde_json::Value>("bad").unwrap_err(),
        );
        assert!(format!("{}", err).contains("JSON parse error"));
    }
}
