# evid-verify-wasm

EvoRule EVID 验证器的 wasm 绑定：让审计链判定跑进浏览器/JS 侧——**免安装、零网络请求、判定语义与 Rust 内核逐字节同源**。判定逻辑全部委托 [`evid-verify-core`](https://gitee.com/evorule/evorule/tree/main/evid-verify-core)，本 crate 只做 JSON 报文形态（handlers 共享判定层，永不 panic，解析失败同样产出结构化红）。

## 双路线

### route A — extern "C" 字节 ABI（默认构建，免安装主产物）

零 import、零 JS 胶水，`WebAssembly.instantiate` 拿到模块即用（不挂任何宿主 import）。三入口：`verify_export_wasm` / `verify_wal_wasm` / `verify_seals_wasm`。

调用契约（JS 侧四步）：

```js
const p = wasm_alloc(n);            // 1. 分配入参缓冲
//    写入 n 字节 JSON（入参 = {"entries":[...]} 等对应面报文）
const out = verify_export_wasm(p, n); // 2. 判定（wal/seals 入口同形）
wasm_free(p, n);                    // 3. 归还入参
const len = getUint32(out, true);   // 4. 出参 = [u32 LE 长度][JSON 字节] 包
//    读毕 wasm_free(out, 4 + len) 归还
```

任何路径不 panic（workspace lints deny panic 族）；空指针/零长入参返回结构化错误包。

### route B — wasm-bindgen 绑定面（`--features bindgen`）

wasm-pack / npm 生态友好，字符串进出（如 `verify_export_json(input) -> String`）。不进默认构建——wasm-bindgen 会往模块里挂宿主 import，破坏 route A 的零 import 免安装实例化。

```bash
wasm-pack build --target web --out-dir dist/bindgen -- --features bindgen
```

## 单文件 HTML 验证器（免安装成品）

`dist/index.html`（`build-dist.ps1` 生成）——wasm base64 内嵌的**单文件**验证器：

- 文件拖入 / 粘贴双通道导入；
- 公钥按 `key_id` 手动粘贴建表，**零硬编码信任根**（验签谁由你决定）；
- 导出 / WAL / 锚点三面判定 UI，逐条目展示判定与拒收原因；
- **零网络请求**——air-gapped / 保密环境可直接离线分发。

`dist/fixtures/` 随仓六件套为浏览器实测冻结向量（CI `evid-wasm-gate` 每跑必证：wasm32 产物对七件冻结向量逐字节比对宿主判定结果）。

## 构建

```bash
# route A（cdylib，零 import）
cargo build -p evid-verify-wasm --target wasm32-unknown-unknown --release

# 单文件 HTML 验证器
pwsh ./build-dist.ps1
```
