<!--
SPDX-License-Identifier: CC0-1.0
Security disclosure procedures are public knowledge; we release them under CC0 so everyone knows how to report vulnerabilities safely.
-->

# 安全漏洞报告政策

**最后更新**: 2026-07-20

## ⚠️ Supported Versions / 支持的版本

| 版本 | 支持状态 | 说明 |
|---|---|---|
| `v0.1.0-alpha.x` | ✅ Supported | 公开基座阶段 |
| `v0.1.0` (production) | ⏳ Pending | 0.2.0 后发,届时成为主支持线 |
| `v6.0.x` (内部旧版) | ❌ EOL | 已退役,无 Gitee 撤回成本 |
| `< v0.1.0-alpha.1` | ❌ Unsupported | 公开仓库之前的 commit 不维护 |

**alpha 阶段承诺**:

- Critical / High 漏洞:60 天内修
- Medium / Low 漏洞:推迟到 0.2.0
- 安全公告:修完后 30 天内公开披露(经协调)

**1.0.0 之后承诺**(届时更新本文档):

- Critical:7 天
- High:30 天
- Medium:90 天
- Low:next release

---

## 报告安全漏洞

如果您发现 EvoRule 项目中的安全漏洞,请通过以下方式负责任地披露:

### 📧 联系方式

- **邮箱**: <evorulelab@gmail.com>(主题加 `[SECURITY]`)
- **Gitee 私信**: 维护者(@evorulelab)
- **加密**: 当前未提供 PGP 公钥(如有需要可联系)

### 📋 报告内容

请在报告中包含:

1. 漏洞类型和描述
2. 复现步骤
3. 潜在影响评估
4. 建议的修复方案(如有)
5. 已尝试的缓解措施

### ⏱️ 响应时间承诺

- **确认收到**: 48 小时内
- **初步评估**: 5 个工作日内
- **修复计划**: 10 个工作日内
- **公开披露**: 修复后 30 天内(经协调)

### 🔒 保密承诺

在漏洞修复并公开披露之前,我们将:

- 严格保密您的报告
- 不与第三方分享相关信息
- 及时向您通报修复进展

### 🙏 致谢

对于负责任披露的安全研究者,我们将在修复后的发布公告中予以致谢(经您同意)。

### 🔐 EvoRule 特有的安全考虑

EvoRule 涉及一些需要特别注意的安全边界:

| 边界 | 风险 | 缓解 |
|---|---|---|
| `core_eval.json` 加载 | 恶意宪法可执行任意 transform | 仅加载受信任的宪法文件,build.rs 编译时门禁 |
| `query_db` SQL | SQL 注入 | 必须用 `?` 占位符绑定参数,`audit/verify` 全程记录 |
| 业务规则热重载 | 恶意 JSON 触发危险操作 | 监听目录限制 + 验证 + audit log |
| HTTP API 认证 | 未授权访问 | Bearer token + 时序攻击防护(`subtle` crate) |
| FFI 接口 | 内存安全 | Opaque pointer + "指针+长度" 字符串模式 |

### 📜 已知安全问题

当前**没有已知未修复的安全问题**。

历史上修复过的问题请见 [CHANGELOG.md](CHANGELOG.md)。

---

**重要提示**: 请勿在公共论坛、社交媒体或 issue tracker 中公开未修复的安全漏洞。

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>

---

**本政策遵循 evorule-core-backup 的发布原则。**
