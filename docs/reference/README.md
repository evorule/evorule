<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# reference/ — 字典式参考

> **面向需要查阅确切信息的开发者**:API、CLI、配置项、字段。

适合:用的时候翻一翻,看完就走,不需要从头读到尾。

## 写什么

- **准确、完整**:字段、类型、默认值、约束
- **简洁、无废话**:不解释为什么,只写"是什么"
- **尽量自动生成**:API/CLI 文档从代码或注解生成,**不**手抄

## 不要写在这里

- ❌ 教程式引导 → 去 [tutorial/](../tutorial/)
- ❌ 任务步骤 → 去 [how-to/](../how-to/)
- ❌ "为什么这么设计" → 去 [explanation/](../explanation/)

## 命名规范

按"对象"命名(API 名 / CLI 子命令 / 配置文件名),**不**按"任务"命名。

## 已有参考

| 文档 | 内容 | 代码依据 |
|------|------|---------|
| [cli-reference.md](./cli-reference.md) | evorule CLI 全部 7 个子命令参考（参数/默认值/示例/退出码） | evorule-cli/src/cli.rs |
| [fact-types.md](./fact-types.md) | Fact 类型参考（8 种变体的字段说明，含 Stable 瘦身设计） | evorule-reactor/src/fact.rs |
| [json-rule-schema.md](./json-rule-schema.md) | JSON 规则集格式参考（transform 规则/6 种元指令/7 种 domain/指令格式） | evorule-tcb/{executor,domain,transition}.rs + core_eval.json |

> 所有参考文档的技术结论均有源码行号依据，无"待核实"内容。

## 外部参考

- **API 文档**：[docs.rs/evorule-tcb](https://docs.rs/evorule-tcb) / [docs.rs/evorule-reactor](https://docs.rs/evorule-reactor) / [docs.rs/evorule-governance](https://docs.rs/evorule-governance)
- **crates.io**：[crates.io/crates/evorule-cli](https://crates.io/crates/evorule-cli)
