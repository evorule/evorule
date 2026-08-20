<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published
  by the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# 有所得，必有所失：EvoRule 的取舍立场

> **立场总纲 · 一切推广叙事的第一篇**
> 这篇不讲功能，不讲"能做到什么"。它先回答一个更根本的问题：**EvoRule 凭什么敢于做减法？**
> 放在它后面的，才是技术论证（《为什么需要"只接受 JSON"的执行引擎》）与上手教程（《快速开始》）。

---

## 一、承认没有免费的午餐

工程世界里，我见过太多"全都给你"的系统：又要通用、又要高性能、又要好上手、又要可审计、又要分布式、又要 AI 原生……结果往往是——**什么都沾一点，什么都没给够。**

我越来越相信一句朴素的老话：**有所得，必有所失。**

你不可能既要确定性，又要无限的表达自由；既要透明可审计，又要隐藏实现捷径；既要纯净克制，又要无所不能。

**EvoRule 的立场，就是直面这个"得与失"的算术题，并且把答案摊开在桌面上。**

---

## 二、EvoRule 选择了什么，放弃了什么

我把 EvoRule 的取舍写成一笔诚实的分账：

| 我得到的（得） | 我为此放弃的（失） |
|---|---|
| **确定性** —— 同样的输入，必然同样的输出 | **表达力** —— 无 Lambda、无复杂类型推导；JSON 很"笨" |
| **可审计** —— 每一步有因果链，账本可回放可验真 | **便利捷径** —— 没有隐藏的 DSL、没有"藏着"的逻辑 |
| **透明** —— 规则、状态、事件全是 JSON，可直接读写 | **抽象花招** —— 不发明让人看不懂的专用语法 |
| **纯净** —— 机制层最小化，零 unsafe，三层严格分层 | **全能** —— 不是通用规则引擎、不是工作流平台、不是分布式数据库 |
| **可证明** —— Kani / TLA+ 把不变量钉死 | **捷径** —— 很多事"重做一遍"比"偷懒一次"更值得 |
| **克制** —— 只做执行，不含 LLM、记忆、规划 | **虚名** —— 不蹭"AI 框架"的热点 |

> 一句话：**我用"功能上的贫瘠"，换来了"信任上的富足"。**

---

## 三、为什么敢于做减法

有人会问：**你主动砍掉这么多，不是自断手脚吗？**

我的回答藏在 README 里那句反复强调的话里：**"这是边界，不是 bug。"**

- JSON 表达力有限 —— **是边界，不是 bug**。因为它正是透明和可审计的代价，是刻意为之。
- 不是通用规则引擎 —— **是边界，不是 bug**。因为"只接受 JSON"这条铁律，本身就是差异化的来源。
- 不是 AI Agent 框架 —— **是边界，不是 bug**。因为 EvoRule 的价值是"给 LLM 一个可信任的执行层"，而不是替你做 LLM 该做的事。

**"不是什么"定义了"是什么。"** 一个项目最清晰的时刻，不是它宣布自己能做什么，而是它诚实地承认自己拒绝做什么。

克制，恰恰是信任的来源。因为只有敢说"我不做"的系统，你才敢把重要的东西交给它。

---

## 四、这种取舍，对你意味着什么

选了 EvoRule，你会——

**得到：**
- 一份你能**亲自读懂**的规则（JSON，不藏私）
- 一份**可回放、可对比、可验真**的审计账本
- 一个**形式化可证明**的确定性内核
- 一个**业务与工程解耦**的干净边界

**失去：**
- 花哨的表达方式（无 Lambda，请接受 JSON 的朴素）
- "跑得更快一点"的诱惑（确定性优先于极致性能）
- "什么都能干"的想象（它只干执行这一件事，且干得彻底）

它**不适合**想要"一个库解决所有问题"的人；它**适合**那些愿意为**可解释、可审计、可信任**付出一点代价的人——合规、审计、确定性工作流、需要把 LLM 输出落到可靠执行层的场景。

---

## 五、收尾

README 的开篇写着：

> _规则不言语。它们只运行。而我们是首批见证者。_

EvoRule 的立场，就是相信**"少即是可信"**：敢于失去，才配得到最值得的东西。

> **有所得，必有所失。**
> **EvoRule 选择失去"全能"，来得到"可信"。**
>
> —— 没有智能，只有执行。确定性执行，可回溯，可审计。
