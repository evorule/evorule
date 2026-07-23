<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule 当前状态(2026-07-20)

> **TL;DR**:0.1.0-alpha.1 公开基座。**能用,但有 caveat**。
>
> 这是面向**外部读者**的诚实记账。我们知道自己能跑什么、不能跑什么。

---

## 0.1.0-alpha.1 基线数据

| 维度 | 数据 | 来源 |
|---|---|---|
| **代码量** | tier0 (1.5K) + tier1 (~10K) + tier2 (~10K) + evorule-cli (~3K) = **~25K 行 Rust** | `git ls-files \| xargs wc -l` |
| **测试** | evorule 158/158 + 4/4 integration + 19/19 e2e | `cargo test --workspace` |
| **警告** | **0** Rust 警告(编译时 6 个 gate 全 PASS) | `cargo check --workspace` |
| **依赖** | 25 个第三方 crate,0 已知 CVE | `docs/security/DEPENDENCY_AUDIT_v0.1.0.md` |
| **审计链** | blake3 哈希链,自洽,export/import 完整,P04-P06 改进已通过评估 | `文档/28_evorule服务器评估.md` |
| **启动** | 162ms (Windows + localhost,release 构建) | `文档/28_evorule服务器评估.md` |
| **可复现构建** | x86_64 + aarch64 musl,SHA256 验证 PASS | `.gitee-ci/build-musl.yml` |
| **CLI 体积** | 1.6MB stripped(x86_64) / 1.4MB(aarch64) | `build-musl.sh --check` |

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

### HTTP API(30+ 端点)
- [x] **健康检查** — `/api/health` `/api/health/liveness` `/api/health/readiness`
- [x] **会话** — create / list / state / facts / history / payload
- [x] **命令** — submit / io_response
- [x] **审计** — audit / audit/verify / audit/causal/{fact_id} / audit/export[/compressed] / audit/import[/compressed]
- [x] **时间机器** — replay / rewind/{v} / diff?a=&b= / fork/{parent}
- [x] **调试** — debug/phase / debug/queue / debug/pending_io
- [x] **指标** — `/metrics` (Prometheus 格式)
- [x] **调试器 UI** — `/debugger/debugger.html` v0 sketch

### 独立 CLI(`evorule-cli`)
- [x] **4 个子命令** — validate / run / replay / diff
- [x] **6 个子命令测试** — 全部通过
- [x] **19 个 e2e 测试** — 全部通过(TAP 格式)
- [x] **musl static** — x86_64 + aarch64,reproducible(SHA256 验证)
- [x] **Payload via file** — `--payload-file` 解决 PowerShell `"` 误读

### 工具集(`evo-agent` 内置 6 个)
- [x] **file_read / file_list / file_write / search_files** — workdir sandbox,大小限制
- [x] **shell_exec** — 8 active + 20 candidate + 28 blocked + propose 协议
- [x] **http_get** — 6 个 active hosts + SSRF blocklist
- [x] **3 层安全模型** — active/candidate/blocked 统一语义

### 安全(自评,未独立审计)
- [x] **M1 Bearer token** — `--auth-token` / `EVORULE_AUTH_TOKEN`,非 loopback 启动警告
- [x] **M3 工具调用** — 已在 Fact log(`IoRequest` + `IoResponse`)
- [x] **M4 工具注册校验** — `AgentRunner::from_definition` 早失败
- [x] **SECURITY_AUDIT v0.1.0** — 4 medium closed, 11 LOW documented
- [x] **THREAT_MODEL v0.1.0** — 14 章节,7 attack trees,STRIDE per component
- [x] **DEPENDENCY_AUDIT v0.1.0** — 25 deps,0 known CVEs

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
- ❌ **第三方安全审计** — 自审 + 修正历史,1.0 才做

### 不能跑
- ❌ **跨平台 release 验证** — 28 号评估只跑过 Windows + localhost
- ❌ **Gitee Go CI 流水线实测** — `.yml` 写了但没真跑过
- ❌ **GZIP + PowerShell 兼容** — PowerShell 5.1 + Invoke-WebRequest 会自动解压 gzip,需要 `--output` 或 .NET HttpClient
- ❌ **Public demo 视频** — 有评估文档,没 GIF/视频

### 还没写
- ✅ **L9 Kani 部分真实证明** — 5 proof, 4/5 PASS(`verify_value_roundtrip` / `verify_set_integer_safety` / `verify_transition_bounded` / `verify_set_sub_safety`);剩余 1 个 `verify_path_no_panic` 因 Kani 工具链对 `BTreeMap` 内部 `correct_childrens_parent_links` 与 `memcmp` 的默认 unwind bound 不够而 5min TIMEOUT,改由 proptest `resolve_path_never_panics_arbitrary_path` 保底覆盖,等 Kani 0.68+ 修复。原 `verify_domain_boolean` 已删除(改由 proptest `domain_eval_never_panics_arbitrary_type` 替代)
- ❌ **cargo-audit 自动跑** — `kstring@2.0.4` 要求 rustc 1.96,本机 1.92,装不上
- ❌ **第三方代码 review** — 团队之外没人看过
- ❌ **3 个 mock-LLM integration test** — **已修** ✅(2026-07-20)
- ❌ **168 个 missing_docs 警告** — **已修** ✅(2026-07-20)

### 已知问题(LOW,11 条)
见 [docs/security/SECURITY_AUDIT_v0.1.0.md](docs/security/SECURITY_AUDIT_v0.1.0.md) — L1-L11,均不阻塞 0.1,推迟到 0.2.0。

---

## 下一步(诚实记账)

### 阶段 0(本周) — 公开基座 push
- [ ] Gitee 仓库 URL/凭证(等用户)
- [ ] git remote + git push
- [ ] 暂不打 tag

### 阶段 1(1-2 周) — 验证 + 性能基准
- [ ] evorule-server musl release 端到端测试
- [ ] 性能基准(并发 session / 长 session / 压缩吞吐量)
- [ ] 跨平台冷启动(Linux / WSL)
- [ ] docs/benchmarks/RESULTS.md

### 阶段 2(4-6 周) — 上下文架构
- [ ] evo-agent MemoryManager 升级
- [ ] evorule fact log 索引优化
- [ ] 长期记忆压缩

### 阶段 3(8+ 周) — 应用层 P0
- [ ] Time-Travel Debugger v1(D 计划)
- [ ] Audit Inspector(可选)

### 阶段 4(2-3 周) — 正式 v0.1.0 release
- [ ] README banner 去掉
- [ ] demo 视频
- [ ] 手写 release notes
- [ ] 重跑全部验证
- [ ] 打 `v0.1.0` tag(不是 alpha)

---

## 给用户的承诺

**0.1.0-alpha.1 期间**:
- 我们**能跑** server / CLI / tools
- 我们**不能**承诺 API 稳定
- 我们**不修**新发现的安全问题(除非 critical)
- 我们**鼓励** 提 issue / 提 PR,但**不承诺响应时间**

**1.0.0 之后**:
- 上述全部反过来

---

**最后更新**:2026-07-20
**下次更新**:阶段 0 push 后 1 周内
