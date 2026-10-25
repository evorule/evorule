// S4 移动端黄金向量闸（wasm32 产物侧）：Node 裸实例化 cdylib（零 import），
// 对 dist/fixtures 冻结向量跑三入口，输出与宿主 handlers（core 同源正本，
// 由 tests/wasm_output_dump.rs 落盘）逐字节比对，不等即红 fail-closed。
// 另对单文件 HTML 内嵌 wasm 做行为对拍（防 dist 漂移；因 wasm 内嵌 panic
// 定位路径含构建机 OS 形态，跨 OS 字节恒不等，故不做字节比对——o25）。
//
// 用法：
//   node scripts/evid_wasm_gate.mjs <wasm路径> <fixtures目录> <宿主输出目录> [dist/index.html]
//
// 裸实例化附带属性：cdylib 若被污染出宿主 import（如默认编入 wasm-bindgen），
// `new WebAssembly.Instance(mod, {})` 直接抛错——零 import 契约在 CI 每跑必证。

import { readFileSync } from "node:fs";
import { basename } from "node:path";

const [wasmPath, fixturesDir, hostOutDir, distHtmlPath] = process.argv.slice(2);
for (const [v, label] of [[wasmPath, "wasm"], [fixturesDir, "fixtures"], [hostOutDir, "host-out"]]) {
  if (!v) {
    console.error(`缺少参数: ${label}`);
    process.exit(2);
  }
}

// fixture 文件名 → 判定面（与 tests/wasm_output_dump.rs 的 FACES 对应；
// 任一侧映射漂移即对拍红，自守）
const FACES = {
  export_valid: 1,
  export_tampered: 1,
  self_check: 1,
  wal_valid: 2,
  wal_tampered: 2,
  seals_valid: 3,
  seals_tampered: 3,
};
const ENTRY = { 1: "verify_export_wasm", 2: "verify_wal_wasm", 3: "verify_seals_wasm" };

// ---- ABI 调用序（与 abi.rs 模块级契约逐条对应）----
function callEntry(instance, entryName, inputBytes) {
  const e = instance.exports;
  const inPtr = e.wasm_alloc(inputBytes.length);
  let mem = new Uint8Array(e.memory.buffer); // wasm_alloc 可能扩内存，分配后取新视图
  mem.set(inputBytes, inPtr);
  const outPtr = e[entryName](inPtr, inputBytes.length);
  e.wasm_free(inPtr, inputBytes.length);
  const dv = new DataView(e.memory.buffer);
  const len = dv.getUint32(outPtr, true); // 出包 = [u32 LE 长度][JSON 字节]
  mem = new Uint8Array(e.memory.buffer); // package 分配可能扩内存，读前取新视图
  const out = mem.slice(outPtr + 4, outPtr + 4 + len);
  e.wasm_free(outPtr, 4 + len);
  return out;
}

function fail(msg) {
  console.error(`GATE RED: ${msg}`);
  process.exit(1);
}

// ---- 1. 裸实例化（零 import 契约：污染即抛错即红）----
const wasmBytes = readFileSync(wasmPath);
let instance;
try {
  instance = new WebAssembly.Instance(new WebAssembly.Module(wasmBytes), {});
} catch (e) {
  fail(`裸实例化失败（零 import 契约被破坏？）: ${e}`);
}
for (const need of ["wasm_alloc", "wasm_free", "verify_export_wasm", "verify_wal_wasm", "verify_seals_wasm", "memory"]) {
  if (!(need in instance.exports)) fail(`wasm 缺导出 ${need}`);
}

// ---- 2. 逐向量：wasm 输出 vs 宿主正本输出 逐字节比对 + 极性复核 ----
let checked = 0;
for (const [name, face] of Object.entries(FACES)) {
  const inputBytes = readFileSync(`${fixturesDir}/${name}.json`);
  const out = callEntry(instance, ENTRY[face], inputBytes);

  let verdict;
  try {
    verdict = JSON.parse(Buffer.from(out).toString("utf8"));
  } catch {
    fail(`${name}: wasm 出包非合法 JSON`);
  }
  const expectOk = !name.includes("tampered");
  if (verdict.ok !== expectOk) {
    fail(`${name}: 极性不符——期望 ok=${expectOk}，实得 ${JSON.stringify(verdict).slice(0, 200)}`);
  }

  const expected = readFileSync(`${hostOutDir}/${name}.out.json`);
  if (Buffer.compare(out, expected) !== 0) {
    fail(
      `${name}: wasm 输出与宿主正本输出不一致\n  wasm  : ${Buffer.from(out).toString("utf8")}\n  host  : ${expected.toString("utf8")}`,
    );
  }
  checked += 1;
}

// ---- 3. dist 单文件内嵌 wasm 行为对拍（防 dist 漂移，fail-closed）----
// o25：嵌入 wasm 编有 panic 定位路径（构建机 OS 形态：Windows 反斜杠/本机绝对
// 路径 vs Linux 正斜杠//home/runner），跨 OS 字节恒不等，故不做字节比对——
// 改为抽嵌入 wasm 裸实例化（零 import 契约同证）跑同批冻结向量，输出与宿主
// 正本逐字节比；陈旧/被篡改的 dist 行为不等即红，行为等价的重建产物放行
// （黄金向量判定哲学本就接受该粒度）。
if (distHtmlPath) {
  const html = readFileSync(distHtmlPath, "utf8");
  // wasm base64 约 440K 字符，仓库内无更长同形串；阈值取 2000 避开十六进制串等噪声
  const m = html.match(/[A-Za-z0-9+/]{2000,}={0,2}/);
  if (!m) fail(`${basename(distHtmlPath)} 未找到内嵌 wasm base64`);
  const embedded = Buffer.from(m[0], "base64");
  let distInstance;
  try {
    distInstance = new WebAssembly.Instance(new WebAssembly.Module(embedded), {});
  } catch (e) {
    fail(`dist 内嵌 wasm 裸实例化失败（零 import 契约被破坏或产物损坏）: ${e}`);
  }
  let distChecked = 0;
  for (const [name, face] of Object.entries(FACES)) {
    const inputBytes = readFileSync(`${fixturesDir}/${name}.json`);
    const distOut = callEntry(distInstance, ENTRY[face], inputBytes);
    const expected = readFileSync(`${hostOutDir}/${name}.out.json`);
    if (Buffer.compare(distOut, expected) !== 0) {
      fail(
        `dist 内嵌 wasm 输出与宿主正本不一致（dist 漂移，须重跑 build-dist.ps1）\n  ${name} dist: ${Buffer.from(distOut).toString("utf8")}\n  ${name} host: ${expected.toString("utf8")}`,
      );
    }
    distChecked += 1;
  }
  console.log(`dist embedded wasm 行为对拍一致 (${distChecked} 向量, ${embedded.length} bytes)`);
}

console.log(`GATE GREEN: ${checked} 个冻结向量，wasm32 产物与宿主正本输出逐字节一致`);
