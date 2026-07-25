<!--
SPDX-License-Identifier: CC0-1.0
Benchmark reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.
-->

# 实验 1.1:evorule-server 端点冒烟测试(35/35)

**日期**:2026-07-20
**目标**:验证 evorule-server release 模式在 Windows 平台上的 35 个 HTTP 端点全部可访问
**构建**:`D:\evorule\.build\rust\release\evorule-server.exe` (10.7 MB,Windows release)
**数据库**:SQLite,`.build/exp1/evorule.db`
**客户端**:`curl.exe` (Windows 10+ bundled,避免 PowerShell 5.1 自动解压 gzip)

---

## 1. 结果汇总

| 指标 | 数值 |
|---|---|
| **端点总数** | 35 |
| **通过** | 35 ✅ |
| **失败** | 0 |
| **总耗时** | 3.05s |
| **平均延迟** | 87ms/端点 |
| **GZIP magic 验证** | ✅ 31 139 8 |
| **平台** | Windows 10,127.0.0.1:18081 |

---

## 2. 端点覆盖清单(35 个)

### 2.1 健康检查(3/3)

| 端点 | 状态 | 备注 |
|---|---|---|
| `GET /api/health` | 200 | `{"success":true,"message":"ok"}` |
| `GET /api/health/liveness` | 200 | `{"success":true,"message":"alive"}` |
| `GET /api/health/readiness` | 200 | `{"success":true,"message":"ready"}` |

### 2.2 会话(2/2)

| 端点 | 状态 | 备注 |
|---|---|---|
| `POST /api/sessions` | 200 | `{"session_id": N}` |
| `GET /api/sessions` | 200 | 列表 |

### 2.3 命令(1/1)

| 端点 | 状态 | 备注 |
|---|---|---|
| `POST /api/sessions/{id}/command` | 200 | set 指令正常 |

### 2.4 State / Payload(3/3)

| 端点 | 状态 | 备注 |
|---|---|---|
| `GET /api/sessions/{id}/state` | 200 | |
| `GET /api/sessions/{id}/payload` | 405 | **POST-only**(预期行为) |
| `POST /api/sessions/{id}/payload` | 200 | PayloadUpdate |

### 2.5 Facts(2/2)

| 端点 | 状态 | 备注 |
|---|---|---|
| `GET /api/sessions/{id}/facts` | 200 | |
| `GET /api/sessions/{id}/facts?prefix=test` | 200 | |

### 2.6 History(1/1)

| 端点 | 状态 |
|---|---|
| `GET /api/sessions/{id}/history` | 200 |

### 2.7 Audit 链(6/6)

| 端点 | 状态 | 备注 |
|---|---|---|
| `GET /api/sessions/{id}/audit` | 200 | 6 entries |
| `GET /api/sessions/{id}/audit/verify` | 200 | valid: true |
| `GET /api/sessions/{id}/audit/causal/{fact_id}` | 200 | 因果链 |
| `GET /api/sessions/{id}/audit/export` | 200 | 1478 bytes JSON |
| `POST /api/sessions/{id}/audit/import` | 200 | 验证后 verify_ok: true |
| `GET /api/sessions/{id}/audit/export/compressed` | 200 | 567 bytes gzip |
| `POST /api/sessions/{id}/audit/import/compressed` | 200 | 验证后 verify_ok: true |

### 2.8 Time Machine(4/4)

| 端点 | 状态 |
|---|---|
| `GET /api/sessions/{id}/replay` | 200 |
| `GET /api/sessions/{id}/rewind/0` | 200 |
| `GET /api/sessions/{id}/diff?a=0&b=1` | 200 |
| `POST /api/sessions/fork/{id}?version=0` | 200 |

### 2.9 Debug(3/3)

| 端点 | 状态 |
|---|---|
| `GET /api/sessions/{id}/debug/phase` | 200 |
| `GET /api/sessions/{id}/debug/queue` | 200 |
| `GET /api/sessions/{id}/debug/pending_io` | 200 |

### 2.10 Shared Facts(4/4)

| 端点 | 状态 | 备注 |
|---|---|---|
| `GET /api/shared/facts` | 200 | |
| `GET /api/shared/facts?prefix=shared.` | 200 | |
| `GET /api/shared/facts/1/source` | 404 | fact_id 1 不存在(预期) |
| `GET /api/shared/facts/1/used_by` | 200 |  |

### 2.11 Used at Startup(2/2)

