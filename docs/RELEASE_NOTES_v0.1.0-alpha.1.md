<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule v0.1.0-alpha.1 Release Notes

**Release Date**: 2026-07-20
**Tag**: `v0.1.0-alpha.1`
**Status**: ⚠️ **First Public Preview — NOT production-ready**

---

## 一句话总结

这是 EvoRule **第一次公开亮相**。能跑核心 API、blake3 审计链、musl static CLI,但**不承诺 API 稳定**,**不承诺安全审计**,**不承诺 SLA**。

适合:
- ✅ 围观 / 评估 / 给反馈
- ✅ 跑通 demo / 教学
- ✅ 提 issue / 提 PR
- ❌ 生产环境(等 1.0)
- ❌ 合规场景(等 0.2.0 + 第三方审计)

---

## 🎉 这个版本有什么

### 核心机制(JSON 执行引擎)

- ✅ **接受 JSON,执行 JSON,产生 JSON** — 规则 / 状态 / 事件 / I/O 全部 JSON
- ✅ **3.5 个元指令** — `set` / `push` / `branch` / `io_request`
- ✅ **6 个基本域** — Boolean / Integer / Decimal / String / Array / Object / Null
- ✅ **确定性** — same input → same output,always
- ✅ **反应式** — drain → stable → block → execute 主循环
- ✅ **因果链** — 每个 StateTransition 指向 cause

### 审计链(P01-P06)

- ✅ **P01 完整性** — blake3 哈希链,genesis 起每个 fact 链 prev_hash
- ✅ **P02 持久化** — WAL 重启恢复
- ✅ **P03 文件轮换** — 100MB 上限,自动 rotate
- ✅ **P04 导出/导入** — JSON 往返一致,verify 通过
- ✅ **P05 gzip 压缩** — 50.3% 压缩率,大文件推荐
- ✅ **P06 实时验证** — `--auto-verify` 全程校验

### HTTP API(30+ 端点)

- 健康检查:`/api/health`,`/api/health/liveness`,`/api/health/readiness`
- 会话:`/api/sessions`,`/api/sessions/{id}/state`
- 命令:`/api/sessions/{id}/command`,`/api/sessions/{id}/io_response`
- 审计:`/api/sessions/{id}/audit[/verify|/causal/{fact_id}|/export|/import]`
- 时间机器:`/api/sessions/{id}/replay`,`/rewind/{v}`,`/diff?a=&b=`,`/fork/{parent}`
- 调试:`/api/sessions/{id}/debug/{phase,queue,pending_io}`
- 指标:`/metrics` (Prometheus 格式)
- 调试器 UI:`/debugger/debugger.html` (v0 sketch)

### 独立 CLI(`evorule-cli`,musl static)

- ✅ `validate` — 校验 JSON 规则
- ✅ `run` — 执行规则集
- ✅ `replay` — 重放历史
- ✅ `diff` — 对比两个版本
- ✅ **体积**:1.6MB stripped(x86_64)/ 1.4MB(aarch64)
- ✅ **可复现构建**:`SOURCE_DATE_EPOCH` + SHA256 验证

### 工具集(`evo-agent` 内置 6 个)

- ✅ `file_read` / `file_list` / `file_write` / `search_files` — workdir sandbox
- ✅ `shell_exec` — 8 active + 20 candidate + 28 blocked + propose 协议
- ✅ `http_get` — 6 个 active hosts + SSRF blocklist
- ✅ **3 层安全模型**:active / candidate / blocked 统一语义

### 安全(自评,未独立审计)

- ✅ **M1 Bearer token** — `--auth-token` / `EVORULE_AUTH_TOKEN`
- ✅ **M3 工具调用** — 已在 Fact log
- ✅ **M4 工具注册校验** — `from_definition` 早失败
- ✅ **SECURITY_AUDIT** — 4 medium closed,11 LOW documented
- ✅ **THREAT_MODEL** — 14 章节,7 attack trees
- ✅ **DEPENDENCY_AUDIT** — 25 deps,0 known CVEs

### 文档

- ✅ `README.md` — 5 原则 + CLI quickstart
- ✅ `ROADMAP.md` — 公开路线图
- ✅ `STATUS.md` — 当前状态诚实记账
- ✅ `VERSION_STRATEGY.md` — 12 章节发版标准
- ✅ `DESIGN_PRINCIPLES.md` — 5 属性 checklist
- ✅ `STRATEGIC_DIRECTION.md` — 3 圈战略
- ✅ `docs/security/` — SECURITY_AUDIT / THREAT_MODEL / DEPENDENCY_AUDIT
- ✅ `docs/PLAN_v0.1.0-alpha.md` — 阶段 0-4 发版计划
- ✅ `docs/benchmarks/EVAL_2026-07-20.md` — release 模式评估

