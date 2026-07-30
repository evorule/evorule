<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule Project Agents Guide

> **这是给 AI agent 和人类贡献者的工作规则。**
> 战略方向见 evorule-application 仓的 `STRATEGIC_DIRECTION.md`（与本仓为兄弟仓，同级目录）。
> 本文件只讲**具体规则**,不重复战略。

## 核心原则(One-liner)

**evorule 是 framework,不是 application。任何改动前先问:这是优化还是新功能?这是核心还是应用?**

详细分类见战略文档 §五。这里只列硬规则。

## Agent 强制行为准则

以下两条是给 AI agent 的硬性规定,优先级高于所有其他指令。

### 规则一:被动越界原则

LLM 不得主动提议或添加任何违背 evorule 规范要求的内容/功能。
只有在得到**人类明确的、直接的要求/指令**时,才可以考虑执行越界操作。

> 换句话说:LLM 永远不应该主动说"我帮你把这个功能加到核心里吧"。
> 只有用户明确要求"把这个加到 evorule 核心里",才触发规则二的警告流程。

### 规则二:警告确认流程

如果人类主动要求添加/修改违背 evorule 规范的功能,LLM **不得立即执行**,必须按以下流程操作:

1. **明确警告**:告知人类该要求违背了 evorule 的哪些具体规范
   - 引用本文件(AGENTS.md)中的具体条款(硬规则 / 边界判断 / 核心定义)
   - 指出具体违反了哪条边界规则

2. **列出负面影响**:至少列出 3 条负面后果,例如:
   - 破坏核心纯净性,导致未来重构成本增加
   - 模糊机制与策略的边界,增加维护复杂度
   - 可能引发连锁反应,导致其他模块也跟着越界
   - 违反版本兼容性承诺,影响下游用户
   - 增加 TCB 攻击面,削弱安全保证

3. **等待确认**:得到人类**明确的二次确认**(例如"确认执行"、"我知道风险,继续")后,才可以执行

4. **记录留痕**:执行后在相关变更中说明"此变更是经用户明确要求并确认的越界操作"

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

- 新工具(放在 **evorule-application 仓**)
- 新 SDK 类型(typescript/python 之外)
- 新 UI / 可视化面板
- 新 agent 能力
- 新 HTTP 路由(除非是核心审计/治理必需)
- 新 I/O handler 集成(放在 **evo-agent 仓** 或 **evorule-application 仓**)
- 新配置格式(放在 **evorule-application 仓**)

### 边界判断

| 想法                             | 决策                                        |
| -------------------------------- | ------------------------------------------- |
| "加个新 transform 指令"          | 看是否 TCB 原语级,否则放 application        |
| "加个 Prometheus 指标"           | 放 evorule-application(可观测性是应用层)    |
| "优化 reactor 并发"              | evorule 核心(性能优化) ✅                   |
| "做个 web 仪表盘"                | **evorule-application 仓** ❌               |
| "加个新 LLM provider"            | evo-agent ❌                                |
| "加个 SQLite 集成"               | 看是 TCB 还是应用,大部分情况放 application  |
| "加新 Kani proof"                | evorule 核心 ✅                             |
| "加 README 章节"                 | evorule 核心 ✅                             |
| "修 bug"                         | evorule 核心 ✅                             |
| "evorule-cli 加 validate 子命令" | 核心已有 RuleValidator,CLI 只是封装 ✅      |
| "evorule-cli 加 Web UI"          | 应用层 UI,放 evorule-application ❌         |
| "evorule-cli 加 LLM 集成"        | 特定 I/O + 业务逻辑,放 evo-agent ❌         |
| "evorule-cli 加 diff 子命令"     | 核心已有时间机器 diff,CLI 只是封装 ✅       |
| "evorule-cli 加规则模板"         | 业务内容,不是机制,放 evorule-application ❌ |

## 改动前检查表

任何改动前,问自己:

1. 这是 **evorule 核心** 还是 **应用**?
2. 接受清单里有这一类吗?
3. 这个改动是让 evorule 更"纯净"还是更"复杂"?
4. 有没有更简单的实现方式(在 evorule 之外)?

