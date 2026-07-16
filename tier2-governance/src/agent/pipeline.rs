#![forbid(unsafe_code)]
//! 流水线编排（Phase A-6）—— 多 Agent 顺序执行，前一个的输出是后一个的输入。
//!
//! # 设计
//!
//! PipelineRunner 是一个独立的编排组件，不属于反应器层。它按顺序创建多个
//! AgentRunner（每个有独立的 Reactor + IoSubscriber），将前一步的输出
//! 作为后一步的输入。
//!
//! 与委托（delegate）的区别：
//! - 委托是 LLM 在 ReAct 循环中**主动决定**调用另一个 Agent
//! - 流水线是**用户预定义**的编排顺序，Agent 不感知流水线结构
//!
//! # goal_template 模板替换
//!
//! 每个步骤的 `goal_template` 中可包含 `{input}` 占位符，运行时被替换为
//! 上一步的输出。若模板不含 `{input}`，则直接拼接上一步输出。

use std::path::PathBuf;
use std::sync::Arc;

use tier0_tcb::JsonValue;
use tier1_reactor::Reactor;

use crate::agent::definition::AgentDefinitionManager;
use crate::agent::runner::AgentRunner;
use crate::agent::AgentResult;
use crate::api::agent_api::DispatcherFactory;
use crate::io_subscriber::IoSubscriber;

/// 流水线步骤定义
#[derive(Debug, Clone)]
pub struct PipelineStep {
    /// Agent 类型（对应 agent.json 中的 agent_type）
    pub agent_type: String,
    /// 目标模板，可含 `{input}` 占位符（被替换为上一步输出）
    pub goal_template: String,
}

/// 流水线规格（完整定义）
#[derive(Debug, Clone)]
pub struct PipelineSpec {
    /// 按顺序执行的步骤列表
    pub steps: Vec<PipelineStep>,
}

/// 单个步骤的执行结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineStepResult {
    /// Agent 类型
    pub agent_type: String,
    /// 实际执行的 goal（模板替换后）
    pub goal: String,
    /// Agent 输出
    pub output: String,
    /// Agent 执行步数
    pub steps: usize,
}

/// 流水线执行结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineResult {
    /// 每个步骤的结果
    pub step_results: Vec<PipelineStepResult>,
    /// 最终输出（最后一步的输出）
    pub final_output: String,
}

/// 流水线执行器
///
/// 持有创建 Agent 所需的全部基础设施，按顺序执行流水线步骤。
pub struct PipelineRunner {
    /// Agent 定义管理器
    definitions: AgentDefinitionManager,
    /// core_eval 配置
    core_eval: Arc<Vec<JsonValue>>,
    /// 反应器最大轮次
    max_rounds: usize,
    /// IoDispatcher 工厂
    dispatcher_factory: DispatcherFactory,
    /// 工具描述（OpenAI tools 格式）
    tools_json: Arc<JsonValue>,
    /// 记忆系统根目录（可选，预留给流水线 Agent 共享 memory 上下文）
    #[allow(dead_code)]
    memory_dir: Option<Arc<PathBuf>>,
}

impl PipelineRunner {
    /// 创建流水线执行器
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        definitions: AgentDefinitionManager,
        core_eval: Arc<Vec<JsonValue>>,
        max_rounds: usize,
        dispatcher_factory: DispatcherFactory,
        tools_json: Arc<JsonValue>,
        memory_dir: Option<Arc<PathBuf>>,
    ) -> Self {
        Self {
            definitions,
            core_eval,
            max_rounds,
            dispatcher_factory,
            tools_json,
            memory_dir,
        }
    }

    /// 执行流水线
    ///
    /// # 参数
    /// - `spec`: 流水线规格
    /// - `initial_input`: 初始输入（传递给第一步，替换 `{input}`）
    ///
    /// # 返回
    /// 流水线执行结果，包含每步的输出和最终输出
    pub async fn run(
        &self,
        spec: &PipelineSpec,
        initial_input: &str,
    ) -> Result<PipelineResult, String> {
        if spec.steps.is_empty() {
            return Err("流水线步骤不能为空".to_string());
        }

        let mut current_input = initial_input.to_string();
        let mut step_results = Vec::with_capacity(spec.steps.len());

        for (i, step) in spec.steps.iter().enumerate() {
            // 替换 {input} 占位符
            let goal = if step.goal_template.contains("{input}") {
                step.goal_template.replace("{input}", &current_input)
            } else {
                // 无占位符时，将上一步输出拼接到 goal 末尾
                if i == 0 {
                    step.goal_template.clone()
                } else {
                    format!("{}\n\n上一步输出：\n{}", step.goal_template, current_input)
                }
            };

            tracing::info!(
                step = i,
                agent_type = %step.agent_type,
                goal = %goal,
                "流水线步骤 {} 开始",
                i + 1
            );

            // 执行 Agent
            let result = self.run_agent(&step.agent_type, &goal).await?;

            tracing::info!(
                step = i,
                agent_type = %step.agent_type,
                steps = result.steps,
                output_len = result.final_answer.len(),
                "流水线步骤 {} 完成",
                i + 1
            );

            current_input = result.final_answer.clone();
            step_results.push(PipelineStepResult {
                agent_type: step.agent_type.clone(),
                goal,
                output: result.final_answer,
                steps: result.steps,
            });
        }

        Ok(PipelineResult {
            final_output: current_input,
            step_results,
        })
    }

    /// 执行单个 Agent（创建独立的 Reactor + IoSubscriber + AgentRunner）
    async fn run_agent(&self, agent_type: &str, goal: &str) -> Result<AgentResult, String> {
        // 1. 加载 Agent 定义
        let def = self
            .definitions
            .load(agent_type)
            .map_err(|e| format!("加载 Agent 定义失败 ({}): {}", agent_type, e))?;

        let config = def.to_agent_config();

        // 2. 创建反应器
        let reactor = Reactor::builder((*self.core_eval).clone())
            .max_rounds(self.max_rounds)
            .build();
        let (command_tx, event_rx, event_tx, _reactor_handle, _facts_log) = reactor.spawn();

        // 3. 创建 IoDispatcher + IoSubscriber
        let dispatcher = (self.dispatcher_factory)().await?;
        let subscriber = IoSubscriber::new(dispatcher);
        let sub_rx = event_tx.subscribe();
        let sub_tx = command_tx.clone();
        tokio::spawn(async move {
            let _ = subscriber.run(sub_rx, sub_tx).await;
        });

        // 4. 创建 AgentRunner
        let mut runner = AgentRunner::new(config, command_tx, event_rx, (*self.tools_json).clone());

        // 5. 运行 Agent
        runner
            .run(goal)
            .await
            .map_err(|e| format!("Agent 执行失败: {}", e))
    }
}

