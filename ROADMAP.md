<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  EvoRule 公开路线图
  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 公开路线图

> **当前状态**:v0.1.0 公开基座(2026-07-20)。详见 [STATUS.md](STATUS.md)。
>
> 本文档是面向**外部读者**的精简版;安全审计详见 [`docs/security/SECURITY_AUDIT_v0.1.0.md`](docs/security/SECURITY_AUDIT_v0.1.0.md)。

---

## 一句话定位

**EvoRule = 只接受和运行 JSON 数据集的反应式执行引擎。**

它不发明新的 DSL,不把规则"编译"成代码,不藏业务逻辑在 `.py` / `.ts` 文件里。
它只做一件事:**接受 JSON,执行 JSON,产生可审计的事实账本。**

智能由应用层(evo-agent / evorule-application)提供。EvoRule 本身**没有智能,只有执行的最佳实践**(确定性 + 反应式 + 完整审计 + 时间旅行)。

---

## 版本语义

| 版本号         | 含义                              | 承诺                                                   |
| -------------- | --------------------------------- | ------------------------------------------------------ |
| **0.1.0** 当前 | first public preview / alpha      | 能跑、知道自己能跑什么、知道自己不能跑什么、API 可能变 |
| **0.2.0** 下一 | 上下文架构 + L9 Kani 剩余 1 proof | API 大致稳定,但仍可能有 breaking                       |
| **0.3.0**      | 应用层 P0 完成                    | API 接近稳定,开始考虑兼容承诺                          |
| **1.0.0** 远期 | production-grade                  | API 冻结、SLA、第三方安全审计                          |

**承诺**:在 1.0.0 之前,**不承诺向后兼容**。这是 semver 给 0.x 的本意,也是给项目"自由演进"的红利。

---

## 当前阶段:0.1.0 公开基座(2026-07-20 起)

### 已经能做的

- ✅ **JSON 数据集** — 接受/执行/产生,所有规则、状态、事件都是 JSON
- ✅ **blake3 审计链** — 因果完整,可验证,可导出/导入,可压缩
- ✅ **时间机器** — replay / rewind / fork / diff 后端 production-ready
- ✅ **HTTP API** — 40+ 端点,完整覆盖会话/命令/状态/审计/调试
- ✅ **独立 CLI** — `evorule-cli` 1.6MB musl static binary (x86_64 + aarch64)
- ✅ **Gitee CI** — 验证 / 构建 musl / 验证可复现
- ✅ **3 层安全工具集** — 6 个 builtin tools (active/candidate/blocked) + propose 协议

### 还不行的(诚实记账)

- ❌ **第三方安全审计** — 已完成内部自审(P0 5 项全修复,P1 4 项 HIGH 待公网部署前修复),1.0 之前不做第三方
- 🟡 **L9 Kani 部分真实证明** — 5 proof, 4/5 PASS + 19 proptest(2026-07-23 更新, Kani 0.67.0);4 PASS(i64 加/减不上溢、JsonValue 类型安全、状态转换有界);1 个 verify_path_no_panic 因 Kani 工具链 alloc std unwind bound 限制 TIMEOUT(改由 proptest `resolve_path_never_panics_arbitrary_path` 保底);删除 verify_domain_boolean(改由 proptest `domain_eval_never_panics_arbitrary_type` 替代);新增 5 个 proptest。详见 [`tier0-tcb/TCB_SPEC.md`](tier0-tcb/TCB_SPEC.md)
- ❌ **公开 demo 视频** — 有评估文档,没 GIF/视频
- ❌ **依赖自动审计** — cargo-audit 装不上(rustc 1.92 vs kstring 1.96 要求)
- ❌ **应用层 killer app** — 时间旅行调试器在 0.2.0 之后

### 阶段 0 详细清单

见 [`STATUS.md`](STATUS.md) — v0.1.0 基线状态 + 诚实记账。

---

## 阶段 1:0.2.0 上下文架构(预计 4-6 周)

