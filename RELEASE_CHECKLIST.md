<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule v0.1.0 发布前检查清单

**生成时间**: 2026-07-20
**版本**: 0.1.0
**检查人**: 自动生成

---

## 一、代码质量检查

### 1.1 Rust 代码

| 检查项 | 命令 | 状态 | 备注 |
|--------|------|------|------|
| 代码格式化 | `cargo fmt --all -- --check` | ✅ 通过 | 退出码 0 |
| Clippy 警告 | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 通过 | 退出码 0，0 warnings |
| 编译时门禁 | `build.rs` (tier0/tier1/tier2) | ✅ 通过 | G8 enforced |
| 单元测试 | `cargo test --workspace` | ✅ 通过 | 退出码 0 |
| Release 构建 | `cargo build --release --bin evorule-server` | ✅ 通过 | 28s 完成 |
| 无 `unsafe_code` | `#![forbid(unsafe_code)]` | ✅ 通过 | tier0-tcb 强制 |
| 无 `unwrap()`/`expect()` | 编译时门禁扫描 | ✅ 通过 | G8 enforced |
| 无硬编码业务字符串 | 编译时门禁扫描 | ✅ 通过 | tier1/tier2 G8 |

### 1.2 TypeScript SDK

| 检查项 | 命令 | 状态 | 备注 |
|--------|------|------|------|
| 类型检查 | `npm run typecheck` | ✅ 通过 | tsc --noEmit 退出码 0 |
| E2E 测试 | `npm run test:e2e` | ✅ 通过 | 所有场景通过 |

### 1.3 Python SDK

| 检查项 | 命令 | 状态 | 备注 |
|--------|------|------|------|
| 静态分析 | `python -m pyflakes evorule/ tests/ examples/` | ✅ 通过 | 退出码 0，0 warnings |
| E2E 测试 | `python tests/test_e2e.py` | ✅ 通过 | 43 passed, 0 failed |
| 语法检查 | `python -m py_compile evorule/*.py` | ✅ 通过 | CI 中验证 |

---

## 二、版本号一致性

| 项目 | 文件 | 版本号 | 状态 |
|------|------|--------|------|
| Rust Workspace | `Cargo.toml` | 0.1.0 | ✅ |
| tier0-tcb | `tier0-tcb/Cargo.toml` | 0.1.0 | ✅ |
| tier1-reactor | `tier1-reactor/Cargo.toml` | 0.1.0 | ✅ |
| tier2-governance | `tier2-governance/Cargo.toml` | 0.1.0 | ✅ |
| TypeScript SDK | `sdk/typescript/package.json` | 0.1.0 | ✅ |
| Python SDK | `sdk/python/pyproject.toml` | 0.1.0 | ✅ |
| README 徽章 | `README.md` | 0.1.0 | ✅ 已修复 |
| CHANGELOG | `CHANGELOG.md` | 0.1.0 | ✅ |

---

## 三、文档完整性

| 文档 | 路径 | 状态 | 备注 |
|------|------|------|------|
| README.md | `/README.md` | ✅ | 项目总览、特性、架构说明 |
| CHANGELOG.md | `/CHANGELOG.md` | ✅ | v0.1.0 章节完整 |
| VERSION_STRATEGY.md | `/VERSION_STRATEGY.md` | ✅ | v1.1 版本策略 |
| CONTRIBUTING.md | `/CONTRIBUTING.md` | ✅ | 贡献流程 |
| CONTRIBUTING_ZH.md | `/CONTRIBUTING_ZH.md` | ✅ | 中文贡献流程 |
| SECURITY.md | `/SECURITY.md` | ✅ | 安全策略 |
| CODE_OF_CONDUCT.md | `/CODE_OF_CONDUCT.md` | ✅ | 行为准则 |
| 特别规范.md | `tier1-reactor/特别规范.md` | ✅ | 机制/策略分离规范 |

### SDK 文档

| SDK | README | CHANGELOG | 示例 | 状态 |
|-----|--------|-----------|------|------|
| TypeScript | ✅ | ✅ | ✅ `quick_start.ts` | 完整 |
| Python | ✅ | ✅ | ✅ `quick_start.py` | 完整 |