| 端点 | 状态 |
|---|---|
| `GET /api/sessions/{id}/used_at_startup` | 200 |
| `POST /api/sessions/{id}/used_at_startup` | 200 |

### 2.12 SSE Events(1/1)

| 端点 | 状态 | 备注 |
|---|---|---|
| `GET /api/sessions/{id}/events` | 200 | text/event-stream,2s 内连接保持 |

### 2.13 Metrics & Debugger UI(2/2)

| 端点 | 状态 | 备注 |
|---|---|---|
| `GET /metrics` | 200 | Prometheus 格式 |
| `GET /debugger/debugger.html` | 200 | v0 sketch,~18.7KB |

---

## 3. 关键技术发现

### 3.1 PowerShell JSON 字符串陷阱(老问题,新场景)

`curl.exe -d '{"key":"value"}'` 在 PowerShell 5.1 下会被吃成 `{"key":"value"}`(双引号被剥掉),server 报 400。

**修法**:用 `--data-binary @file.json`,JSON 写到文件。
**文件保存**:必须用 `[System.IO.File]::WriteAllText(..., [System.Text.UTF8Encoding]::new($false))` 避免 BOM。

这个 bug 同样影响 `Invoke-WebRequest -Body '{"..."}'`,必须用 `-BodyFile` 或 `.NET HttpClient`。

### 3.2 PowerShell `${var}?query` 变量边界陷阱

`/api/sessions/fork/$sessId?version=0` 在 PowerShell 里被解析为 `?version=0` 接在变量名后,变成 `$sessId?version` 这个不存在的变量 + `=0`。

**修法**:用 `${sessId}?version=0`(花括号包住变量名)。

### 3.3 GZIP 端点的客户端陷阱

服务端正确返回 `Content-Type: application/gzip` + `Content-Disposition: attachment`,但:

- `Invoke-WebRequest` 会自动解压,gzip magic 丢失
- `curl.exe -o file` 保存原始字节,正确

**已验证**:`curl.exe -o audit.gz .../audit/export/compressed` 写出文件首 3 字节 = `31 139 8`,通过 gzip magic 检查。

### 3.4 fork 端点需要 query 参数

`POST /api/sessions/fork/{parent_id}` 不带 `?version=N` 返回 400(Invalid version)。

---

## 4. 与 28 号评估的对比

| 维度 | 28 号 | 1.1 |
|---|---|---|
| 端点覆盖 | ~10 个核心 | **35 个**(全量) |
| 测试方法 | 手动 curl | 自动化 PowerShell 脚本 |
| 结果可复现 | 不可 | **可**(脚本保留) |
| JSON 转义 | 手动 | **脚本处理**(file-based) |
| GZIP 验证 | 列出 magic 数字 | **脚本读 3 字节断言** |
| 失败用例 | 未明确 | **明确 35/35 pass** |

---

## 5. 已知限制

| 限制 | 影响 | 缓解 |
|---|---|---|
| **仅 Windows + localhost** | 没测过 Linux/macOS、跨网络 | 1.7 跨平台冷启动会补 |
| **musl build 被 openssl C 依赖卡住** | 没法生成 linux musl static binary | 1.7 跨平台时用 `vendored-openssl` 或迁 `rustls` |
| **Sequential,非并发** | 真实并发行为没测 | 1.5 并发测试会补 |
| **未测 server 崩溃/重启** | WAL 恢复路径没验 | 1.8 鲁棒性会补 |

---

## 6. 交付物

- [x] `docs/benchmarks/exp_1.1_smoke.ps1` — 自动化 smoke test 脚本(下次跑直接复现)
- [x] `docs/benchmarks/exp_1.1_results.csv` — 35 个端点结果
- [x] `docs/benchmarks/exp_1.1_output.txt` — 本次运行的输出日志
- [x] `docs/benchmarks/EVAL_2026-07-20.md` — 28 号评估的脱敏公开版

---

## 7. 复现方法

```powershell
# 1. 启动 server
cd D:\evorule
.\.build\rust\release\evorule-server.exe --addr 127.0.0.1:18081 `
  --db-path .\.build\exp1\evorule.db `
  --memory-dir .\.build\exp1\memory `
  --log-level warn --log-file .\.build\exp1\server.log

# 2. 跑 smoke test
.\docs\benchmarks\exp_1.1_smoke.ps1
```

预期输出:

- Pass: 35 / Fail: 0 / Total: 35
- Elapsed: ~3s
- GZIP magic OK

---

**最后更新**:2026-07-20
**下次实验**:1.2 sessions/sec 性能基准
