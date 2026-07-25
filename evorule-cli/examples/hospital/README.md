<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 医院合规规则样例(HIPAA / 等保 2.0)

> 适用对象:医院信息科 / 病案室 / 合规官
> 法规对应:HIPAA Privacy Rule / 等保 2.0 三级 / 《个人信息保护法》

## 这套样例做什么

演示 evorule 怎么把医院的 3 类核心合规规则写成 JSON:

| 规则文件 | 用途 | 监管要求 |
|---|---|---|
| `01-access-audit.json` | 每次访问患者数据 → 留痕 | HIPAA §164.312(b) 审计控制 |
| `02-prescription-guard.json` | 处方前检查医生是否有权开这种药 | 《处方管理办法》第 8 条 |
| `03-privacy-redaction.json` | 返回前脱敏敏感字段 | 《个人信息保护法》第 28 条 |

## 30 秒使用

```bash
# 1) 验证(0 错误即合规结构 OK)
evorule validate ./rules/

# 2) 用真实数据跑一次
cp payload.example.json payload.json
evorule run ./rules/ --payload-file payload.json -o fact.log

# 3) 给监管看 fact.log(每步留痕,可 grep / diff)
cat fact.log
```

## 输出示例

```json
{"step":1,"type":"io_required","io_type":"audit_log","params":{...}}
{"step":2,"type":"io_required","io_type":"check_doctor_credentials","params":{...}}
{"step":3,"type":"state_transition","new_payload":{"redaction_applied":true,...}}
{"step":4,"type":"io_required","io_type":"log_redaction","params":{...}}
{"total_steps":4,"type":"final","final_payload":{...}}
```

> **注意**:`io_required` 是合规规则"请求外部审计系统"——这正是 evorule 的"零网络"保证:
> 规则本身不连任何外网,**等生产环境的 I/O handler 来落地审计日志**。

## 跟 Excel 宏的对比

| 维度 | Excel 宏 / VBA | evorule |
|---|---|---|
| 审计留痕 | 没有 / 自己拼 | **自动**,JSON Lines,grep / diff |
| 规则变更可追溯 | git diff 不友好 | **纯 JSON**,可 diff / code review |
| 跨系统复用 | 每个表一份 | 一份 JSON,多系统共用 |
| 监管汇报 | 手工截图 | **fact.log 本身就是证据** |
| 防篡改 | 改了就改了 | BLAKE3 哈希链(生产配 I/O handler 启用) |

## 落地到生产需要什么

1. **规则上线**:把 `rules/*.json` 复制到 `/etc/hospital-rules/`
2. **I/O handler 接入**:把 `audit_log` / `check_doctor_credentials` / `log_redaction` 接到医院 HIS / 审计库 / 卫健委监管平台
3. **定时跑**:`crontab -e` 加 `0 * * * * cd /etc/hospital-rules && evorule run . -o /var/log/evorule/$(date +\%Y\%m\%d).log`
4. **监管检查日**:`evorule replay /var/log/evorule/2026-07-20.log` 重放当天所有合规事件

## 定制提示

- **改字段名**:直接编辑 `01-access-audit.json` 里的 `"user_id"` → 你的字段名
- **加规则**:复制 `01-access-audit.json`,改 `event_type` 和字段
- **强校验**:在 `branch.domain` 里加更严的判断(目前是宽松的"存在 doctor_id 就走 on_true")
- **定期审计**:复制 `payload.example.json` 为 `payload.weekly.json`,周一跑一次汇总