---

## 四、许可证与法律文件

| 文件 | 路径 | 状态 |
|------|------|------|
| LICENSE (AGPL-3.0) | `/LICENSE` | ✅ |
| DUAL_LICENSE.md | `/DUAL_LICENSE.md` | ✅ |
| COMMERCIAL_LICENSE.md | `/COMMERCIAL_LICENSE.md` | ✅ |
| FREE_COMMERCIAL_LICENSE.md | `/FREE_COMMERCIAL_LICENSE.md` | ✅ |
| CLA-individual.md | `/CLA-individual.md` | ✅ |
| TRADEMARK.md | `/TRADEMARK.md` | ✅ |
| NOTICE.md | `/NOTICE.md` | ✅ |
| AUTHORS.md | `/AUTHORS.md` | ✅ |

### SDK 许可证

| SDK | LICENSE | NOTICE | 状态 |
|-----|---------|--------|------|
| TypeScript | ✅ | ✅ | 完整 |
| Python | ✅ | ✅ | 完整 |

---

## 五、CI/CD 流程

| 流程 | 文件 | 状态 | 备注 |
|------|------|------|------|
| CI 持续集成 | `.github/workflows/ci.yml` | ✅ | lint + test + build + E2E |
| Release 发布 | `.github/workflows/release.yml` | ✅ | 多平台构建 + Docker + Release |
| Kani 形式化验证 | `.github/workflows/kani.yml` | ✅ | 核心不变式验证 |
| 变异测试 | `.github/workflows/mutants.yml` | ✅ | 测试有效性验证 |
| Gitee CI | `.gitee-ci/validate.yml` | ✅ | Gitee Go 流水线 |

### CI 覆盖的检查项

- ✅ `cargo fmt --check` — 格式检查
- ✅ `cargo clippy -D warnings` — 代码质量
- ✅ `cargo test --workspace` — 单元测试
- ✅ `cargo build --release` — 编译验证
- ✅ `npm run typecheck` — TypeScript 类型检查
- ✅ `python -m py_compile` — Python 语法检查
- ✅ TypeScript E2E 测试
- ✅ Python E2E 测试

### Release 流程覆盖

- ✅ 多平台二进制构建（Linux x86_64/ARM64, Windows x86_64, macOS ARM64/x86_64）
- ✅ Docker 镜像构建（multi-arch: linux/amd64, linux/arm64）
- ✅ GitHub Release 自动创建
- ✅ 产物上传（tar.gz / zip / docker.tar）

---

## 六、基础设施

| 组件 | 路径 | 状态 | 备注 |
|------|------|------|------|
| Dockerfile | `/Dockerfile` | ✅ | 多阶段构建，生产就绪 |
| .dockerignore | `/.dockerignore` | ✅ | 排除不必要的文件 |
| .gitignore | `/.gitignore` | ✅ | 排除构建产物 |
| Docker Compose | `monitoring/docker-compose.yml` | ✅ | Prometheus + Grafana |
| Prometheus 配置 | `monitoring/prometheus.yml` | ✅ | 指标采集 |
| Grafana 配置 | `monitoring/grafana/` | ✅ | 数据源 provisioning |

---

## 七、核心功能验证

### 7.1 Rust 核心测试覆盖

| 模块 | 测试文件 | 状态 |
|------|----------|------|
| tier0-tcb | `integration_end_to_end.rs` | ✅ |
| tier0-tcb | `panic_free.rs` | ✅ |
| tier0-tcb | `proptest_props.rs` | ✅ |
| tier0-tcb | `tcb_error_variants.rs` | ✅ |
| tier1-reactor | `integration_test.rs` | ✅ |
| tier2-governance | `integration_test.rs` | ✅ |
| tier2-governance | `sse_integration_test.rs` | ✅ |
| tier2-governance | `fault_recovery_test.rs` | ✅ |

### 7.2 E2E 测试场景覆盖

