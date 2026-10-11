# 社区贡献授权签发记录 · GRANT-2026-ALE-001

**依据**: [CONTRIBUTION_GRANT.md](../../../CONTRIBUTION_GRANT.md) v1.0（通道 D）
**目标社区**: Agents' Last Exam（ALE）——UC Berkeley 主导的学术评测项目（Engineering track 贡献）
**签发日期**: 2026-10-11
**签发人**: Mr. DAMU ZHENG（EvoRule Project 创始人、唯一版权人）

---

## 一、授权物清单

目标：WASM 可验证判卷组件（设计文档：ALE 专项研究 01-A v1.2——**内部档，不入公开提交包**）。

| # | 授权物 | 形态 | 再许可协议 |
|---|---|---|---|
| 1 | WASM 判卷引擎（`evorule-wasm-demo` 子集源码+编译二进制） | Rust 源码子集 + `.wasm` 产物 | Apache-2.0 |
| 2 | 内含生态源码子集：`evorule-tcb`（TCB 判定核）、`evorule-reactor`（执行引擎）、`evorule-governance`（治理/审计）——以 wasm 二进制静态/动态包含的编译产物计 | 编译产物（含源指针） | Apache-2.0（随件） |
| 3 | bridge 库（`wasmfacts.py`，产物→facts 组装层） | 单文件 Python | Apache-2.0 |
| 4 | demo/hello WASM grader 改造版 | 任务包（tasks/demo/hello_wasm/） | Apache-2.0 |
| 5 | 真实任务示例 1~2 件 | 任务包 | Apache-2.0 |
| 6 | verify.html 独立验证页 | 单文件 HTML | Apache-2.0 |
| 7 | DETERMINISM.md+复跑脚本 | 文档+脚本 | CC-BY-4.0 |
| 8 | README（设计说明+动机） | 文档 | CC-BY-4.0 |

**版本锚定**: 授权物源码基线=evorule 主仓 tag `v0.8.3`（commit `cdb0772`）起的
`evorule-wasm-demo` 及其 path 依赖三 crate；提交包内每件附 BLAKE3 或 commit 锚。

## 二、义务（对 ALE 社区及其下游）

依 CONTRIBUTION_GRANT.md 第五节：署名保留（Apache-2.0 §4）/源指针保留/
非背书声明/范围诚实（本清单外的一切——含 EvoRule 全仓其余部分与商标——不在授权内）。

## 三、签发

- 依据 AUTHORS.md（唯一作者）与 GOVERNANCE.md §一（创始人对许可证最终拍板权），
  **单签即生效**；
- 主仓与全部生态仓的 AGPL-3.0-or-later 主线**不变**；
- 本记录追加制存档，不删改。

**签发人签字**: __________________　Mr. DAMU ZHENG（evorulelab@gmail.com）
**日期**: 2026 年 10 月 11 日
**git 锚**: 本 commit（签发记录入库即锚定）
