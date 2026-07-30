<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule 当前状态(2026-07-25)

> **TL;DR**:v0.1.0 公开基座。**能用,但有 caveat**。
>
> 这是面向**外部读者**的诚实记账。我们知道自己能跑什么、不能跑什么。

---

## v0.1.0 基线数据

| 维度           | 数据                                                                              | 来源                                       |
| -------------- | --------------------------------------------------------------------------------- | ------------------------------------------ |
| **代码量**     | tier0 (1.5K) + tier1 (~10K) + tier2 (~10K) + evorule-cli (~3K) = **~25K 行 Rust** | `git ls-files \| xargs wc -l`              |
| **测试**       | **731 passed + 4 ignored**(workspace 全部通过)                                    | `cargo test --workspace`                   |
| **警告**       | **0** Rust 警告(编译时 6 个 gate 全 PASS)                                         | `cargo check --workspace`                  |
| **依赖**       | 24 个直接依赖,0 已知 CVE                                                          | `docs/security/DEPENDENCY_AUDIT_v0.1.0.md` |
| **审计链**     | blake3 哈希链,自洽,export/import 完整,P04-P06 改进已通过评估                      | `docs/security/SECURITY_AUDIT_v0.1.0.md`   |
| **可复现构建** | x86_64 + aarch64 musl,SHA256 验证 PASS                                            | `.gitee-ci/build-musl.yml`                 |
| **CLI 体积**   | 1.6MB stripped(x86_64) / 1.4MB(aarch64)                                           | `build-musl.sh --check`                    |

---

## 工作(✅ — 知道能跑什么)

### 核心机制

- [x] **JSON 数据集执行** — 接受 JSON,执行 JSON,产生 JSON 事实账本
- [x] **确定性** — 相同输入永远产生相同输出(已用 13 条规则验证)
- [x] **反应式** — `set` / `push` / `branch` / `io_request` 4 个元指令驱动
- [x] **因果链** — 每个 StateTransition 指向 cause,可正向反向遍历
- [x] **时间旅行** — replay / rewind / fork / diff 4 个 API 完整

### 审计链(P01-P06)

- [x] **P01 完整性** — blake3 链,自 genesis 起每个 fact 链接 prev_hash
- [x] **P02 持久化** — WAL 重启后能恢复审计状态
- [x] **P03 文件轮换** — 100MB 上限,自动 rotate
- [x] **P04 导出/导入** — JSON 往返一致,verify 通过(1478 bytes 样本)
- [x] **P05 gzip 压缩** — 50.3% 压缩率,gzip magic 验证 PASS
- [x] **P06 实时验证** — `--auto-verify --auto-verify-threshold 100` 全程无失败

### 独立 CLI(`evorule-cli`)

- [x] **4 个子命令** — validate / run / replay / diff
- [x] **6 个子命令测试** — 全部通过
- [x] **19 个 e2e 测试** — 全部通过(TAP 格式)
- [x] **musl static** — x86_64 + aarch64,reproducible(SHA256 验证)
- [x] **Payload via file** — `--payload-file` 解决 PowerShell `"` 误读

> **HTTP API 端点 / 调试器 UI / 指标 / evorule-server 二进制** 已归应用层，见 evorule-application 仓。
> **evo-agent 内置工具集**（file_read / shell_exec / http_get 等）已归 agent 层，见 evo-agent 仓。

### 安全(自评,未独立审计)

- [x] **SECURITY_AUDIT v0.1.0** — 走神 9 拆分后 evorule 仓独立范围（纯机制层）；P0 全修复；P1 随 io_handlers 迁移至 application 仓修复
- [x] **THREAT_MODEL v0.1.0** — STRIDE + Attack Trees + DFD（evorule 仓范围）
- [x] **DEPENDENCY_AUDIT v0.1.0** — cargo-audit 实跑，0 CVE 0 warnings
- [x] **M1 Bearer token / M3 工具调用 / M4 注册校验** — 属 evorule-server + evo-agent 应用层，在对应仓安全文档中跟踪
- [x] **tier0 编译时门禁** — `#![deny(unwrap_used)]` / `#![deny(expect_used)]` / `#![deny(indexing_slicing)]` / `#![deny(panic)]`
- [x] **tier0/1/2 build.rs 门禁** — T1/T2/T3/T15 全开

### 文档

- [x] **README** — 5 原则 + CLI quickstart + 一句话定位
- [x] **VERSION_STRATEGY** — 12 章节发版标准
- [x] **DESIGN_PRINCIPLES** — 5 属性 checklist
- [x] **STRATEGIC_DIRECTION** — 3 圈战略 + 框架 vs 应用边界
- [x] **5 个 PowerShell 校验脚本** — SemVer / CHANGELOG / License / Cargo.lock / Tag