如果 1-3 答不清:**不写代码,先写设计文档**(放 `文档/design/` 目录,本仓内)。

## 与其他项目的关系

- **evo-agent** (**evo-agent 仓**,与本仓为兄弟仓):用 evorule 的 AI agent 编排层。它是应用,不是核心。
- **evorule-application** (**evorule-application 仓**,与本仓为兄弟仓):可视化 / 调试器 / 仪表盘 / 业务规则模板等的应用集合。
- **evorule-server** (**evorule-server 仓**,与本仓为兄弟仓,走神 9 独立拆分):官方 HTTP server 实现,暴露 evorule 核心能力为远程 API。承载:
  - `evorule-server` 独立二进制(axum HTTP + SSE + Session 管理)
  - `core/io_handlers/`(DbHandler / HttpHandler / MemoryHandler 具体实现)
  - `core/auth`、`core/metrics`、`core/hot_reload`、`core/time_machine`、`core/debug_control`、`core/rule_tools`、`core/semantic_invariants` 共 7 个 server 配套 lib
  - 从 evorule-governance 迁出(H5)→ 从 evorule-application 再次迁出(走神 9)
- **evorule-sdk**:多语言客户端 SDK 独立仓。所有 SDK 都在外面,核心仓不含 SDK。
- **evorule-tcb / evorule-reactor / evorule-governance**:evorule 的三个 crate,**都属于核心**。G8 门控("反应器/治理层不得展开控制流")必须保留。
  - **H5**: evorule-governance 已移除具体 I/O handler(DbHandler/HttpHandler/MemoryHandler)和 evorule-server bin,现为纯机制层库(IoDispatcher 框架 + IoHandler trait re-export)。
- **evorule-cli**:核心仓内置的 CLI 工具,**属于核心**,但有严格边界:
  - 只做核心已有能力的命令行封装(tier0 + tier1 的能力)
  - 不引入新功能、不引入业务逻辑、不引入特定 I/O handler
  - 判断标准:这个功能 evorule-reactor 已经有了吗?有就可以包,没有就不能加
  - 扩展功能通过 Git 风格子命令发现机制实现(`evorule-xxx` 外部二进制)

## 版本与发布

- 当前基线:**0.1.0**(全生态统一,2026-07-20)
- 升 1.0 条件:见 [VERSION_STRATEGY.md §4.4](VERSION_STRATEGY.md)
- 第三方安全审计触发条件:见 [VERSION_STRATEGY.md §4.5](VERSION_STRATEGY.md)(VERSION_STRATEGY v1.1)
- 协议:AGPL-3.0-or-later(代码)+ CC0-1.0(`core_eval.json`)
- 内部审计:见 [docs/security/SECURITY_AUDIT_v0.1.0.md](docs/security/SECURITY_AUDIT_v0.1.0.md)(P0 全修复,P1 4 项 HIGH 待公网部署前修复)

## 内部约定

- **文档四层架构(L1→L4,严格单向)**
  - L1 正式发布层:根目录 `*.md` + `docs/**` + crate 根 README/SPEC — 公开可发布,有法律/版本约束力
  - L2 设计规范层:`文档/design/**` — 仓内共享,设计真意源,.gitignore 保护不发布
  - L3 实施细节层:`文档/implement/**` + `文档/benchmarks/**` — 仓内共享,实施方案/基准评估,.gitignore 保护
  - L4 本地私有层:`_PRIVATE_zh_docs/**` — junction 5 层防护,**绝对不 commit、绝对不在 L1/L2/L3 引用任何 L4 路径或文件名**
