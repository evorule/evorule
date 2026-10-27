# evid-verify-core

EvoRule EVID 独立验证内核：对审计链数据做**零信任完整性判定**的 `no_std` + `alloc` crate——给定导出文档 / WAL 行序列 / 锚点封条链，从公式重算完成判定，不信任任何外部结论。

## 三面判定（报告式，首红即停）

| 判定面 | 入口 | 步骤 |
|---|---|---|
| 导出面 | `verify::verify_export` | 每条目四步：prev 链接 → 内容哈希（`fact_hash` 公式重算）→ stable 深比（`fact_to_stable_json` 重算与条目原文对账）→ 链折叠；链尾对 `last_hash` |
| WAL 面 | `verify::verify_wal_lines` | 每行三步：prev 链接 → `content_hash` 重算 → `chain_hash` 链折叠 |
| 锚点面 | `seal::verify_seal` / `verify_seal_chain` | 四规则：seq 连续 → 区间无缝 → `anchor_hash` 链 → ed25519 验签（`evorule-anchor/2` 载荷语义） |

断链后折叠链头失去锚，后续条目如实标记未判定（`stopped_at`）；全绿 = 无停止点且链尾与声明尾哈希对账成功。

## 公式纪律（SSOT）

- `fact_hash = blake3(to_string(fact_to_stable_json(fact)))`
- `chain_step = blake3(prev_hex ++ content_hex)`，链首 prev = `genesis`
- `anchor_hash = blake3(prev_anchor ++ '|' ++ 载荷九字段)`，锚点链首 prev = `anchor-genesis`

事实链公式全部经 `evorule-audit-chain` SSOT 调用，本 crate 不私写事实链哈希路径；blake3 直连仅用于锚点载荷哈希（与 server 侧 anchor.rs 逐字段对齐，向量闸守护）。

## fail-closed 判形

- 导出版本仅认 `1.0`，锚点仅认 v2 载荷语义（0.8.1 的 v1 锚点不兼容，混版链判失败，不静默跨格式验过）；
- 未知 Fact 变体 / 缺字段 / 类型不符 / 浮点数字段一律拒收；
- 数字通道与 SSOT 序列化对偶：i64 全域 → `Integer`，u64 高位 → `U64`。

## 使用

```rust
use evid_verify_core::record::parse_export_doc;
use evid_verify_core::verify::verify_export;

let v: serde_json::Value = serde_json::from_str(&input_json)?;
let doc = parse_export_doc(&v)?;   // fail-closed 解析（ParseError 即结构化红）
let report = verify_export(&doc);  // 逐条目判定报告
```

`g5` 黄金向量 11 组 self-test 内嵌（`cargo test` 即验），覆盖解析 → 重算 stable 的 roundtrip 与三面判定。

## 诚实边界

- 本内核判「链未被篡改」，不判「链首记的是真事」——那是签名锚点与门限签名的领域；
- JSON 数字无形态标签：`U64(5)` 解析 roundtrip 后归位 `Integer(5)`（值域保真，两形态 stable 字节等价，不影响哈希判定）；
- 锚点公钥由调用方注入（按 `key_id` 查表），本 crate 不含密钥生成/时钟/随机。

## 下游

- [`evid-verify-wasm`](https://gitee.com/evorule/evorule/tree/main/evid-verify-wasm)：浏览器免安装单文件验证器与 wasm-pack/npm 绑定面；
- 三方 Rust 宿主（移动端 app、server 复核、运维哨兵）直接依赖本 crate 离线验链。
