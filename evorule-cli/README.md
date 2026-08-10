<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# `evorule` CLI

[![版本 v0.2.2](https://img.shields.io/badge/version-v0.2.2-blue)](../Cargo.toml)
[![AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0--or--later-green)](../LICENSE)
[![文档索引 DOCS_INDEX](https://img.shields.io/badge/docs-DOCS_INDEX-8A2BE2)](../DOCS_INDEX.md)

> **架构层次**:EvoRule 三层架构之上的 **L1 命令行封装工具**(面向圈 2 合规用户,不引入新机制,只封装 evorule-tcb + evorule-reactor 已有的能力)。

> **业务规则模板位置说明（边界合规）**:
> 面向行业的开箱即用规则集（**医院 HIPAA / 律所利益冲突 / 金融 AML / 政务数据分级** 等）不在本仓——本仓仅保留机制层最小化演示用例（见本仓 `evorule-cli/examples/README.md` 或 `evorule-cli/tests/fixtures/`）。行业规则模板请参见对应独立仓。

**本地 JSON 规则执行工具,面向"圈 2 合规刚需"用户**(医疗/律所/金融/政务等隐私敏感行业)。

> **"evorule 没有智能,只有执行的最佳实践"**
> **"把你公司的合规规则写成一个 JSON 文件,放到本地,evorule 帮你跑 + 审计 + 重放 + 验真"**

**G8 门控遵守**:编译期 `build.rs` 递归扫描 `src/**/*.rs`(非测试代码)拦截「硬编码业务控制流」违规;所有业务语义均由规则目录的 JSON 数据驱动,CLI 本身只做命令分发 + 文件读写 + fact log 格式转换。

## 特性

- ✅ **零网络** —— 不调用任何外部服务
- ✅ **零遥测** —— 无任何隐式上报
- ✅ **零 AI 决策** —— 不调用 LLM
- ✅ **零系统依赖** —— musl 静态链接,1.8 MB 单文件
- ✅ **完整审计** —— 每步 fact 落盘(tier1 WAL JSONL 格式,与 evorule-governance 互通)
- ✅ **哈希链** —— blake3 哈希链 + 结构不变量校验(防篡改)
- ✅ **FIFO 队列** —— 修复原 LIFO bug,正确执行 push 语义
- ✅ **确定性加载** —— 规则文件按文件名排序,跨平台一致
- ✅ **可对比** —— 两个 fact log 按 FactId 对齐 diff(非 HashSet)
- ✅ **可重放** —— `replay` 子命令播放 fact log
- ✅ **可验真** —— `verify-chain` 子命令验证 fact log 完整性
- ✅ **可重现构建** —— 同源码两次构建 SHA256 一致(圈 2 监管可独立复现)
- ✅ **G8 门控** —— 编译期拦截"硬编码控制流"违规(与 tier1/tier2 同套规则)
- ✅ **多架构** —— `x86_64-unknown-linux-musl` + `aarch64-unknown-linux-musl`(AWS Graviton / RPi 适用)

## 快速开始(圈 2 用户)

```bash
# 1) 下载(根据 CPU 架构选)
wget https://gitee.com/evo-rule-lab/evorule/releases/download/v0.2.2/evorule-x86_64
wget https://gitee.com/evo-rule-lab/evorule/releases/download/v0.2.2/evorule-aarch64

# 2) 验证(可选,确认下载完整)
sha256sum -c evorule-x86_64.sha256

# 3) 装上
chmod +x evorule
mv evorule /usr/local/bin/evorule

# 4) 校验你的合规规则
evorule validate /etc/company-rules/

# 5) 跑你的合规规则
evorule run /etc/company-rules/ -o /var/log/evorule-fact.log

# 6) 验真(确认 fact log 未被篡改)
evorule verify-chain /var/log/evorule-fact.log

# 7) 出事故?重放
evorule replay /var/log/evorule-fact.log

# 8) 监管检查?导出 fact log
cat /var/log/evorule-fact.log
```

## 编译(开发者)

### 前置

- **Linux 容器** / **WSL Ubuntu 22.04** / 真 Linux
- `rustup` 1.92+(推荐 rustup 自动管理)
- 对应架构的 musl 工具链:
  - x86_64: `apt install musl-tools && rustup target add x86_64-unknown-linux-musl`
  - aarch64: `apt install musl-tools gcc-aarch64-linux-gnu && rustup target add aarch64-unknown-linux-musl`

### 编译(默认 x86_64)

```bash
cd evorule-cli  # 在 evorule 仓根目录下执行；Linux 用户进入对应目录即可
bash build-musl.sh
# 产物: $TARGET_DIR/x86_64-unknown-linux-musl/release/evorule
#   1.8 MB,静态链接,stripped
```

### 编译 aarch64(AWS Graviton / RPi)

```bash
bash build-musl.sh --target aarch64-unknown-linux-musl
# 产物: $TARGET_DIR/aarch64-unknown-linux-musl/release/evorule
#   1.4 MB,静态链接,stripped
```

### 可重现构建(reproducible build)

```bash
bash build-musl.sh --target x86_64-unknown-linux-musl --repro
# 输出:
#   REPRODUCIBLE ✓ <SHA256>
#   Both builds produced identical SHA256
```

**原理**:

- `SOURCE_DATE_EPOCH=1700000000` 固定所有时间戳
- `CARGO_INCREMENTAL=0` 强制全量构建
- `RUSTFLAGS=-Wl,--build-id=none` 去掉 linker 随机 build-id
- `CARGO_PROFILE_RELEASE_STRIP=true` 自动去符号
- 两次 `cargo clean` + `cargo build` 产出**字节完全相同**的二进制

**圈 2 价值**:监管机构可以独立复现 release artifact,验证供应链可信。

### 验证二进制

```bash
$ file evorule
ELF 64-bit LSB pie executable, x86-64, ... static-pie linked, ...

$ ldd evorule
        statically linked
```

### Windows 开发构建

```cmd
cd D:\evorule
cargo build --release --bin evorule
REM 产物: .build\rust\release\evorule.exe
```

## 子命令

### 1. `evorule validate <rules-dir>`

校验 JSON 规则文件,验证 transform 类型是否在 core_eval 元指令白名单内。

```bash
evorule validate ./rules/
```

输出:

- `[OK]   transform[N]: type='branch'` —— 合法 type
- `[ERROR] transform[N]: unknown type 'X'` —— 未知 type(阻断)
- `[ERROR] transform[N]: missing 'type' field` —— 缺少 type 字段(阻断)

合法 type 白名单(core_eval 元指令):

| type         | 用途                                     |
| ------------ | ---------------------------------------- |
| `branch`     | 条件分支(domain 匹配 → on_true/on_false) |
| `set`        | 修改 payload 字段(set/add/sub)           |
| `push`       | 推指令到队列前端(插队语义)               |
| `io_request` | 产生 I/O 请求信号(不修改状态)            |
| `noop`       | 空操作                                   |
| `increment`  | 自增(业务指令,由 core_eval 映射)         |
| `decrement`  | 自减(业务指令,由 core_eval 映射)         |

退出码:

- 0 = 全部通过
- 1 = 有 error(未知 type 或缺 type 字段)
- 2 = 规则目录不存在或无 .json 文件

---

### 2. `evorule run <rules-dir> [--payload X | --payload-file X] [-o output] [--max-steps N]`

加载并执行 JSON 规则,输出 fact log(tier1 WAL JSONL 格式,每行一个 Fact)。

```bash
# 默认空 payload
evorule run ./rules/

# 提供初始 payload(JSON 字符串)
evorule run ./rules/ --payload '{"user_id": 42}'

# 从文件读 payload(Windows 友好,避免命令行转义问题)
evorule run ./rules/ --payload-file ./payload.json

# 输出到文件(而非 stdout)
evorule run ./rules/ -o ./fact.log

# 限制最大执行步数(防死循环,默认 10000)
evorule run ./rules/ --max-steps 100
```

输出示例(tier1 WAL JSONL 格式):

```json
{"id":1,"instruction":{"type":"noop"},"type":"Command"}
{"cause":1,"id":2,"new_payload":{"x":42},"new_queue":[],"type":"StateTransition"}
{"final_snapshot":{"x":42},"id":3,"type":"Stable"}
```

**Fact 类型**(7 种,与 evorule-reactor / evorule-governance 一致):

| type              | 字段                              | 说明                                                 |
| ----------------- | --------------------------------- | ---------------------------------------------------- |
| `Command`         | id, instruction                   | 用户提交新指令(触发执行)                             |
| `StateTransition` | id, cause, new_payload, new_queue | 状态转换(每步执行)                                   |
| `IoRequest`       | id, cause, io_type, params        | I/O 请求信号(0.2.0 无 handler → Error)               |
| `IoResponse`      | id, request_id, result, error     | I/O 响应(0.2.0 不产生)                               |
| `Stable`          | id, final_snapshot                | 稳定状态(最终快照,始终发射)                          |
| `Error`           | id, message                       | 执行错误(max_steps 超限 / TCB 错误 / 无 I/O handler) |
| `PayloadUpdate`   | id, path, value                   | 载荷更新(0.2.0 不产生)                               |

退出码:

- 0 = 执行完成(可能含 Error fact,但 CLI 本身不失败)
- 1 = 加载或执行错误(规则目录问题 / payload 解析失败)
- 2 = 规则目录不存在或无 .json 文件

---

### 3. `evorule replay <fact-log>`

播放 fact log(pretty-print,每个 Fact 一行摘要)。

```bash
evorule replay ./fact.log
```

输出示例:

```text
=== Replaying ./fact.log ===
[F1] Command type=noop
[F2] StateTransition cause=F1
[F3] Stable
=== End (3 facts) ===
```

---

### 4. `evorule diff <a.log> <b.log>`

对比两个 fact log(**按 FactId 数组下标对齐**,非 HashSet)。

```bash
evorule diff ./before.log ./after.log
```

输出示例(差异):

```text
=== Diff ./before.log <-> ./after.log ===
A: 3 facts
B: 3 facts

[~] [F2] StateTransition cause=F1
[~] [F2] StateTransition cause=F1
[~] [F3] Stable
[~] [F3] Stable

=== 2 difference(s) ===
```

diff 前缀:

- `[~]` —— 两边都有但内容不同
- `[-]` —— 只在 A
- `[+]` —— 只在 B
- `(identical)` —— 完全相同

---

### 5. `evorule verify-chain <fact-log>`

验证 fact log 完整性(**三层验证**):

1. **哈希链**:blake3 链式哈希(与 evorule-governance 字节级一致)
2. **FactId 单调递增**:每个 Fact 的 id 必须严格大于前一个
3. **cause 引用有效性**:`StateTransition.cause` 和 `IoRequest.cause` 必须指向已存在的 FactId

```bash
evorule verify-chain ./fact.log
```

输出示例(通过):

```text
=== Verifying hash chain: ./fact.log ===
Facts: 3
Algorithm: blake3 (compatible with evorule-governance auditor)

[OK] Hash chain computed
[OK] Structural invariants verified (FactId monotonic, cause references valid)
     genesis → F1 → F2 → ... → F3 (final)
```

输出示例(篡改检测):

```text
=== Verifying hash chain: ./tampered.log ===
Facts: 3
Algorithm: blake3 (compatible with evorule-governance auditor)

[OK] Hash chain computed
[ERROR] Structural invariant violations:
        fact[2]: id=3 not strictly greater than prev id=99 (monotonicity violated)
Error: Hash chain verification failed: structural violations: 1
```

退出码:

- 0 = 哈希链 + 结构不变量全部通过
- 1 = 任一检查失败(fact 被篡改或结构异常)

**与 evorule-governance 的关系**:`hash.rs` 复制自 `evorule-governance/src/hash.rs`,由 `include_str!` 交叉验证测试强制双边同步。CLI 与 tier2 Auditor 对同一份 fact.log 必产生相同的链哈希。

---

## G8 门控(编译期拦截)

evorule-cli 的 `build.rs` 强制执行 G8 + F11,**与 evorule-reactor / evorule-governance 同一套规则**:

| 规则    | 禁止                                                   | 目的                               |
| ------- | ------------------------------------------------------ | ---------------------------------- |
| **G8**  | `"conditional"` / `"while_loop"` / `"sequence"` 字面量 | 反应器/治理层/CLI 都不得展开控制流 |
| **F11** | `debug_assert!` / `.unwrap(` / `.expect(` / `panic!(`  | 主代码路径不 panic-prone           |

**扫描范围**:递归扫描 `src/**/*.rs`(含 `src/commands/*.rs` 等子模块),通过 `strip_test_mod` 剥离 `#[cfg(test)] mod tests { ... }` 测试模块体。

**零豁免原则**:G8/F11 对 `src/**/*.rs`(非测试)零容忍,无任何业务字面量豁免。

任何违规立即 `exit(1)`,违规行号 + 标签全打印。

**紧急跳过**(不推荐):

```bash
EVORULE_SKIP_GATE=1 cargo build
```

## 规则文件格式

JSON 规则文件遵循 `core_eval.json` 格式(transform 列表)。

**示例**:`rules/my-rule.json`:

```json
{
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "instruction",
          "instruction_type": "increment"
        },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "x",
              "operation": "add",
              "value": "__exec__.instruction.params.delta"
            }
          }
        ]
      }
    }
  ]
}
```

**`set` 指令的 `attr` 字段**:

- 字面字符串(如 `"x"`):直接作为 payload 字段名
- 点分路径(如 `"a.b.c"`):自动创建嵌套对象,设置 `payload.a.b.c`
- **不要**用 `"__exec__.payload.x"` —— 这会被当作路径引用解析,而非字段名

**多文件**:`run` 会按**文件名字典序排序**后加载目录下所有 `*.json` 文件的 `transform` 数组,保证跨平台确定性。

**支持的文件格式**:

- `{"transform": [...]}` —— 标准格式
- `{"transforms": [...]}` —— 别名(同义)
- `[{...}, {...}]` —— 顶层数组,每项是一条 transform
- `{...}` —— 单条 transform 对象

**I/O 类型**(mechanism-policy separation):

自 v0.2.0 起,`io_type` 支持任意字符串(`IoType::new("your_service")`),不再限于固定白名单;`IoType::parse` 已 `#[deprecated]`,新代码用 `IoType::new`。内置 5 个工厂函数(字符串值不变,向后兼容):

| io_type         | 内置工厂函数              | 用途               |
| --------------- | ------------------------- | ------------------ |
| `call_external` | `IoType::call_external()` | 调用外部服务       |
| `query_db`      | `IoType::query_db()`      | 查询数据库         |
| `http_get`      | `IoType::http_get()`      | HTTP GET 请求      |
| `save_memory`   | `IoType::save_memory()`   | 保存到记忆         |
| `call_service`  | `IoType::call_service()`  | 调用外部服务(通用) |

**业务语义**既可通过 `call_service` + `service_name` 表达(如 `"io_type": "call_service", "service_name": "audit_log"`),也可直接用自定义 io_type(如 `"io_type": "audit_log"`,由应用层 subscriber 按字符串路由)。机制层只认字符串、不校验语义,校验由 `ReactorBuilder::known_io_types` 或 subscriber 决定。

---

## 圈 2 合规刚需演示(30 秒话术)

> "把你公司的合规规则写成一个 JSON 文件,放到本地文件夹。
> 运行 `evorule run ./rules/`,它会:
>
> 1. 执行你的规则
> 2. 记录每一步到本地 fact log(tier1 WAL JSONL 格式,与 tier2 审计链互通)
> 3. **不会联网,不会上报,不会 AI 决策**
> 4. 出事故?`evorule replay fact.log` 重放
> 5. 监管检查?fact.log 本身就是审计证据
> 6. 验真?`evorule verify-chain fact.log` 验证哈希链 + 结构完整性
> 7. 供应链可信?同源码两次构建 SHA256 一致,监管可独立复现"

**对比 Excel 宏 / VBA**:

| 维度       | Excel 宏      | evorule                        |
| ---------- | ------------- | ------------------------------ |
| 规则格式   | 散在各个 cell | 集中 JSON,版本控制友好         |
| 执行可重复 | 不一定        | 100% 确定性 + 可重现二进制     |
| 审计追踪   | 没有          | 完整 fact log + blake3 哈希链  |
| 防篡改     | 没有          | 哈希链 + 结构不变量校验        |
| 跨文件规则 | 难            | 简单(目录扫描,确定性排序)      |
| 时间回放   | 撤销 5 步     | 任意时刻回放                   |
| 监管汇报   | 手工截图      | 自动导出 fact.log              |
| 架构选择   | 仅 x86        | x86_64 + aarch64(Graviton/RPi) |

---

## 圈 2 分发清单(给合规用户)

| 物料                                 | 来源                  | 大小   |
| ------------------------------------ | --------------------- | ------ |
| `evorule-x86_64-unknown-linux-musl`  | CI artifact / release | 1.8 MB |
| `evorule-aarch64-unknown-linux-musl` | CI artifact / release | 1.4 MB |
| `*.sha256`                           | 校验文件              | < 1 KB |
| AGPL-3.0 许可证文本                  | 随源码提供            | -      |
| 用户规则文件                         | 用户自备              | -      |

**目标交付命令**:

```bash
# Linux x86_64 圈 2 用户
wget https://gitee.com/evo-rule-lab/evorule/releases/download/v0.2.2/evorule-x86_64
chmod +x evorule-x86_64
./evorule-x86_64 validate /etc/company-rules/
./evorule-x86_64 run /etc/company-rules/ -o /var/log/evorule-fact.log
./evorule-x86_64 verify-chain /var/log/evorule-fact.log

# Linux aarch64 圈 2 用户 (AWS Graviton / RPi)
wget https://gitee.com/evo-rule-lab/evorule/releases/download/v0.2.2/evorule-aarch64
chmod +x evorule-aarch64
./evorule-aarch64 run /etc/company-rules/ -o /var/log/evorule-fact.log
```

---

## 端到端测试

`tests/e2e.sh` 是一个 TAP 格式的 e2e 测试,覆盖 5 个子命令的 28 个用例:

```bash
# 自动检测二进制
bash tests/e2e.sh

# 显式指定
bash tests/e2e.sh .build/rust/x86_64-unknown-linux-musl/release/evorule
bash tests/e2e.sh .build/rust/aarch64-unknown-linux-musl/release/evorule
```

测试覆盖:

```
ok 1   - --version exits 0
ok 2   - --help shows subcommands
ok 3   - validate valid rule exits 0
ok 4   - validate invalid rule exits 1
ok 5   - validate unknown-type errors
ok 6   - validate empty dir exits 2
ok 7   - validate nonexistent dir exits 2
ok 8   - run valid rule exits 0 with Stable fact
ok 9   - run with --payload exits 0
ok 10  - run with --payload-file exits 0
ok 11  - run -o writes JSONL with Command first line
ok 12  - run --max-steps 0 produces Error fact
ok 13  - replay exits 0 with Replaying header
ok 14  - replay nonexistent exits 1
ok 15  - diff identical logs
ok 16  - diff different logs shows differences
ok 17  - verify-chain valid log exits 0
ok 18  - verify-chain tampered log exits 1
ok 19  - all fact log lines are valid JSON with type+id
ok 20  - FIFO queue: step1 then step2 (order=second confirms FIFO)
ok 21  - deterministic loading: 3 files loaded in order, all fields set
ok 22  - verify-chain on echo log exits 0
ok 23  - validate hospital example rules
ok 24  - validate law-firm example rules
ok 25  - hospital example runs with payload (Stable)
ok 26  - law-firm example runs with payload (Stable)
ok 27  - hospital example produces IoRequest + Error (no handler in 0.2.0)
ok 28  - verify-chain on hospital log (complex facts) exits 0
```

**aarch64 验证**:用 `qemu-user-static` 模拟跑(不需要真 ARM 机器):

```bash
sudo apt install qemu-user-static
bash tests/e2e.sh .build/rust/aarch64-unknown-linux-musl/release/evorule
```

---

## 示例规则(给圈 2 用户开箱即用)

`examples/` 下放了两套真实场景的合规规则,每套都包含规则文件、示例 payload、README:

| 目录                                       | 适用对象            | 法规对应                              |
| ------------------------------------------ | ------------------- | ------------------------------------- |
| [`examples/hospital/`](examples/hospital/) | 医院信息科 / 病案室 | HIPAA / 等保 2.0 / 《个人信息保护法》 |
| [`examples/law-firm/`](examples/law-firm/) | 律所合规部          | 律师执业规范 / 客户保密 / GDPR Art.30 |

每套 3 条核心规则(覆盖 访问审计 / 权限检查 / 隐私脱敏),复制整个目录到 `/etc/your-rules/` 即可落地。

**30 秒试用**:

```bash
cd examples/hospital/
evorule validate ./rules/
evorule run ./rules/ --payload-file payload.example.json
```

详细说明 + 监管对话脚本见 `examples/README.md`。

---

## 安全与验真

### `verify-chain` 哈希链验证

`evorule verify-chain <fact.log>` 同时做 **三重校验**,任何一项不通过立即返回退出码 1:

| 校验层        | 算法/规则                                                  | 失败原因                        |
| ------------- | ---------------------------------------------------------- | ------------------------------- |
| 结构完整性    | FactId 单调递增 + `cause` 引用必须指向已出现的 FactId      | 篡改 FactId 顺序 / 插入虚假因果 |
| BLAKE3 哈希链 | `chain_hash = blake3(prev_hash + content_hash + id_bytes)` | 修改任何已落盘 Fact 的 content  |
| WAL 格式      | 每行必须是合法 JSON,且含 `type` + `id` 字段                | 文件损坏 / 手工编辑             |

哈希算法**单一真相源**在 `evorule-reactor/src/hash.rs`,cli 与 evorule-governance 均 re-use 同一实现,避免分叉。

### 供应链可信:可重现构建

同源码两次 `cargo build --release`(同一 Rust toolchain、同一 target)产物 SHA256 一致。圈 2 监管方可独立从 GitHub/Gitee 拉源码构建,与官方 artifact 做 SHA 对比,无需信任分发渠道。

---

## 已知限制(0.2.0)

- ❌ 无 I/O handler(`io_request` 会产生 IoRequest fact + Error fact,不实际执行 I/O)
- ❌ 无 HTTP API(本 crate 是本地 CLI，不提供 HTTP 服务；如需 HTTP/SSE 由应用层基于核心仓机制自行构建)
- ❌ 无配置文件(后续加 `.evorule.toml`)
- ❌ 无 hot-reload(后续加)
- ✅ 0 网络 ✓
- ✅ 0 遥测 ✓
- ✅ 0 系统依赖(musl 静态链接)x2 架构 ✓
- ✅ 1.8 MB 单文件 ✓
- ✅ G8 门控(递归扫描 src/\*_/_.rs + strip_test_mod)✓
- ✅ blake3 哈希链(与 evorule-governance 交叉验证)✓
- ✅ 结构不变量校验(FactId 单调 + cause 引用)✓
- ✅ FIFO 队列(pop_front,修复原 LIFO bug)✓
- ✅ 确定性加载(文件名排序,跨平台一致)✓
- ✅ e2e 测试 28/28 ✓
- ✅ 可重现构建 ✓

---

## 架构

```
evorule-cli
├── src/
│   ├── main.rs          # 入口:tracing 初始化 + 子命令分发
│   ├── cli.rs           # clap derive 参数定义(5 个子命令)
│   ├── error.rs         # CliError 枚举 + 退出码映射(0/1/2)
│   ├── executor.rs      # 同步反应器循环(FIFO + max_steps + I/O 两阶段)
│   ├── fact_log.rs      # JSONL 读写(tier1 WAL 格式,fact_to_json/fact_from_json)
│   ├── hash.rs          # blake3 哈希链(复制自 evorule-governance,交叉验证)
│   ├── io_util.rs       # 规则加载(确定性排序)+ payload 解析 + 文件读写
│   ├── output.rs        # human-readable 格式化 + diff 前缀
│   └── commands/
│       ├── mod.rs       # 子命令模块声明
│       ├── validate.rs  # validate:core_eval 元指令白名单校验
│       ├── run.rs       # run:加载→执行→输出 fact log
│       ├── replay.rs    # replay:读 fact log → pretty-print
│       ├── diff.rs      # diff:按 FactId 数组下标对齐比对
│       └── verify_chain.rs # verify-chain:哈希链 + 结构不变量
├── build.rs             # G8/F11 编译期门控(递归扫描 + strip_test_mod)
├── tests/
│   ├── e2e.sh           # 28 个 TAP 端到端测试
│   └── fixtures/        # 测试 fixtures(valid/invalid/unknown-type/empty/echo/fifo/multi)
└── examples/            # 圈 2 合规模板(hospital/law-firm)
```

**依赖范围**:

- `evorule-tcb`(纯函数 `execute_transition`)—— 核心 TCB
- `evorule-reactor`(`Fact`/`FactId`/`IoType`/`wal`/`serde_to_tcb`)—— Fact 序列化
- `blake3` —— 哈希链(复制自 evorule-governance,避免拉入 axum/sqlx/reqwest 破坏 musl)
- 不依赖 `evorule-governance`(避免破坏 musl 静态链接)
- 不创建 tokio runtime(`execute_transition` 是同步纯函数)

---

## CI 集成

`.gitee-ci/build-musl.yml` 自动构建并上传产物:

| Stage                 | 输出                                          | 触发                            |
| --------------------- | --------------------------------------------- | ------------------------------- |
| `build:musl:x86_64`   | `evorule-x86_64-unknown-linux-musl` (1.8 MB)  | tag / main / dev/wip / src 变更 |
| `build:musl:aarch64`  | `evorule-aarch64-unknown-linux-musl` (1.4 MB) | 同上                            |
| `verify:reproducible` | (无产物,只校验)                               | tag(v*.*.\*)                    |

每个 build stage 内置 G8 门控校验(通过 evorule-tcb / evorule-cli build.rs 自动执行)。

---

## 配套工具

- **`core_eval.json`** —— 宪法文件(默认在 `evorule-tcb/core_eval.json`)
- **可视化时间旅行调试器** —— 后续做(可视化版 replay，由应用层实现)

---

## 参见(项目级治理文档)

- [**DOCS_INDEX.md**](../DOCS_INDEX.md) —— **所有 L1 公开文档的唯一入口(必读)**
- [根 `README.md`](../README.md) —— EvoRule 主入口,三层架构 + 快速开始
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) —— 贡献流程
- [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md) —— 社区行为准则
- [`CLA-individual.md`](../CLA-individual.md) —— 个人贡献者许可协议
- [`VERSION_STRATEGY.md`](../VERSION_STRATEGY.md) —— 版本号标准
- [`docs/constitution.md`](../docs/constitution.md) —— evorule 仓治理结构(治理模型/决策层级/贡献者阶梯)
- [`docs/oss_strategy.md`](../docs/oss_strategy.md) —— 开源治理策略(AGPL-3.0 解释 + 商业 license 路径)
- [`docs/security/SECURITY_AUDIT_v0.1.0.md`](../docs/security/SECURITY_AUDIT_v0.1.0.md) —— 安全审计基线
- [`docs/security/THREAT_MODEL.md`](../docs/security/THREAT_MODEL.md) —— 威胁模型