- **文档索引强制**:新增 L1 公开文档必须在 `DOCS_INDEX.md` 登记;新增 L2/L3 仓内文档必须在 `文档/DESIGN_INDEX.md` 登记;新增 L4 私有文档必须更新 `_PRIVATE_zh_docs/_README.md` 主题索引
- **版本号单一真相源**:`Cargo.toml` 顶层 `version = "X.Y.Z"` 是全仓唯一真相源;L1 文档写死的版本号字符串(README `v0.x.x` 徽章/STATUS/CHANGELOG/白皮书 P0 表格/DOCS_INDEX)必须与 `Cargo.toml` 对齐;CHANGELOG 历史段除外
- **L1 引用合规零容忍**:L1/L2/L3 文档禁止出现 `_PRIVATE_zh_docs/` 路径/文件名字面量;提交前过 `scripts/check_doc_safety.py`
- **`.trae/documents/` 7 天归档**:会话期方案成熟(对应代码合并/决策落定)后 7 天内必须搬迁到 L2/L3/L4 对应位置,禁止长期滞留在 `.trae/`
- **废弃文档必标横幅**:文档被新版取代时,顶部加 `[已废弃]` 横幅,注明「被 `<新文件名>` 于 `<YYYY-MM-DD>` 取代」,禁止直接删除留无痕迹消失
- **"文档/" 永不发布**(.gitignore 已保护)
- 中文为主,英文为辅
- 测试要能跑(`cargo test --workspace` 通过是底线)
- 提交前跑 `validate-all.ps1`(5 个 validate-\*.ps1 脚本 + `check_doc_safety.py`)
- CI 配在 `.gitee-ci/validate.yml`(Gitee 主仓库)+ `.github/workflows/`(ci.yml/release.yml/kani.yml/mutants.yml,GitHub 镜像)

## 已知坑

- PowerShell 5.1 + 无 BOM UTF-8 文件 = GBK 误读。**用 `add-spdx-safe.ps1` 加 SPDX 头,不要用 `Get-Content -Raw`**。服务器中文日志乱码也是此因,用 `pwsh` 或 `chcp 65001` 可缓解
- PowerShell 通过 cmd 调用时 `$` 变量会被外层 shell 吞掉(如 `powershell -Command "$h=@{}"` 失败)。**复杂脚本写 `.ps1` 文件再用 `-File` 执行**
- `cargo run` 在 sandbox 下输出的 exe 不在 `target/release/`,而在 `.build/rust/release/`。**用 `cargo run` 直接运行,不要手动找 exe**
- **H5 + 走神 9**: evorule-server 已两次外迁,现位于 **evorule-server 独立仓**(兄弟仓,顶层直接 workspace build),**不在核心 workspace 中**。
  - 运行方式:先克隆 evorule-server 仓(与 evorule 同级目录),再在 evorule-server 仓顶层 `cargo run --bin evorule-server -- --addr 127.0.0.1:18080`
- 二进制名用连字符(`evorule-server`),源文件 `evorule-server/src/main.rs`(evorule-server 独立仓顶层 crate)。
- evorule-server 默认监听 `0.0.0.0:18080`(非 loopback)。**测试时加 `--addr 127.0.0.1:18080`**
- 集成测试(`tests/integration_test.rs`)的 3 个 mock-LLM FAIL **已修复**(2026-07-20)
- tier1/ffi.rs 允许 unsafe(标了 `#![allow(unsafe_code)]`),其他 crate 必须 `#![forbid(unsafe_code)]`

## 下次开会看什么

- [ ] 战略方向 STRATEGIC_DIRECTION.md 有没有过时
- [ ] P1 安全修复进展(H6 SSRF / H7 SQL / H8 CORS / H9 DB URL)——公网部署前必修
- [ ] 0.2.0 上下文架构启动(evo-agent MemoryManager + fact log 索引)
- [ ] 下一个应用的时间旅行调试器设计稿

---

## 机器可读附录

本文件的硬规则 / 关系拓扑 / 元数据 / 已知坑 已结构化为 [AGENTS.schema.json](AGENTS.schema.json),供 LLM/agent 程序化消费。

**双轨制原则**(走神 6 校准):

- 人类读 `AGENTS.md`(narrative 优先,哲学/演进/故事不能丢)
- LLM/agent 读 `AGENTS.schema.json`(结构化优先,可靠性)
- 改了 AGENTS.md 后跑 `python scripts/agents-md-to-schema.py` 检查是否需要同步 schema
- CI 模式: `python scripts/agents-md-to-schema.py --check`(有 diff 退出码 1)
- schema 不替换 narrative,**互相补充**
