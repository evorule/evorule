# evo-agent 审计链：可独立验证的可信性白皮书

> **一句话**：你的 AI 助手的每一步操作都被记录在一条防篡改的密码学链条上。你不需要信任我们——你可以亲自验证。

---

## 第一层 · 三十秒读完（给每一个使用者）

当你使用 evo-agent 完成一次任务——问一个问题、执行一个操作、生成一份文档——背后发生了什么？

AI 模型的每次调用、每次返回、每条规则的命中、每次状态变更，都会被追加写入一条**只增不删**的密码学链条。链条上每一环都携带前环的哈希指纹，任何一环被篡改，后续所有环的指纹都会失配——**篡改即红**。

但"我们说它安全"毫无意义。真正重要的是：**你可以自己验**。

一条命令，不装我们的软件也行，用 Python 也行，用任何语言写二十行代码也行——把你手中的审计记录重算一遍哈希，跟链尾值对比。一致，则可信；不一致，则被动过。

更进一步：整条审计链可以导出为一份**标准化证据文档（EVID）**——一个 JSON 文件交给任何第三方，对方用一个单文件 Python 脚本、装一个 blake3 库，就能离线验完。不需要我们的软件，不需要访问我们的服务器。

这不是承诺，是数学。

---

## 第二层 · 五分钟读完（给工程师与技术决策者）

### 它是什么

evo-agent 内置一套三层审计体系加一层公开投影，记录 AI 代理执行过程中的**全部事实**：

| 层 | 解决的问题 | 一句话 |
|---|---|---|
| **事实哈希链** | 记录被改了怎么办 | 每条事实带 BLAKE3 哈希 + 前条链接，改动任何一条即链断 |
| **签名锚点链** | 整条链被重写了怎么办 | 周期性对链尾做 Ed25519 签名封条——整链重算也无法通过绑定验证 |
| **运行时哨兵** | 没人盯着怎么办 | 定时扫描 + 跨扫描基线比对——纯追加与历史被改后重算可精确区分 |
| **EVID 公开投影** | 外部第三方怎么验 | 审计链一键导出为标准化证据文档；单文件验证器离线验链+验签（无密钥设计——投影须与签名锚点联用） |

### 实际记录了什么

以一次真实会话为例——用户向 AI 提问"1+1等于几？"，审计链自动记录了 **20 条事实**：

| 链上事实 | 记录了什么 |
|---|---|
| Command | 用户发送的原始指令（逐字在链） |
| IoRequest → IoResponse | 实际发送给模型的消息集合 + 模型的真实返回（逐字在链） |
| StateTransition | 每次状态变更的完整快照 |
| TransitionTrace | 每条规则的命中/未命中判定（含域归因） |
| Stable | 收敛标记 |
| 会话摘要与偏好沉淀 | 自动捕获的用户偏好（受保护域写入） |
| journal 锚 | 本地操作日志的链尾指纹（绑定到引擎链） |

**没有抽样，没有日志级别过滤**——全部事实，逐条落链。

### 篡改检测实测

我们对这条真实链做了四种攻击测试：

| 攻击方式 | 难度 | 结果 |
|---|---|---|
| 修改一条回答的内容（"2"改"3"，不重算哈希） | 低 | ✅ **检出**：链哈希失配 |
| 删除末尾记录 | 中 | ✅ **检出**：哨兵基线"记录回退" |
| 修改内容 + **重算全部哈希**（使链自身自洽） | 高 | ✅ **检出**：签名锚点绑定失败——锚点声称的链头 ≠ 重算链头 |
| 伪造签名锚点 | 极高 | 需持有签名私钥；密钥不落审计资产 |

**关键**：第三种攻击（整链重算）是哈希链的天然盲区——攻击者改完内容后重算全部哈希，链自身完全自洽。签名锚点链的存在正是为了封堵这一缺口：锚点在链外签名，攻击者没有私钥就造不出能通过绑定验证的假锚。

**证据文档面同样实测**：对 EVID 证据文档的七连破坏注入（改字段 / 删中间记录 / 插入记录 / 删尾 / 整链重算+旧签名 / 同密钥不同链 / 验证器自检）全部检出（8/8），并作为 CI 常驻验收门。

### 怎么验

**方式一：CLI 验证（推荐）**

```bash
# 从 server 导出审计链
curl http://your-server:port/api/sessions/{id}/audit?include_content=true > audit.json

# 验证哈希链 + 签名锚点
evorule verify-chain audit.json
evorule verify-anchors audit.json --pubkey <你的公钥>
```

**方式二：任何语言，二十行代码**