---

## 不工作 / 弱项(❌ — 知道不能跑什么)

### 不做(明确不做)

- ❌ **向后兼容承诺** — 0.x 阶段不承诺,1.0 才承诺
- ❌ **生产级 SLA** — 没有性能/可用性数字
- ❌ **第三方安全审计** — 已完成内部自审(P0 全修复),1.0 才做第三方

### 不能跑

- ❌ **跨平台 release 验证** — 28 号评估只跑过 Windows + localhost
- ❌ **Gitee Go CI 流水线实测** — `.yml` 写了但没真跑过
- ❌ **GZIP + PowerShell 兼容** — PowerShell 5.1 + Invoke-WebRequest 会自动解压 gzip,需要 `--output` 或 .NET HttpClient
- ❌ **Public demo 视频** — 有评估文档,没 GIF/视频

### 还没写

- ✅ **L9 Kani 部分真实证明** — 5 proof, 4/5 PASS
  - 4 PASS: `verify_value_roundtrip` / `verify_set_integer_safety` / `verify_transition_bounded` / `verify_set_sub_safety`
  - 剩余 1 个 `verify_path_no_panic` 因 Kani 工具链对 `BTreeMap` 内部 `correct_childrens_parent_links` 与 `memcmp` 的默认 unwind bound 不够而 5min TIMEOUT
  - 改由 proptest `resolve_path_never_panics_arbitrary_path` 保底覆盖，等 Kani 0.68+ 修复
  - 原 `verify_domain_boolean` 已删除(改由 proptest `domain_eval_never_panics_arbitrary_type` 替代)

- ❌ **cargo-audit 自动跑** — `kstring@2.0.4` 要求 rustc 1.96,本机 1.92,装不上
- ❌ **第三方代码 review** — 团队之外没人看过
- ❌ **3 个 mock-LLM integration test** — **已修** ✅(2026-07-20)
- ❌ **168 个 missing_docs 警告** — **已修** ✅(2026-07-20)

### 已知问题(P1 4 项 HIGH + LOW 若干)

见 [docs/security/SECURITY_AUDIT_v0.1.0.md](docs/security/SECURITY_AUDIT_v0.1.0.md) — H6 SSRF / H7 SQL / H8 CORS / H9 DB URL,公网部署前必修;LOW 不阻塞 0.1.0。

---

## 下一步(诚实记账)

### 阶段 0(已完成) — 公开基座

- [x] Gitee 仓库 URL/凭证
- [x] git remote + git push
- [x] v0.1.0 基线统一(版本号/文档/审计全部对齐)

### 阶段 1(1-2 周) — 验证 + 性能基准

- [ ] 性能基准(并发 session / 长 session / 压缩吞吐量)
- [ ] 跨平台冷启动(Linux / WSL)
- [ ] 性能基准评估报告（仓内 L3 文档，不对外发布）
- [ ] evorule-server musl release 端到端测试 — 见 evorule-application 仓

### 阶段 2(4-6 周) — 上下文架构

- [ ] evo-agent MemoryManager 升级 — 见 evo-agent 仓
- [ ] evorule fact log 索引优化
- [ ] 长期记忆压缩

### 阶段 3(8+ 周) — 应用层 P0

- [ ] Time-Travel Debugger v1(D 计划)— 见 evorule-application 仓
- [ ] Audit Inspector(可选)— 见 evorule-application 仓

### 后续(0.1.0 首发后)

- [ ] demo 视频
- [ ] P1 安全修复(SSRF/SQL/CORS/DB URL)— 在 evorule-application 仓完成
- [x] 打 `v0.1.0` tag（evorule 仓首发）+ 四 crate 同步发布 crates.io
- [ ] v0.2.0 质量硬化（Kani 证明覆盖、CI 跑通）

---

## 给用户的承诺

**0.1.0 期间**:

- 我们**能跑** CLI + 3 个 lib crate（evorule-tcb / evorule-reactor / evorule-governance）
- **evorule-server / HTTP API / 调试器 UI / evo-agent 工具集** → 请使用 evorule-application 仓 + evo-agent 仓
- 我们**不能**承诺 API 稳定
- 我们**不修**新发现的安全问题(除非 critical)
- 我们**鼓励** 提 issue / 提 PR,但**不承诺响应时间**

**1.0.0 之后**:

- 上述全部反过来

---

**最后更新**:2026-07-30
**下次更新**:v0.1.0 首发 tag 后
