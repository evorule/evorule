# evorule 圈 2 样例规则

> 给合规刚需用户(医院、律所、金融、政务等)**开箱即用**的 JSON 规则集。

## 目录

| 目录 | 适用对象 | 法规对应 |
|---|---|---|
| [`hospital/`](hospital/) | 医院信息科 / 病案室 / 合规官 | HIPAA / 等保 2.0 / 《个人信息保护法》 |
| [`law-firm/`](law-firm/) | 律所合规部 / 案件管理 | 律师执业规范 / 客户保密 / GDPR Art.30 |

每个目录都包含:
- `rules/*.json` —— 3 条核心合规规则(开箱即用)
- `payload.example.json` —— 一份真实场景的初始数据
- `README.md` —— **30 秒使用说明 + 监管对话脚本**

## 30 秒试用

```bash
# 选一个场景
cd hospital/  # 或 law-firm/

# 1) 验证规则
evorule validate ./rules/

# 2) 跑一遍
cp payload.example.json payload.json
evorule run ./rules/ --payload-file payload.json

# 3) 落盘为审计日志
evorule run ./rules/ --payload-file payload.json -o fact.log
```

输出示例(`fact.log`):

```json
{"step":1,"type":"io_required","io_type":"audit_log","params":{...}}
{"step":2,"type":"io_required","io_type":"check_doctor_credentials","params":{...}}
{"step":3,"type":"state_transition","new_payload":{...}}
{"step":4,"type":"io_required","io_type":"log_redaction","params":{...}}
{"total_steps":4,"type":"final","final_payload":{...}}
```

> `io_required` 是合规规则的"审计请求"——evorule **自己不动外网**,
> 留给生产环境的 I/O handler 落地到真实审计库。

## 怎么改造成你公司的规则

1. **复制场景目录**:`cp -r hospital/ my-firm-rules/`
2. **改字段名**:把 `rules/*.json` 里的字段(`user_id` / `patient_id` 等)换成你的字段
3. **加规则**:复制 `01-access-audit.json`,改 `event_type` 和字段
4. **接到生产**:把 `io_request` 的 `io_type` 接到你公司的审计 / 数据库 / 监控系统

## 给监管的演示套路(30 秒讲完)

> "我们把合规规则写成 JSON,放到本地,evorule 单文件帮我们跑 + 留痕 + 重放。
> 这份是 fact.log —— 它就是审计证据本身,纯文本、可 grep、可 diff。
> 我们的 release artifact 是**可重现构建**的,您可以独立验证它没被篡改。
> evorule 不连任何外网,不调任何 LLM,完全本地化。"

## 反馈

如果某个规则有 bug 或想加新的合规场景,在 [Gitee issue](https://gitee.com/evorulelab/evorule/issues) 提。
