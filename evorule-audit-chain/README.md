# evorule-audit-chain

`EvoRule` 审计链 no_std 内核——Fact 数据形态 + 稳定 JSON 序列化 + BLAKE3 链哈希的**单一真相源**。

- `#![no_std]` + alloc：同一实现同时服务 std 宿主（evorule-reactor / governance / cli 经 re-export 消费）与嵌入式目标（evorule-mcu，SC3）。
- 确定性：同输入同输出，序列化键序由 BTreeMap 固定；`hash_snapshot.txt` 快照测试守护存量哈希不变。
- 上提记录：2026-10-18 SC3-a 方案乙（自 evorule-reactor 迁入，`std::sync::Arc` → `alloc::sync::Arc`，行为逐字一致）。

## 哈希链算法

```text
prev_hash 初始 = "genesis"
content_hash = blake3(fact_to_stable_json(fact))   // 显式 JSON，稳定格式
chain_hash   = blake3(prev_hash + content_hash)
```

## License

AGPL-3.0-or-later
