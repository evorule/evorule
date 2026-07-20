# `evorule` CLI

**本地 JSON 规则执行工具,面向"圈 2 合规刚需"用户**(医疗/律所/金融/政务等隐私敏感行业)。

> **"evorule 没有智能,只有执行的最佳实践"**
> **"把你公司的合规规则写成一个 JSON 文件,放到本地,evorule 帮你跑 + 审计 + 重放"**

## 特性

- ✅ **零网络** —— 不调用任何外部服务
- ✅ **零遥测** —— 无任何隐式上报
- ✅ **零 AI 决策** —— 不调用 LLM
- ✅ **零系统依赖** —— musl 静态链接,1.6 MB 单文件
- ✅ **完整审计** —— 每步 fact 落盘(JSON Lines 格式)
- ✅ **可对比** —— 两个 fact log 一行 diff
- ✅ **可重放** —— `replay` 子命令播放 fact log
- ✅ **可重现构建** —— 同源码两次构建 SHA256 一致(圈 2 监管可独立复现)
- ✅ **G8 门控** —— 编译期拦截"硬编码控制流"违规(与 tier1/tier2 同套规则)
- ✅ **多架构** —— `x86_64-unknown-linux-musl` + `aarch64-unknown-linux-musl`(AWS Graviton / RPi 适用)

## 快速开始(圈 2 用户)

```bash
# 1) 下载(根据 CPU 架构选)
wget https://gitee.com/evorulelab/evorule/releases/download/v0.1.0/evorule-x86_64
wget https://gitee.com/evorulelab/evorule/releases/download/v0.1.0/evorule-aarch64

# 2) 验证(可选,确认下载完整)
sha256sum -c evorule-x86_64.sha256

# 3) 装上
chmod +x evorule
mv evorule /usr/local/bin/evorule

# 4) 跑你的合规规则
evorule run /etc/company-rules/ -o /var/log/evorule-fact.log

# 5) 出事故?重放
evorule replay /var/log/evorule-fact.log

# 6) 监管检查?导出 fact log
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
cd D:\evorule\evorule-cli  # 或 Linux 上的等效路径
bash build-musl.sh
# 产物: $TARGET_DIR/x86_64-unknown-linux-musl/release/evorule
#   1.6 MB,静态链接,stripped
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

### Windows 开发构建(交叉到 MSVC)

```cmd
cd D:\evorule
cargo build --release --bin evorule
REM 产物: .build\rust\release\evorule.exe
```

## 子命令

### 1. `evorule validate <rules-dir>`

校验 JSON 规则文件 schema。

```bash
evorule validate ./rules/
```

输出:
- `[OK] transform[N]: type='branch'` —— 合法 type
- `[WARN] transform[N]: unknown type 'X'` —— 未知 type(警告不阻断)
- `[ERROR] transform[N]: missing 'type' field` —— 缺少 type 字段(阻断)

合法 type 白名单:`branch`, `set`, `push`, `io_request`, `noop`, `instruction`, `all`, `exists`

退出码:
- 0 = 全部通过(有警告也可)
- 1 = 有错误

---

### 2. `evorule run <rules-dir> [--payload X | --payload-file X] [-o output]`

加载并执行 JSON 规则,输出 fact log(JSON Lines 格式,每行一个 fact)。

```bash
# 默认空 payload
evorule run ./rules/

# 提供初始 payload(JSON 字符串)
evorule run ./rules/ --payload '{"user_id": 42}'

# 从文件读 payload(Windows 友好,避免命令行转义问题)
evorule run ./rules/ --payload-file ./payload.json