**目标**:加上"会话记忆 + 长期记忆",这是 0.1 → 0.2 唯一值得做架构变更的事。

- evo-agent MemoryManager 升级(自动 recall 策略、压缩、去重)
- evorule fact log 索引优化(支持跨 session 命名空间查询)
- 长期记忆压缩机制(防止无限膨胀)
- L9 Kani 剩余 1 proof(`verify_path_no_panic`)— 跟踪 Kani 0.68+ alloc std unwind bound 修复

---

## 阶段 2:0.3.0 应用层 P0(预计 8+ 周)

**目标**:至少一个 killer app,让 EvoRule 不再是"裸基座"。

候选(按 ROI 排):

- **Time-Travel Debugger** — 设计稿规划中(rewind/replay/diff/fork 已在后端就绪)
- **Audit Inspector** — blake3 链可视化
- **Live Monitor** — 实时跑 session

预计先做 Time-Travel Debugger。

---

## 阶段 3:1.0.0 production(预计 6-12 月)

**目标**:配得上"production-grade"的承诺。

- 第三方安全审计
- 公开 API 稳定承诺
- SLA(性能 / 可用性)
- 完整 benchmark suite(自动化)
- 应用层至少 2 个杀手级 demo
- 企业级部署文档(Docker / k8s / 私有云)

---

## 三个用户圈(战略目标)

EvoRule 战略上服务三类用户,**优先级递进**:

### 圈 1:中小程序员(BYOK,免费)

- 痛点:已有 IDE + AI 工具,缺一个"确定性 + 可审计"的小引擎
- 卖点:JSON 规则 + 零依赖 + 编译时门禁
- 阶段:0.1.0 起步,0.3.0 完善

### 圈 2:合规刚需(隐私敏感,**杀手级**)

- 痛点:想自动化但不敢上云(医疗/律所/金融/政务)
- 卖点:**本地执行 + 零数据泄露 + 完整审计 + blake3 防篡改**
- 杀手 demo:`evorule run ./rules/` → 30 秒赢得监管严格行业
- 阶段:**0.1.0 就要打**;0.2.0 加硬化(审计加固、Air-gapped 模式)

### 圈 3:企业级(等保/SOX/HIPAA)

- 痛点:合规 + 审计 + 权限分级 + SLA
- 卖点:和圈 2 一样 + 多租户 + RBAC + 审计仪表盘
- 阶段:1.0.0 才正式开

---

## 4 人团队分工(2026-05 共识)

| 角色                    | 责任                                                  |
| ----------------------- | ----------------------------------------------------- |
| **你 / Tech Lead**      | 架构决策 + 跨模块接口 + 6 个核心工具实现 + dogfooding |
| **R1 Engine Engineer**  | tier0/tier1 内核扩展 + codegraph + 约束生成           |
| **R2 Tooling Engineer** | LLM 适配器 + 工具系统 + Diff/Patch + 沙箱             |
| **R3 Product Engineer** | CLI + 对话 UX + 任务规划层 + 文档                     |

---

## 时间表(总览)

```
2026-07 ──── 2026-09 ──── 2026-11 ──── 2027-01 ──── 2027-Q2
│           │            │            │            │
0.1.0       0.2.0        0.3.0        0.4.0        1.0.0
公开基座    上下文架构    Time-Travel   审计+硬化    production
+ 收尾     + L9 Kani     Debugger     + 多租户    + 第三方审计
                          + Audit
                          Inspector
```

具体每月详见 [`STATUS.md`](STATUS.md) 的发版 checklist。

---

## 反馈渠道

- **Gitee Issue**:https://gitee.com/evorulelab/evorule/issues(主渠道)
- **邮箱**:evorulelab@gmail.com
- **安全漏洞**:见 [SECURITY.md](SECURITY.md)

---

**作者**:EvoRule Project
**协议**:AGPL-3.0-or-later(代码) + CC0-1.0(`core_eval.json` 宪法)
**最后更新**:2026-07-25
