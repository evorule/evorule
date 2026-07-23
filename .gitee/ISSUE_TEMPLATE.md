<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule Issue 模板

> ⚠️ **alpha 阶段**:我们看 issue,但**不保证响应时间**。
> Critical / Security 问题会优先,其他排队。
> 详细承诺见 [SECURITY.md](../SECURITY.md) §"Supported Versions"。

---

## 🐛 Bug Report / 缺陷报告

**请删除不相关的部分,保留必要的。**

### 1. 复现步骤

最小化复现步骤:

```bash
# evorule-server 启动命令
./target/release/evorule-server --addr 127.0.0.1:18081

# evorule-cli 命令(如果是 CLI 问题)
./evorule-cli run --payload-file ./test.json
```

最小化的 `test.json` / 规则文件(粘贴代码或附 gist):

```json
{
  "type": "set",
  "params": {
    "path": "counter",
    "value": 0
  }
}
```

### 2. 期望行为

应该发生什么。

### 3. 实际行为

实际发生什么(贴错误日志、HTTP 响应、stack trace)。

### 4. 环境信息

- **OS / Arch**: (e.g. `Windows 11 / x86_64`, `Ubuntu 22.04 / aarch64`)
- **EvoRule 版本**: (`./evorule-cli --version` / `cargo metadata --format-version 1 | jq '.packages[].version'`)
- **构建模式**: (`release` / `debug` / `musl`)
- **是否 musl static**: (是 / 否)

### 5. 重要程度

- [ ] Blocker(完全不能用)
- [ ] Critical(核心功能坏)
- [ ] Major(影响工作流)
- [ ] Minor(边缘情况)
- [ ] Cosmetic(纯样式 / 文档)

### 6. 已尝试的缓解措施

(可选)

---

## 💡 Feature Request / 功能建议

### 1. 一句话描述

你希望加什么?

### 2. 背景 / 动机

为什么需要?你打算用这个功能做什么?

### 3. 提议的 API / 行为

具体怎么用?贴代码 / 伪代码 / 命令:

```rust
// 或者
let result = runner.run_with_replay("...", |version| {
    // 可选:在某版本上重放
});
```

### 4. 替代方案

有别的方式实现吗?为什么这个更好?

### 5. 优先级建议

- [ ] P0 (1.0 之前必须)
- [ ] P1 (0.3 / 0.4 阶段)
- [ ] P2 (1.0+)
- [ ] P3 (有精力再说)

### 6. 愿意贡献吗?

- [ ] 我愿意提 PR
- [ ] 我只能提 issue,需要别人实现
- [ ] 我想讨论方案,再决定

---

## 📚 Documentation / 文档问题

(链接 / 描述 / 建议改什么)

---

## ❓ Question / 提问

(自由格式。请先看 [README](../README.md) / [ROADMAP](../ROADMAP.md) / [STATUS](../STATUS.md) / [docs/](../docs/),这些是单一真相源。)

---

## ⚠️ 不接受的 issue 类型

- **安全问题** → 走 [SECURITY.md](../SECURITY.md),**不要**在 issue 里公开
- **功能咨询** → 用 Discussion(Gitee 暂未开,请用邮件 evorulelab@gmail.com)
- **HR / 商业合作** → 邮件 evorulelab@gmail.com
- **无关推广** → 直接 close

---

**感谢你帮 EvoRule 变得更好 🙏**