# 输出到文件(而非 stdout)
evorule run ./rules/ -o ./fact.log
```

输出示例(JSON Lines):
```json
{"step":1,"type":"state_transition","new_payload":{"x":10}}
{"step":2,"type":"io_required","io_type":"query_db","params":{"query":"SELECT 1"}}
{"total_steps":2,"type":"final","final_payload":{"x":10}}
```

退出码:
- 0 = 正常结束
- 1 = 加载或执行错误

---

### 3. `evorule replay <fact-log>`

播放 fact log(pretty-print,每步一行)。

```bash
evorule replay ./fact.log
```

输出:
```
=== Replaying ./fact.log ===
[   1] state_transition
[   2] io_required
=== End ===
```

---

### 4. `evorule diff <a.log> <b.log>`

对比两个 fact log(基于行集合差异)。

```bash
evorule diff ./before.log ./after.log
```

输出:
```
=== Diff ./before.log <-> ./after.log ===
Only in A (2):
  - {"step":1,"type":"state_transition",...}
  - ...
Only in B (1):
  + {"step":1,"type":"state_transition",...}
```

如果完全相同,输出 `(identical)`。

---

## G8 门控(编译期拦截)

evorule-cli 的 `build.rs` 强制执行 G8 + F11 + §5.2,**与 tier1-reactor / tier2-governance 同一套规则**:

| 规则 | 禁止 | 目的 |
|---|---|---|
| **G8** | `"conditional"` / `"while_loop"` / `"sequence"` 字面量 | 反应器/治理层/CLI 都不得展开控制流 |
| **F11** | `debug_assert!` / `.unwrap(` / `.expect(` | 主代码路径不 panic-prone |
| **§5.2** | `"math_rule"` / `"summarize"` 等业务术语 | 业务逻辑由数据驱动,不硬编码 |

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
              "attr": "__exec__.instruction.params.attr",
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

**多文件**:`run` 会自动合并目录下所有 `*.json` 文件的 `transform` 数组。

**两种支持的格式**:
- `{"transform": [...]}` —— 标准格式
- `{"transforms": [...]}` —— 别名(同义)
- 单个对象(无 transform 数组)—— 当作 1 条 transform

---

## 圈 2 合规刚需演示(30 秒话术)

> "把你公司的合规规则写成一个 JSON 文件,放到本地文件夹。
> 运行 `evorule run ./rules/`,它会:
> 1. 执行你的规则
> 2. 记录每一步到本地 fact log(JSON Lines)
> 3. **不会联网,不会上报,不会 AI 决策**
> 4. 出事故?`evorule replay fact.log` 重放
> 5. 监管检查?fact.log 本身就是审计证据
> 6. 供应链可信?同源码两次构建 SHA256 一致,监管可独立复现"

**对比 Excel 宏 / VBA**:
| 维度 | Excel 宏 | evorule |
|---|---|---|
| 规则格式 | 散在各个 cell | 集中 JSON,版本控制友好 |
| 执行可重复 | 不一定 | 100% 确定性 + 可重现二进制 |
| 审计追踪 | 没有 | 完整 fact log + 哈希链 |
| 跨文件规则 | 难 | 简单(目录扫描) |
| 时间回放 | 撤销 5 步 | 任意时刻回放 |
| 监管汇报 | 手工截图 | 自动导出 fact.log |
| 架构选择 | 仅 x86 | x86_64 + aarch64(Graviton/RPi) |

---

## 圈 2 分发清单(给合规用户)

| 物料 | 来源 | 大小 |
|---|---|---|
| `evorule-x86_64-unknown-linux-musl` | CI artifact / release | 1.6 MB |
| `evorule-aarch64-unknown-linux-musl` | CI artifact / release | 1.4 MB |
| `*.sha256` | 校验文件 | < 1 KB |
| AGPL-3.0 许可证文本 | 随源码提供 | - |
| 用户规则文件 | 用户自备 | - |

**目标交付命令**:
```bash
# Linux x86_64 圈 2 用户
wget https://gitee.com/evorulelab/evorule/releases/download/v0.1.0/evorule-x86_64
chmod +x evorule-x86_64
./evorule-x86_64 validate /etc/company-rules/
./evorule-x86_64 run /etc/company-rules/ -o /var/log/evorule-fact.log

# Linux aarch64 圈 2 用户 (AWS Graviton / RPi)
wget https://gitee.com/evorulelab/evorule/releases/download/v0.1.0/evorule-aarch64
chmod +x evorule-aarch64
./evorule-aarch64 run /etc/company-rules/ -o /var/log/evorule-fact.log
```

---

## 端到端测试

`tests/e2e.sh` 是一个 TAP 格式的 e2e 测试,覆盖 4 个子命令的 19 个用例:

```bash
# 自动检测二进制
bash tests/e2e.sh

# 显式指定
bash tests/e2e.sh .build/rust/x86_64-unknown-linux-musl/release/evorule
bash tests/e2e.sh .build/rust/aarch64-unknown-linux-musl/release/evorule
```

输出示例:
```
ok 1 - --version exits 0
ok 2 - validate valid rule exits 0
ok 3 - validate invalid rule exits 1
ok 4 - validate unknown-type warns but passes
ok 5 - validate empty dir exits 1
ok 6 - validate nonexistent dir exits 1
ok 7-9 - run payload variants
ok 10 - run -o writes valid JSONL
ok 11-12 - replay normal / missing
ok 13-14 - diff identical / different
ok 15 - all fact log lines are valid JSON
# tests 15 | passed 15 | failed 0
```

**aarch64 验证**:用 `qemu-user-static` 模拟跑(不需要真 ARM 机器):

```bash
sudo apt install qemu-user-static
bash tests/e2e.sh .build/rust/aarch64-unknown-linux-musl/release/evorule
```

**CI 集成**:`.gitee-ci/build-musl.yml` 的 `test:e2e:x86_64` stage 每次 build 后自动跑。

---

## 示例规则(给圈 2 用户开箱即用)

`examples/` 下放了两套真实场景的合规规则,每套都包含规则文件、示例 payload、README:

| 目录 | 适用对象 | 法规对应 |
|---|---|---|
| [`examples/hospital/`](examples/hospital/) | 医院信息科 / 病案室 | HIPAA / 等保 2.0 / 《个人信息保护法》 |
| [`examples/law-firm/`](examples/law-firm/) | 律所合规部 | 律师执业规范 / 客户保密 / GDPR Art.30 |

每套 3 条核心规则(覆盖 访问审计 / 权限检查 / 隐私脱敏),复制整个目录到 `/etc/your-rules/` 即可落地。

**30 秒试用**:
```bash
cd examples/hospital/
evorule validate ./rules/
evorule run ./rules/ --payload-file payload.example.json
```

详细说明 + 监管对话脚本见 `examples/README.md`。

---

## 已知限制(0.1.0)

- ❌ 无 I/O handler(MVP 只做 `noop` + state transition,有 `io_request` 会终止并输出)
- ❌ 无 HTTP API(那是 `evorule-server` 的事,在 tier2-governance crate)
- ❌ 无配置文件(后续加 `.evorule.toml`)
- ❌ 无 hot-reload(后续加)
- ✅ 0 网络 ✓
- ✅ 0 遥测 ✓
- ✅ 0 系统依赖(musl 静态)x2 架构 ✓
- ✅ 1.6 MB 单文件 ✓
- ✅ G8 门控 ✓
- ✅ e2e 测试 15/15 ✓
- ✅ 可重现构建 ✓
- ✅ 可重现构建 ✓

---

## CI 集成

`.gitee-ci/build-musl.yml` 自动构建并上传产物:

| Stage | 输出 | 触发 |
|---|---|---|
| `build:musl:x86_64` | `evorule-x86_64-unknown-linux-musl` (1.6 MB) | tag / main / dev/wip / src 变更 |
| `build:musl:aarch64` | `evorule-aarch64-unknown-linux-musl` (1.4 MB) | 同上 |
| `verify:reproducible` | (无产物,只校验) | tag(v*.*.*) |

每个 build stage 内置 G8 门控校验(通过 tier0-tcb / evorule-cli build.rs 自动执行)。

---

## 配套工具

- **`evorule-server`** —— HTTP + SSE 服务的二进制(D:\evorule\tier2-governance\)
- **`core_eval.json`** —— 宪法文件(默认在 `tier0-tcb/core_eval.json`)
- **evorule-application/time-travel-debugger** —— 后续做(可视化版 replay)
