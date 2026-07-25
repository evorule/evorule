<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 律所合规规则样例(律师执业规范 / 客户保密)

> 适用对象:律所合规部 / 案件管理 / 监管对接人
> 法规对应:《律师执业行为规范》/ 客户保密协议 / GDPR Art.30(国际客户)

## 这套样例做什么

演示 evorule 怎么把律所的 3 类核心合规规则写成 JSON:

| 规则文件 | 用途 | 监管对应 |
|---|---|---|
| `01-file-access-audit.json` | 每次访问客户卷宗 → 留痕 | 客户保密协议 / GDPR Art.30 |
| `02-conflict-of-interest.json` | 接案前检查利益冲突 | 律师执业行为规范 第 11 条 |
| `03-deadline-tracker.json` | 案件时效期限追踪 + 提醒 | 民事诉讼法 / 仲裁法 |

## 30 秒使用

```bash
# 1) 验证(0 错误即合规结构 OK)
evorule validate ./rules/

# 2) 用真实数据跑一次
cp payload.example.json payload.json
evorule run ./rules/ --payload-file payload.json -o fact.log

# 3) 给客户/监管看 fact.log
cat fact.log
```

## 输出示例(利益冲突检查)

```json
{"step":1,"type":"io_required","io_type":"audit_log","params":{"event_type":"case_file_access",...}}
{"step":2,"type":"io_required","io_type":"log_conflict_check","params":{"result":"no_potential_conflict",...}}
{"step":3,"type":"state_transition","new_payload":{"deadline_check":{...}}}
{"step":4,"type":"io_required","io_type":"log_deadline_status","params":{...}}
{"total_steps":4,"type":"final","final_payload":{...}}
```

> **3 个 io_required 是审计请求**:访问留痕 / 冲突检查 / 时效记录。
> 生产环境把这 3 个 io_type 接到律所案件管理系统 + 监管报送通道即可。

## 跟传统"案件管理系统"的差异

| 维度 | 传统 CMS(Java/SaaS)| evorule |
|---|---|---|
| 部署 | 需要服务器 / 公网 | **单文件,本地运行** |
| 客户数据位置 | 服务商数据库 | **本地 / 内网,零外联** |
| 规则变更 | 软件升级 / 厂商改 | **改 JSON,git diff** |
| 审计证据 | 厂商出报告 | **fact.log 即可,**第三方独立复现** |
| 客户审计 | 现场接待 + 演示 | **直接给 fact.log + 重建命令** |
| 律师自查 | 开 CMS,跑查询 | **`evorule run .` 一行** |

## 落地到生产需要什么

1. **规则上线**:把 `rules/*.json` 复制到 `/etc/firm-rules/`
2. **I/O handler 接入**:
   - `audit_log` → 接律所案件管理系统的审计库
   - `check_conflict_of_interest` → 接律所案件库的反查接口
   - `log_deadline_status` → 接律师日历 / 飞书 / 钉钉提醒
3. **每日检查**:`crontab -e` 加 `0 9 * * * cd /etc/firm-rules && evorule run . -o /var/log/evorule/$(date +\%Y\%m\%d).log`
4. **年度自查**:把全年的 `fact.log` 打包,作为律协/司法局检查的证据材料

## 客户最常问的 3 个问题(及答复)

**Q1:"客户数据怎么保证不外泄?"**
A:evorule 单文件,本地运行,**没有网络调用**(源码可审计)。规则跑完直接出 fact.log,**不写云、不发邮件**。

**Q2:"审计员怎么验证 fact.log 是真的?"**
A:fact.log 是 JSON Lines,纯文本,可读可 diff。**同源码两次构建 SHA256 一致**(`build-musl.sh --repro`),客户可独立复现二进制本身。

**Q3:"我们用 Excel 已经做了 5 年,为什么要换?"**
A:Excel 的"合规"靠人记,evorule 的"合规"靠 JSON + fact log,监管/客户/律协都可以**独立验证**,不需要信任某个员工/某台服务器。