审计链的哈希算法是公开的：`fact_hash = blake3(stable_json_utf8)`，`chain_step = blake3(prev + content)`。你不需要安装 evo-agent——用 Python、Go、JavaScript 任何语言重算一遍即可。我们提供了 11 组跨语言黄金向量（含中文、边界值）供你校验实现正确性。

**方式三：HTTP 端点**

```
GET /api/sessions/{id}/audit/verify     → 链完整性
GET /api/sessions/{id}/anchors          → 签名锚点列表
POST /api/sessions/{id}/anchors/verify  → 三层验证（结构+签名+绑定）
```

**方式四：EVID 证据文档（给外部第三方——零门槛）**

```bash
# 导出方：从 journal 导出标准化证据文档（evid_export 为独立命令行工具）
evid_export --journal data/sessions/{id}.jsonl -o evidence.json

# 验证方：单文件验证器（仅需 Python + blake3，无需安装本软件）
python evid_verify.py evidence.json                  # 链完整性（篡改六分类定位）
python evid_verify.py evidence.json --pubkey <hex>   # + Ed25519 验签（长度防移植）
python evid_verify.py --self-test                    # 验证器自检 4/4
```

篡改六分类：`field_modified` / `record_deleted` / `record_inserted` / `tail_truncated` / `signature_invalid` / `chain_substituted`——不止报红，还告诉你动过哪里。

### 部署者配置

审计链开箱即用（哈希链默认启用）。签名锚点需注入密钥：

```bash
# 一次性生成密钥对
evorule anchor-keygen --output /secure/path/anchor.seed
evorule anchor-pubkey /secure/path/anchor.seed  # 分发给验证方

# server 启动时注入（密钥仅进内存，不落审计资产）
export EVORULE_ANCHOR_SEED=<64-hex-seed>
export EVORULE_ANCHOR_INTERVAL=64  # 每 64 条事实自动封一锚
```

---

## 第三层 · 深入（给安全研究者与审计专家）

### 威胁模型

| 攻击向量 | 检出机制 | 层 |
|---|---|---|
| A1 修改记录内容 | content_hash 重算失配 | 哈希链 |
| A2 插入/删除记录 | prev_hash 链接断裂 | 哈希链 |
| A3 前缀截断 | 锚点 fact_range 区间无缝校验失败 | 锚点链 |
| A4 整链重算 | 锚点 chain_head 与重算链头绑定失败 | 锚点链 |
| A5 整链替换 | 伪造链无有效签名 | 锚点链 |
| A6 引擎替换 | engine_id 入锚点载荷 | 锚点链 |
| A7 锚点前缀截断 | 锚点链 prev 链接断裂 + seq 连续性 | 锚点链 |
| A8 格式降级 | WAL 文件头格式戳缺失 | 文件头 |
| A9 尾截断 | 哨兵基线记录回退 / 锚点 fact_range 超出 | 哨兵/锚点 |
| A10 密钥持有者从头记假账 | **不防**——见诚实边界 | — |
| A11 篡改 EVID 证据文档 | evid_hash 重算失配——六分类定位（改字段/删/插/删尾） | EVID 投影 |
| A12 伪造 EVID 投影链 | 投影层无密钥、自洽链可构造——签名绑定检出：chain_substituted / signature_invalid | EVID+锚点 |

### 哈希契约

**输入规范化**：每条事实（Fact）通过显式字段构造序列化为稳定 JSON——
- 键序固定（BTreeMap 字母序，与 Python `json.dumps(sort_keys=True)` 一致）
- `None` 字段省略（Option::None 与 Python None 行为一致）
- UTF-8 编码，无转义差异

**哈希计算**：
```
content_hash = BLAKE3( stable_json_utf8 )
chain_hash   = BLAKE3( prev_chain_hash_hex ++ content_hash_hex )
```

**跨语言保证**：11 组黄金向量（含 domain_attr 两态 / 中文换行 / i64 边界）由 Rust 实算导出，Python 独立实现对拍 11/11 一致。CI 强制运行对拍——不靠注释宣称兼容。

**签名载荷**（Ed25519 确定性 nonce，RFC 8032）：
```
signature_payload = "evorule-anchor/2|{session_id}|{seq}|{lo}|{hi}|{chain_head}|{key_id}|{engine_id}|{logical_time}"
```
字段以 `|` 串接——第三方验证器仅需 Ed25519 库即可验签，零额外依赖。

**EVID 投影哈希**（证据文档逐条记录，7 字段因果元组全部字符串化/显式 null——杜绝数值编码分裂）：
```
evid_hash = BLAKE3( canonical_json([ str(seq), step_id, fact_type,
                                      cause|null, previous_evid,
                                      fact_hash, domain_check|null ]) )
```
5 组向量 + 创世根由 Python **独立推导**、Rust 测试**逐值断言**（双向锁定，任一实现漂移即红）；CI `evid-parity` 常驻运行（验证器自检 + 向量对拍 + 七连注入验收）。

