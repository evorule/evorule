# EvoRule Project Agents Guide

> **这是给 AI agent 和人类贡献者的工作规则。**
> 战略方向见 [D:\evorule-application\STRATEGIC_DIRECTION.md](file:///D:/evorule-application/STRATEGIC_DIRECTION.md)。
> 本文件只讲**具体规则**,不重复战略。

## 核心原则(One-liner)

**evorule 是 framework,不是 application。任何改动前先问:这是优化还是新功能?这是核心还是应用?**

详细分类见战略文档 §五。这里只列硬规则。

## 硬规则

### ✅ 接受(在 evorule 核心里)

- 性能优化(reactor 并发、TCB 算法、I/O 路径)
- Bug 修复(包括 tier0/tier1/tier2 任意 crate)
- API 稳定性(签名不变、错误类型不变)
- 文档(Rustdoc、README、CHANGELOG、注释)
- 测试覆盖率(单元测试、集成测试、属性测试)
- 协议(SPDX header、license 标识、协议文本)
- CI/CD 增强
- 形式化验证(Kani 证明、proptest)
- TCB 原语扩展(谨慎,需核心维护者 review)
- 性能架构层面重构(零成本抽象)

### ❌ 拒绝(在 evorule 核心里)

- 新工具(放在 D:\evorule-application\)
- 新 SDK 类型(typescript/python 之外)
- 新 UI / 可视化面板
- 新 agent 能力
- 新 HTTP 路由(除非是核心审计/治理必需)
- 新 I/O handler 集成(放在 evo-agent 或 evorule-application)
- 新配置格式(放在 D:\evorule-application\)

### 边界判断

| 想法 | 决策 |
|---|---|
| "加个新 transform 指令" | 看是否 TCB 原语级,否则放 application |
| "加个 Prometheus 指标" | 放 evorule-application(可观测性是应用层) |
| "优化 reactor 并发" | evorule 核心(性能优化) ✅ |
| "做个 web 仪表盘" | D:\evorule-application\ ❌ |
| "加个新 LLM provider" | evo-agent ❌ |
| "加个 SQLite 集成" | 看是 TCB 还是应用,大部分情况放 application |
| "加新 Kani proof" | evorule 核心 ✅ |
| "加 README 章节" | evorule 核心 ✅ |
| "修 bug" | evorule 核心 ✅ |

## 改动前检查表

任何改动前,问自己:

1. 这是 **evorule 核心** 还是 **应用**?
2. 接受清单里有这一类吗?
3. 这个改动是让 evorule 更"纯净"还是更"复杂"?
4. 有没有更简单的实现方式(在 evorule 之外)?

如果 1-3 答不清:**不写代码,先写设计文档**(放 D:\evorule\文档\design\)。

## 与其他项目的关系

- **evo-agent** (D:\evo-agent\):用 evorule 的 AI agent 编排层。它是应用,不是核心。
- **evorule-application** (D:\evorule-application\):可视化 / 调试器 / 仪表盘等的应用集合。
- **tier0-tcb / tier1-reactor / tier2-governance**:evorule 的三个 crate,**都属于核心**。G8 门控("反应器/治理层不得展开控制流")必须保留。

## 版本与发布

- 当前基线:**0.1.0**(全生态统一,2026-07-20)
- 升 1.0 条件:见 [VERSION_STRATEGY.md §4.4](VERSION_STRATEGY.md)
- 第三方安全审计触发条件:见 [VERSION_STRATEGY.md §4.5](VERSION_STRATEGY.md)(VERSION_STRATEGY v1.1)
- 协议:AGPL-3.0-or-later(代码)+ CC0-1.0(`core_eval.json`)
- 内部审计:见 [文档/security/SECURITY_AUDIT_v0.1.0.md](文档/security/SECURITY_AUDIT_v0.1.0.md)(待写)

## 内部约定

- **"文档/" 永不发布**(.gitignore 已保护)
- 中文为主,英文为辅
- 测试要能跑(`cargo test --workspace` 通过是底线)
- 提交前跑 `validate-all.ps1`(5 个 validate-*.ps1 脚本)
- CI 配在 `.gitee-ci/validate.yml`

## 已知坑

- PowerShell 5.1 + 无 BOM UTF-8 文件 = GBK 误读。**用 `add-spdx-safe.ps1` 加 SPDX 头,不要用 `Get-Content -Raw`**
- 集成测试(`tests/integration_test.rs`)需要 mock LLM 才能跑(目前 3 个 FAIL 是 pre-existing,不是这次回归)
- tier1/ffi.rs 允许 unsafe(标了 `#![allow(unsafe_code)]`),其他 crate 必须 `#![forbid(unsafe_code)]`

## 下次开会看什么

- [ ] 战略方向 STRATEGIC_DIRECTION.md 有没有过时
- [ ] 当前在做的 P0 进展
- [ ] 下一个应用的时间旅行调试器设计稿
