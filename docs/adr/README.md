<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 架构决策记录(ADR)

> 记录 evorule 基础仓中**重要的架构决策**及其历史。

## 什么是 ADR

ADR(Architecture Decision Record)是一种轻量级文档,记录:

- 我们面临什么问题
- 考虑了哪些方案
- 最终选了哪个、为什么
- 选完后带来哪些后果

每一份 ADR 都是**不可变的历史快照**。决策若变更,写新 ADR 并 supersede 旧的,**不要回头改旧文件**。

## 写作规范

- **文件名**:`NNNN-kebab-case-title.md`,序号单调递增,不重用
- **模板**:复制 [template.md](./template.md) 开始写
- **状态**:Proposed → Accepted → (Deprecated | Superseded by ADR-XXXX)
- **完成后**:把新 ADR 链接加进下方目录

## 目录

- [ADR-0001: 验证状态单一真相源与诚实披露机制](./ADR-0001-验证状态单一真相源与诚实披露机制.md)(2026-09-12,Accepted)

## 何时写 ADR

- 选了某个框架 / 库 / 语言版本(如"为什么 Rust 1.74+ 而非 nightly")
- 引入或修改了**架构层面**的设计(如"TCB 划界原则")
- 改变了产品边界或外部接口(如"v0.3 改外部服务 API")
- 拒绝了某个看似合理的方案(如"不做实时协作,理由是 …")

**不要**为琐碎的实现细节写 ADR(变量命名、内部重构不算)。

---

<a id="english"></a>

# Architecture Decision Records (ADR)

> Record the **important architecture decisions** made in the evorule core repository, together with their history.

## What is an ADR

An ADR (Architecture Decision Record) is a lightweight document that records:

- What problem we were facing
- Which options were considered
- Which one was chosen in the end, and why
- What consequences the choice brought

Every ADR is an **immutable historical snapshot**. If a decision changes, write a new ADR and supersede the old one — **never go back and edit the old file**.

## Writing conventions

- **File name**: `NNNN-kebab-case-title.md`, numbered with monotonically increasing sequence numbers that are never reused
- **Template**: copy [template.md](./template.md) to start writing
- **Status**: Proposed → Accepted → (Deprecated | Superseded by ADR-XXXX)
- **When done**: add the new ADR to the index below

## Index

- [ADR-0001: Single Source of Truth for Verification Status and Honest Disclosure](./ADR-0001-验证状态单一真相源与诚实披露机制.md) (2026-09-12, Accepted)

## When to write an ADR

- A framework / library / language version was chosen (e.g. "why Rust 1.74+ rather than nightly")
- An **architecture-level** design was introduced or changed (e.g. "TCB boundary principles")
- The product boundary or an external interface changed (e.g. "v0.3 changed the external service API")
- A seemingly reasonable option was rejected (e.g. "no real-time collaboration, because …")

**Do not** write an ADR for trivial implementation details (variable naming and internal refactoring do not count).