### 形式化语义

审计链哈希完整性的核心性质——**链步 back-link 归纳**——已通过有界模型检测验证（Kani 0.67.0， unwind=10，SUCCESSFUL）：

> 对任意链 `L`，`compute_chain_hash(L)` 等于逐条 `chain_step` 折叠至末尾。

 差分测试（proptest）常驻 CI：进程内追加路径与 WAL 重放路径产生全等四元组（快照/队列/版本/链尾哈希）。

### 诚实边界

| 不防什么 | 为什么 | 补位 |
|---|---|---|
| 密钥持有者从头记假账 | 密钥持有者可以构造自洽链+有效签名——这是所有签名体系的共同边界 | 多签/外部见证/HSM（部署级） |
| 审计链自身的密钥泄露 | 密钥泄露→签名可伪造 | HSM/密钥轮换（已支持 key_id） |
| 哈希碰撞 | BLAKE3 抗碰撞性是密码学假设，非本系统证明 | 密码学社区持续审计 |
| 审计链节点不可用 | 可用性非完整性范畴 | 部署级冗余 |
| 伪造 EVID 证据投影 | 投影层**故意不含密钥**——任何人都能构造一条自洽投影链，这是"公开可验"的代价 | 投影链不可单独作为"执行确实发生过"的证据；须与签名锚点联用（chain_root 绑定 + 长度防移植签名） |

### 架构分层

```
┌─────────────────────────────────────────────┐
│             签名锚点链（防抵赖）               │
│   Ed25519 · 链头绑定 · 区间无缝 · 密钥轮换    │
├─────────────────────────────────────────────┤
│             事实哈希链（防篡改）               │
│   BLAKE3 · 逐条链接 · write-ahead · 崩溃恢复  │
│   ┌─────────────────────────────────────┐   │
│   │       EVID 公开投影（可外验）         │   │
│   │   标准证据文档 · 单文件验证器 · 无密钥 │   │
│   │   chain_root ← 锚点签名绑定（联用）   │   │
│   └─────────────────────────────────────┘   │
├─────────────────────────────────────────────┤
│             运行时哨兵（防懈怠）               │
│   基线比对 · 前缀锚定 · 跨扫描连续性          │
├─────────────────────────────────────────────┤
│             跨语言验证（防单点）               │
│   黄金向量 · 独立实现 · CI 强制对拍           │
└─────────────────────────────────────────────┘
```

---

## 附：快速开始

```bash
# 1. 克隆仓库
git clone https://github.com/evorule/evo-agent.git

# 2. EVID 证据投影：跨语言向量对拍（克隆即可验，无需运行环境）
pip install blake3
python scripts/evid_verify.py --vectors scripts/evid_golden_vectors.json
python scripts/evid_verify.py --self-test
# 期望输出: vectors: 6/6 与 自检 4/4 PASS

# 3. 事实链跨语言黄金向量（内核仓）
git clone https://github.com/evorule/evorule.git
python evorule/scripts/cross_lang_verifier.py evorule/scripts/g5_golden_vectors.json
# 期望输出: PASS: 11 组向量 / 0 失配

# 4. 从运行中的 server 导出审计链并验证
curl http://localhost:18099/api/sessions/1/audit?include_content=true > audit.json
evorule verify-chain audit.json

# 5. 有 journal 文件时：导出证据文档并用单文件验证器离线验证
cargo run --bin evid_export -- --journal data/sessions/{id}.jsonl -o evidence.json
python scripts/evid_verify.py evidence.json        # 验链；--pubkey <hex> 可加验签
```

## 附：文件索引

| 文件 | 说明 |
|---|---|
| `docs/audit-chain.md` | 本文档 |
| `src/agent/journal.rs` | journal 链哈希实现 |
| `src/agent/journal_anchor.rs` | journal 链 → 引擎链锚定 |
| `src/agent/memory_event/store.rs` | 事件防篡改对账（verify_cause_binding） |
| `src/agent/atif.rs` | 审计轨迹导出（携带引擎链尾） |
| `src/bin/journal_verify.rs` | journal 链独立验证工具 |
| `src/agent/evid.rs` | EVID 证据投影（因果元组 + 哈希契约 + journal 投影器） |
| `src/bin/evid_export.rs` | journal → 标准化证据文档导出 |
| `scripts/evid_verify.py` | 单文件零依赖验证器（链/签名/篡改六分类） |
| `scripts/evid_golden_vectors.json` | EVID 跨语言黄金向量 |
| `scripts/evid_injection_test.py` | 七连破坏注入验收（CI 常驻） |