/// 从 JsonValue 构造 PipelineSpec（用于 HTTP API 反序列化）
pub fn parse_pipeline_spec(json: &JsonValue) -> Result<PipelineSpec, String> {
    let steps_arr = json
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "pipeline spec 缺少 steps 数组".to_string())?;

    let mut steps = Vec::with_capacity(steps_arr.len());
    for (i, step_json) in steps_arr.iter().enumerate() {
        let agent_type = step_json
            .get("agent_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("步骤 {} 缺少 agent_type", i))?
            .to_string();

        let goal_template = step_json
            .get("goal_template")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("步骤 {} 缺少 goal_template", i))?
            .to_string();

        steps.push(PipelineStep {
            agent_type,
            goal_template,
        });
    }

    Ok(PipelineSpec { steps })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_parse_pipeline_spec_valid() {
        let mut step1 = BTreeMap::new();
        step1.insert(
            "agent_type".to_string(),
            JsonValue::String("researcher".to_string()),
        );
        step1.insert(
            "goal_template".to_string(),
            JsonValue::String("研究 {input} 的架构".to_string()),
        );

        let mut step2 = BTreeMap::new();
        step2.insert(
            "agent_type".to_string(),
            JsonValue::String("writer".to_string()),
        );
        step2.insert(
            "goal_template".to_string(),
            JsonValue::String("基于研究结果撰写报告：{input}".to_string()),
        );

        let mut spec = BTreeMap::new();
        spec.insert(
            "steps".to_string(),
            JsonValue::Array(vec![JsonValue::Object(step1), JsonValue::Object(step2)]),
        );

        let result = parse_pipeline_spec(&JsonValue::Object(spec)).unwrap();
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.steps[0].agent_type, "researcher");
        assert_eq!(result.steps[1].agent_type, "writer");
        assert!(result.steps[0].goal_template.contains("{input}"));
    }

    #[test]
    fn test_parse_pipeline_spec_missing_steps() {
        let spec = JsonValue::Object(BTreeMap::new());
        let result = parse_pipeline_spec(&spec);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("steps"));
    }

    #[test]
    fn test_parse_pipeline_spec_empty_steps() {
        let mut spec = BTreeMap::new();
        spec.insert("steps".to_string(), JsonValue::Array(vec![]));
        let result = parse_pipeline_spec(&JsonValue::Object(spec)).unwrap();
        assert!(result.steps.is_empty());
    }

    #[test]
    fn test_parse_pipeline_spec_missing_agent_type() {
        let mut step = BTreeMap::new();
        step.insert(
            "goal_template".to_string(),
            JsonValue::String("某任务".to_string()),
        );

        let mut spec = BTreeMap::new();
        spec.insert(
            "steps".to_string(),
            JsonValue::Array(vec![JsonValue::Object(step)]),
        );

        let result = parse_pipeline_spec(&JsonValue::Object(spec));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("agent_type"));
    }

    #[test]
    fn test_pipeline_step_clone() {
        let step = PipelineStep {
            agent_type: "researcher".to_string(),
            goal_template: "研究 {input}".to_string(),
        };
        let cloned = step.clone();
        assert_eq!(cloned.agent_type, step.agent_type);
        assert_eq!(cloned.goal_template, step.goal_template);
    }

    #[test]
    fn test_pipeline_spec_clone() {
        let spec = PipelineSpec {
            steps: vec![
                PipelineStep {
                    agent_type: "a".to_string(),
                    goal_template: "do {input}".to_string(),
                },
                PipelineStep {
                    agent_type: "b".to_string(),
                    goal_template: "write {input}".to_string(),
                },
            ],
        };
        let cloned = spec.clone();
        assert_eq!(cloned.steps.len(), 2);
    }
}