---

## 🐛 已知问题(诚实记账)

### 不做(明确)

- ❌ **API 向后兼容** — 0.x 阶段不承诺,1.0 才承诺
- ❌ **生产 SLA** — 没有性能 / 可用性数字承诺
- ❌ **第三方安全审计** — 1.0 才做

### 不能跑 / 还没做

- ❌ **跨平台 release 没真测过** — 只在 Windows + localhost 跑过
- ❌ **Gitee Go CI 流水线没真跑过** — `.yml` 写了但没 push 验证
- ❌ **公开 demo 视频** — 有评估文档,没 GIF
- ❌ **L9 Kani 真实证明** — 5 个 stub,不是真证明
- ❌ **cargo-audit 自动跑** — `kstring@2.0.4` 要求 rustc 1.96,本机 1.92,装不上

### LOW 11 条(不阻塞 0.1,推迟到 0.2)

见 [`docs/security/SECURITY_AUDIT_v0.1.0.md`](security/SECURITY_AUDIT_v0.1.0.md) §"L1-L11"。

---

## 📊 数字

| 指标 | 数据 | 测试方法 |
|---|---|---|
| 启动时间 | 162ms | Windows + localhost,release 构建 |
| 核心 API | 30+ 端点 | 28 号评估,200 OK |
| 审计链 | 1478 bytes 样本 | export → 744 bytes gzip(50.3%) |
| 实时验证 | 100% 通过 | 28 号评估 |
| 警告 | 0 | `cargo check --workspace` |
| 测试 | 158/158 + 4/4 integration + 19/19 e2e | `cargo test --workspace` |
| CLI 体积 | 1.6MB (x86_64) / 1.4MB (aarch64) | `build-musl.sh --check` |
| 可复现 | ✅ | SHA256 验证 PASS |
| 依赖 | 25 个,0 known CVE | `docs/security/DEPENDENCY_AUDIT_v0.1.0.md` |

---

## 🚀 快速试一下

### 装 evorule-cli(musl static)

```bash
# 下载 prebuilt(发版后才有)
wget https://gitee.com/evorulelab/evorule/releases/download/v0.1.0-alpha.1/evorule-cli-x86_64-linux-musl
chmod +x evorule-cli-x86_64-linux-musl
./evorule-cli-x86_64-linux-musl --help
```

### 跑 evorule-server

```bash
git clone https://gitee.com/evorulelab/evorule.git
cd evorule
cargo build --release --bin evorule-server
./target/release/evorule-server --addr 127.0.0.1:18081
```

### 试一个 JSON 规则

```bash
curl -X POST http://127.0.0.1:18081/api/sessions
# {"session_id":1}

curl -X POST http://127.0.0.1:18081/api/sessions/1/command \
  -H "Content-Type: application/json" \
  -d '{"instruction":{"type":"set","params":{"path":"hello","value":"world"}}}'

curl http://127.0.0.1:18081/api/sessions/1/state
# {"payload": {"hello": "world"}}
```

### 看审计链

```bash
curl http://127.0.0.1:18081/api/sessions/1/audit
# {"audit": [...], "valid": true, "root_hash": "..."}
```

---

## ⏭️ 下一步(0.2.0 计划)

- **上下文架构** — 会话记忆 + 长期记忆(4-6 周)
- **L9 Kani 真实证明** — 5 stub → 真证明(2 周)
- **跨平台 CI** — Linux x86_64 + aarch64 + macOS(1 周)
- **依赖自动审计** — rustc 升级后(等上游)

详见 [`ROADMAP.md`](../ROADMAP.md) §"阶段 1:0.2.0 上下文架构"。

---

## 🙏 致谢

感谢所有给 EvoRule 提过建议、报过 bug、问过问题的早期用户。
虽然 alpha 阶段我们响应不及时,但每个 issue 都会看。

---

**作者**:EvoRule Project
**协议**:AGPL-3.0-or-later(代码) + CC0-1.0(`core_eval.json` 宪法)
**联系方式**:evorulelab@gmail.com
**Gitee**:https://gitee.com/evorulelab/evorule
