# Kani 形式化验证指南

[tier0-tcb](../) 的 5 个 Kani proof 函数位于 [`tests/kani_proofs.rs`](../tests/kani_proofs.rs)。

## 📋 Proof 清单

| # | Proof | 验证目标 | 期望运行时 |
|---|---|---|---|
| 1 | `verify_value_roundtrip` | JsonValue Integer 构造/访问一致性 | < 1 min |
| 2 | `verify_path_no_panic` | 路径解析对任意输入不 panic | < 1 min |
| 3 | `verify_set_integer_safety` | add 不溢出（`i64::MAX + 1`） | < 1 min |
| 4 | `verify_set_sub_safety` | sub 不溢出（`i64::MIN - 1`） | < 1 min |
| 5 | `verify_transition_bounded` | 空 core_eval 成功 | < 1 min |
| ~~6~~ | ~~`verify_domain_boolean`~~ | ~~域评估返回 bool~~ | 🗑️ 已移除 |

> **已移除的 proof**：`verify_domain_boolean` 因注释与代码矛盾（声称避开 BTreeMap
> 却用了 BTreeMap）导致 TIMEOUT，已于 2026-07-23 移除，改用 proptest 属性测试替代
> （`tests/proptest_props.rs` 的 `domain_eval_never_panics_arbitrary_type` /
> `domain_eval_nested_never_panics`）。详见 `文档/kani/01_验证现状与问题分析.txt`。

> **实测时间未知**: Kani 第一次跑可能因 CBMC 状态爆炸超过 1 小时。建议先跑 `--proof verify_value_roundtrip` 等短 proof 验证环境。

## 🛠️ 安装

### Linux / macOS

```bash
cargo install --locked kani-verifier --version 0.50.0
cargo-kani setup
```

### Windows (WSL Ubuntu 22.04 推荐)

```bash
# 1. 启用 WSL (PowerShell admin):
wsl --install -d Ubuntu-22.04

# 2. 在 WSL 内:
cargo install --locked kani-verifier --version 0.50.0
cargo-kani setup

# 3. 验证:
cargo kani --version
```

### Windows (Docker)

```bash
docker run --rm -v ${PWD}/tier0-tcb:/workspace -w /workspace \
  model-checking/kani:latest cargo kani
```

## 🚀 运行

### 全部 proofs

```bash
cd tier0-tcb
cargo kani --output-format=terse
```

### 单个 proof

```bash
cargo kani --proof verify_value_roundtrip --output-format=terse
```

### 使用项目 wrapper (跨平台)

```bash
# 从 D:\evorule 根目录
./scripts/run-kani.sh                    # 自动检测平台
./scripts/run-kani.sh --list             # 列出所有 proof
./scripts/run-kani.sh --install          # 安装 Kani 到 WSL
./scripts/run-kani.sh --docker           # 用 Docker 跑
./scripts/run-kani.sh verify_value_roundtrip  # 跑单个
```

## 🔧 故障排查

| 症状 | 原因 | 修复 |
|---|---|---|
| `kani: command not found` | 未安装 | `cargo install kani-verifier` |
| `cargo-kani: command not found` | PATH 缺 `~/.cargo/bin` | `export PATH="$HOME/.cargo/bin:$PATH"` |
| `CBMC out of memory` | 单 proof 状态爆炸 | 加 `--output-format=terse` 或拆分 proof |
| `error[E0432]: unresolved import kani` | 未启用 kani feature | 不需要 feature — `cargo kani` 自动注入 `--cfg kani` |
| Windows native 失败 | Kani 不支持 Windows | 用 WSL 或 Docker |

## 📊 CI

`.github/workflows/kani.yml` 在以下情况触发:
- push 到 main 且修改 `tier0-tcb/`
- 任何修改 `tier0-tcb/` 的 PR
- 手动触发 (`workflow_dispatch`) 可指定单个 proof

CI 超时: 1440 min (24h)。如需加速, 改 workflow 矩阵跑单个 proof。

## 📖 延伸阅读

- [Kani 官方文档](https://model-checking.github.io/kani/)
- [tier0-tcb 测试策略](../TEST_REPORT.md)
