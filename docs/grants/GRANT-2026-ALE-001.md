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

---

<a id="english"></a>

# Community Contribution Grant Record · GRANT-2026-ALE-001

**Basis**: [CONTRIBUTION_GRANT.md](../../../CONTRIBUTION_GRANT.md) v1.0 (Channel D)
**Target community**: Agents' Last Exam (ALE) — an academic evaluation project led by UC Berkeley (Engineering track contribution)
**Date of issuance**: 2026-10-11
**Issuer**: Mr. DAMU ZHENG (founder and sole copyright holder of the EvoRule Project)

---

## I. Grant Asset Inventory

Target: WASM verifiable grading component (design document: ALE special research 01-A v1.2 — **internal document, not part of the public submission package**).

| # | Granted asset | Form | Relicensing terms |
|---|---|---|---|
| 1 | WASM grading engine (source subset + compiled binary of `evorule-wasm-demo`) | Rust source subset + `.wasm` artifact | Apache-2.0 |
| 2 | Embedded ecosystem source subsets: `evorule-tcb` (TCB decision core), `evorule-reactor` (execution engine), `evorule-governance` (governance/audit) — counted as compiled artifacts statically/dynamically included in the wasm binary | Compiled artifacts (with source pointers) | Apache-2.0 (bundled) |
| 3 | Bridge library (`wasmfacts.py`, artifact→facts assembly layer) | Single-file Python | Apache-2.0 |
| 4 | Modified demo/hello WASM grader | Task package (tasks/demo/hello_wasm/) | Apache-2.0 |
| 5 | Real task examples, 1–2 items | Task package | Apache-2.0 |
| 6 | verify.html standalone verification page | Single-file HTML | Apache-2.0 |
| 7 | DETERMINISM.md + rerun scripts | Document + scripts | CC-BY-4.0 |
| 8 | README (design notes + motivation) | Document | CC-BY-4.0 |

**Version anchoring**: the granted source baseline = `evorule-wasm-demo` and its three path-dependency crates as of main-repository tag `v0.8.3` (commit `cdb0772`); each item in the submission package carries a BLAKE3 hash or commit anchor.

## II. Obligations (for the ALE community and its downstream)

Per CONTRIBUTION_GRANT.md Section 5: attribution retention (Apache-2.0 §4) / source-pointer retention / non-endorsement statement / scope honesty (everything outside this inventory — including the rest of the EvoRule repositories and trademarks — is not covered by this grant).

## III. Issuance

- Per AUTHORS.md (sole author) and GOVERNANCE.md §1 (final licensing authority of the founder),
  **a single signature makes it effective**;
- The AGPL-3.0-or-later mainline of the main repository and all ecosystem repositories **remains unchanged**;
- This record is archived append-only and is never edited or deleted.

**Issuer signature**: __________________　Mr. DAMU ZHENG (evorulelab@gmail.com)
**Date**: October 11, 2026
**git anchor**: this commit (the record is anchored upon entering the repository)