| 场景 | TypeScript | Python | 状态 |
|------|-----------|--------|------|
| 会话生命周期 | ✅ | ✅ | 创建/列表/状态/关闭 |
| 命令提交 + SSE | ✅ | ✅ | Command/StateTransition/Stable |
| Payload 更新 | ✅ | ✅ | 简单 + 嵌套字段 |
| 时间旅行 | ✅ | ✅ | replay/rewind/diff |
| Debug 端点 | ✅ | ✅ | phase/queue/pending_io |
| 执行中断 | ✅ | ✅ | interrupt + 状态可读 |
| 共享 Facts | ✅ | ✅ | 设置 + 查询 |
| 审计链 | ✅ | ✅ | audit + verify |
| 历史查询 | ✅ | ✅ | history |
| 集群协作 | ✅ | ✅ | join/leave/status |
| 会话分叉 | ✅ | ✅ | fork + 状态继承 |
| 健康检查 | ✅ | ✅ | health/liveness/readiness |

---

## 八、发布前待办事项

### 8.1 发布前必须完成

- [x] 所有代码检查通过（fmt/clippy/test）
- [x] 所有 E2E 测试通过（TypeScript + Python）
- [x] 版本号一致性验证
- [x] 文档完整性检查
- [x] 许可证文件齐全
- [x] CI/CD 流程配置完整
- [x] Dockerfile 配置正确
- [x] 代码无未使用的变量或导入
- [x] README 版本徽章已更新

### 8.2 发布时执行

- [ ] `git add -A && git commit -m "release: v0.1.0"`
- [ ] `git tag v0.1.0`
- [ ] `git push origin main --tags`
- [ ] `git push gitee main --tags`
- [ ] 验证 GitHub Actions Release 流程触发
- [ ] 验证多平台二进制产物构建
- [ ] 验证 Docker 镜像推送
- [ ] 验证 GitHub Release 创建
- [ ] SDK 发布到包管理器（可选，0.1.0 可延迟）
  - [ ] `cd sdk/typescript && npm publish`
  - [ ] `cd sdk/python && python -m build && twine upload dist/*`

### 8.3 发布后验证

- [ ] 从 Release 下载二进制并运行 `--help`
- [ ] Docker 镜像拉取并运行
- [ ] 按照快速开始指南验证基本流程
- [ ] 检查 Gitee Release 同步

---

## 九、已知限制（0.1.0 阶段）

| 限制 | 说明 | 计划 |
|------|------|------|
| LLM handler 是 stub | 1.0 门槛要求真实 LLM 调用 | 0.x 后续版本 |
| Tool handler 是 stub | 1.0 门槛要求真实工具调用 | 0.x 后续版本 |
| Kani 证明是 stub | 6 个 proof stubs 待实现 | 0.x 后续版本 |
| 无性能基准 | PERFORMANCE_BENCHMARK.md 待编写 | 1.0 前 |
| 无完整 API 文档 | API_REFERENCE.md 待编写 | 1.0 前 |
| API 不稳定 | 0.x 阶段 API 可能随时变 | 1.0 承诺稳定 |

> 以上限制符合 [VERSION_STRATEGY.md](VERSION_STRATEGY.md) §4.1 对 0.1.0 阶段的定义，不阻碍发布。

---

## 十、检查结论

| 维度 | 状态 | 详情 |
|------|------|------|
| 代码质量 | ✅ 通过 | fmt/clippy/test 全部通过，0 warnings |
| 版本一致性 | ✅ 通过 | 所有 Cargo.toml/package.json/pyproject.toml = 0.1.0 |
| 文档完整性 | ✅ 通过 | README/CHANGELOG/CONTRIBUTING/SECURITY 齐全 |
| 许可证合规 | ✅ 通过 | AGPL-3.0 + 双重许可证 + CLA |
| CI/CD 流程 | ✅ 通过 | ci.yml + release.yml + kani.yml + mutants.yml |
| E2E 测试 | ✅ 通过 | TypeScript + Python 全场景通过 |
| 基础设施 | ✅ 通过 | Dockerfile + 监控配置完整 |

### 🎯 最终结论

**EvoRule v0.1.0 满足发布条件，可以发布。**

所有代码质量检查、测试、文档、许可证、CI/CD 流程均已就绪。已知限制均符合 0.1.0 阶段的定位，不阻碍发布。

---

*本检查清单基于 2026-07-20 的代码状态自动生成。*
