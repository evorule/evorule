<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 验证披露日志（DISCLOSURE_LOG）

> **性质**：记录一切对既定验证计划的偏离与历史状态修正（[MECHANISM.md](MECHANISM.md) M5）。
> **记录规范**：只记事实——什么变了、客观依据、影响、修正去向（M10）。
> **条目格式**：事实 / 依据 / 影响 / 修正去向。

## 首条（2026-09-12）：验证机制建立与历史状态补记

验证机制（MECHANISM.md M1–M11）与验证状态单一真相源（STATUS.md）于 2026-09-12 建立。建立时对公开仓验证文档与实际验证状态做了一次全局核对，以下 12 项为核对发现的不符/漂移及修正去向（一次性补记）。

### 漂移根因说明

本次核对同时确认：上述漂移并非故意隐瞒——EvoRule 迄今经历三次大的重构，大量功能（含部分形式化验证资产）在重构过程中移出至本仓之外的其他目录；漂移前的代码与文档大部分仍存续，其记录（包括 PASS、FAIL 乃至工具崩溃）均为如实记录。漂移的成因是重构后公开仓文档未同步更新、资产与文档的对应关系断裂，而非记录本身不诚实。本补记旨在重建对应关系、修正表述；对重构前历史记录的诚实性予以确认。

| # | 事实（核对发现） | 依据 | 影响 | 修正去向 |
| - | ---------------- | ---- | ---- | -------- |
| 1 | 三层漂移：代码与 CI 为 v0.5.0（2026-09-12，四条验证 CI 落地）；证据库冻结于 2026-08-18（全部产自 v0.3.1）；验证文档冻结于 2026-08-17 | 2026-09-12 全局核对 | 状态类声明整体与实况不符 | STATUS.md 建立（v0.5.0 快照）；不符文档按修正批次处理 |
| 2 | `evorule-tcb/verification/evidence/kani/` 下 16 个证据文件全部不符合 evidence/README.md 命名规范：`single.log` 含无名 FAILED 记录；`p4_t2.log` 为工具崩溃日志；`p4567_tmp.log`、`p8_11.log` 为仅含单行 harness 标题的残文件；`ps_check.txt`、`ps_count.txt` 为进程诊断残留 | 对照 evidence/README.md §三 | TCB Kani 证据链整体失效 | 12 个 `git mv` 至 `_invalidated/`；4 个零价值文件 `git rm`（处置记录③） |
| 3 | 2026-09-09 enforce halt 语义修改了 proof 源码（`kani_proofs.rs`），对应证据未重跑 | git 提交历史 | 旧证据（已隔离）与当前代码不一致 | 阶段 2 于 v0.5.0 重跑归档 |
| 4 | plan v3 属性表 P0-4 标注「✅ 实跑†」与实测记录不符：两代方法均未 PASS，且该文档内部表述自相矛盾 | 2026-09-12 全局核对 | P0-4 状态与实况不符 | STATUS.md：P0-4 = ❌+🔵（proptest 兜底）；plan v3 状态列剥离（修正批次） |
| 5 | 原 INDEX.md 称 TLA+「⏳ 未实现」，实际 TLC 验证报告已存在（2026-07-25，降级模型 N_MAX=2） | `evorule-tcb/tla/TLC_VERIFICATION_REPORT.md` | TLA+ 侧覆盖被低估 | STATUS.md：P0-7/P0-8 = ❌+🟡 |
| 6 | GATE_REFERENCE.md L250 含与实况不符的完成声明（验证项未完成但标已完成） | 2026-09-12 全局核对 | 门禁文档与实况不符 | 修正批次：以五档词汇改写 |
| 7 | 根 README badge 数字与实况不符：「45 proofs (12 verified)」「Pending CI 33」与当前 37-proof（A 档 14 可跑 + B 档 23 不可运行）实况不符 | 对照 kani.yml 与 proof 清单 | 对外表述与实况不符 | 修正批次：badge 及正文（L10、L281-287、中文镜像 L770-776 等） |
| 8 | `evorule-reactor/docs/KANI.md` CI 段与 kani.yml 实况漂移；「10/11 PASS」为 2026-07-27 旧版本口径且未锚定版本 | 对照 kani.yml | reactor 文档与实况不符 | 修正批次（3 处） |
| 9 | 4 处拼写错误死引用（EVORULE_FORMAL_VERTICATION / VERTIFICATION 系列） | 全仓检索 | 引用失效 | 修正批次（保留 2 处登记性出现：DOCS_INDEX.md:159、check_doc_safety.py:383） |
| 10 | `evorule-tcb` 设计稿 P1–P21 proof 编号与 P0-xx/P1-xx 属性编号命名空间冲突 | 2026-09-12 全局核对 | 编号歧义 | MECHANISM.md M8 显式作废旧编号；映射表入 STATUS.md 附录 A |
| 11 | B 档 23 个 TCB proof 判定当前不可运行：实测 600s 全超时（其中 2 个实际跑 910s+）；3600s 仍超时（`verify_merge_safe` 跑满 3603s）；unwind 4/8 无改善 | kani.yml 头注实测记录（2026-09-11，Kani 0.67.0 / WSL Ubuntu 22.04） | P0-1/2/4/5/7/8 的 Kani 侧覆盖缺失 | STATUS.md 标 ❌；kani.yml B 档仅手动触发且允许失败 |
| 12 | 本仓 Coq 形式化为 0 行（部分形式化验证资产已随重构移出本仓，见漂移根因说明）；TLA+ 仅 1/6 模型（ExecuteTransition，N_MAX=2 降级） | 2026-09-12 全局核对 | L1 层覆盖声明须限定 | STATUS.md 如实标注 |

### 资产处置记录

| # | 事实 | 依据 | 影响 | 修正去向 |
| - | ---- | ---- | ---- | -------- |
| ① | `verification/INDEX.md` 删除 | 功能被 MECHANISM.md（规则）、STATUS.md（状态）、README.md（导航与登记）三方吸收，消灭第二状态源（M1） | 存量引用（DOCS_INDEX.md ×4、plan v3 ×2 等）需更新 | 修正批次更新全部引用 |
| ② | 根目录 `CHANGE_REQUEST_TEMPLATE.md` 删除 | 与 `.github/` 版内容漂移 5 行，双份维护实证 | 仅保留 `.github/` 版 | DOCS_INDEX.md 登记核对 |
| ③ | 4 个零证据价值文件（`ps_check.txt`、`ps_count.txt`、`p4567_tmp.log`、`p8_11.log`）直接 `git rm`，不随批隔离 | 进程诊断残留与仅含单行 harness 标题的残文件，无历史证据价值（M3.3）；git 历史可溯 | 无 | — |

### 初值说明

- 本日志建立于 2026-09-12；上述 12 项核对发现与 3 项处置为机制建立时的一次性补记，此后所有偏离按时间顺序追加于下方。
- STATUS.md 初值快照为 v0.5.0（commit `5fac8bd`）；表中 ✅ 项的 v0.5.0 归档证据由整改阶段 2 重跑后落盘，届时同批更新证据列。
- 机制文档（MECHANISM.md / STATUS.md / DISCLOSURE_LOG.md / verification/README.md 重写稿）以未提交状态起草，定稿后由仓库维护者提交。
- 机制建立决策记录见 [docs/adr/ADR-0001-验证状态单一真相源与诚实披露机制.md](../docs/adr/ADR-0001-验证状态单一真相源与诚实披露机制.md)。

## 追加区

（此后按时间顺序追加，格式：日期 + 事实 / 依据 / 影响 / 修正去向）

### 2026-09-12：证据隔离批次 1 执行 + 表述修正

- **事实**：阶段 1 证据隔离执行——12 个 TCB 旧证据 `git mv` 至 `evorule-tcb/verification/evidence/kani/_invalidated/`，4 个零价值文件 `git rm`；隔离区 README 落盘（作废原因与批次记录）。同时修正首条 #2 与处置记录③ 中「0KB 空文件」的不准表述：`p4567_tmp.log`（46 字节）与 `p8_11.log`（49 字节）实际各含单行 harness 标题、无验证结果，非 0KB。
- **依据**：文件实测内容（46B/49B，各一行 `===== <harness名> =====`）；M3.5 隔离约定。
- **影响**：零价值判定与处置（直接删除）不变；仅描述精度修正。隔离后 `kani/` 目录为空（待阶段 2 重跑归档）。
- **修正去向**：本条目即修正记录；隔离详情见 `evorule-tcb/verification/evidence/kani/_invalidated/README.md` 批次 1。

### 2026-09-12：阶段 2 证据重跑归档执行（v0.5.0 基线）

- **事实**：v0.5.0 证据重跑归档完成，共 23 份证据落盘（WSL，Kani 0.67.0 + nightly-2025-11-21）——18 份 PASS + 5 份如实保留的失败过程记录：TCB A 档 14 个 proof 全 PASS；reactor `invariant_version_monotonic` / `max_rounds_termination` PASS；差分 P0-12 与 P0-9-P0-10 PASS（PROPTEST_CASES=1000，后者前两次失败——①编译期 ENOMEM（超时被杀 proof 的 CBMC 孤儿进程占用内存），②`-j 2` 低并行下 1800s 编译未完——第三次环境净化后默认并行 PASS，两次失败证据如实并档保留）。证据基线 commit 为 `bdfb8d4` 而非快照 `5fac8bd`：其间仅有文档/证据整理提交（阶段 0/1），proof 源码未变，M3.4 满足。
- **依据**：各证据 .log 元数据（commit SHA / 工具链 / 运行命令 / 超时上限）；P0-11 三份超时 FAIL 证据（含完整输出与复现命令，M3.2）。
- **影响**：① P0-11 主状态由 ✅ 调整为 ❌+🟡——本地 3 次实测超时（300s+unwind4 / 1200s+unwind4 / 300s 默认 unwind，均未完成求解）；🟡 历史 PASS 2026-07-27（27s），其后 proof 源码（`7da4045`）与被验证代码（`42fe5a2` 等）均有变更；② kani.yml reactor job 含 P0-11（`--default-unwind 4` + 单 proof 300s），该 job 自落地起从未实跑，按本地实测其首跑预计超时，配置修正另行处理；③ STATUS.md 证据列全面更新，P0-3 计数笔误同批修正（"A 档 10 个"→11 个，漏列 `verify_array_index_bounds`，附录 A/B 佐证）。
- **修正去向**：STATUS.md（快照/起草说明 + P0-3/P0-6/P0-9/P0-10/P0-11/P0-12/P1-3/P1-6 八行）；旧差分证据 4 份（8b2932e 版本）`git mv` 隔离至各自 `_invalidated/`（reactor/governance 两个隔离区 README 落盘）。

### 2026-09-12：阶段 3 文档修正批次执行

- **事实**：11 处既定修正全部落地（plan v3 状态列剥离、TCB KANI.md 重写、reactor KANI.md 3 处、TLA 三文件与 tla.yml 死引用改指 v3 并 pin TLC v1.7.4、根 README 数字与过时表述、GATE_REFERENCE/ROADMAP/DOCS_INDEX/Cargo.toml 对齐、根 CHANGE_REQUEST_TEMPLATE.md 删除）；待核对 10 项完成——失实 5 处按五档词汇修订（DESIGN_PHILOSOPHY.md L224「已根治」表述与 proof 计数、L231 补 TLC 有限模型限定、TCB_SPEC.md §六状态断言、CONTRIBUTING.md / CONTRIBUTING_ZH.md Kani 运行命令），属实 5 处核对结论记入 STATUS.md 维护区；另修正 2 处遗漏的 §8.6.2bis 旧章节引用（TLC 报告 L36、ExecuteTransition.tla L27）；M4 发布同步挂接 PR 模板检查项与 release.yml `release-gate` job（核对 STATUS.md 快照版本 = tag 版本、kani.yml `kani-tcb-a-tier` job 在待发布 commit 上绿）。
- **依据**：全仓检索（旧章节引用残留）；`evorule-tcb/Cargo.toml` `[features]` 实况（仅 `std`，不存在 kani feature）；STATUS.md 附录 A/B。
- **影响**：公开仓验证类表述与 STATUS.md 单一真相源对齐；CONTRIBUTING 中不可运行的 Kani 命令（`--features kani` 指向不存在的 feature，且缺 `--tests`）替换为可运行命令；发布流程具备 M4 可执行门禁。
- **修正去向**：本条目即修正记录。同批收尾：`evorule-tcb/scripts/` 下 6 个仍向 `verification/evidence/kani/` 旧路径产出不合规命名证据的 v0.3.1 时代脚本（`fill_kani_p123_layer2` / `launch_kani_p4567` / `run_kani_p8_11` / `run_kani_p4cde` / `run_p5` / `run_single_log`）退役删除（`git rm`）——合规运行命令已由 evorule-tcb/docs/KANI.md 承载，脚本无独有价值；保留的 4 个辅助脚本（`run_kani_{tcb,p123,p4567}.sh` / `run_single.sh`）经核对不产出证据文件。


### 2026-09-12：阶段 4 验收执行与遗留表述修正

- **事实**：验收前修正 3 项遗留：① kani.yml `kani-reactor` job 将 P0-11（`invariant_cause_queue_sync`）移出 PR 闸门，闸门保留 2 个 v0.5.0 重跑 PASS 的 proof（`invariant_version_monotonic` / `max_rounds_termination`），P0-11 与 B 档同性质（实测不可运行）不进闸门；同步更新 evorule-reactor/docs/KANI.md（CI 子集 3→2、非 CI 清单 8→9、Proof 6 补当前状态注记）、STATUS.md（P0-11 行与附录 C）、verification/README.md、plan v3 §五、evorule-reactor/Cargo.toml `[package.metadata.kani]`。② evorule-tcb/README.md 两处过时声明（「✅ P1-P21 已完成 / 34 个 / 已根治 / 17 个 evidence log」）按五档词汇改写。③ 本日志阶段 3 条目依据行移除内部计划性引用（M9/M10）。验收执行中新发现并修正：根 README 中文区 Kani badge 仍为「45 proofs (12 verified)」（首条 #7 的中文镜像修正漏及 badge）、中英目录树注释仍为「34」，均改齐 48/16/37；INDEX.md 已删除后的 3 处死引用（.gitignore、evorule-reactor/Cargo.toml 注释、evorule-tcb/tests/kani_entry.rs 文档注释——阶段 3 引用更新未覆盖非 md 文件）改指 verification/README.md。
- **依据**：P0-11 三份超时 FAIL 证据（`P0-11.invariant_cause_queue_sync_FAIL_bdfb8d4_20260912_*`，第三次为去掉 `--default-unwind` 的重试）；两个入闸 proof 的 v0.5.0 PASS 证据（`P1-3` / `P1-6`，运行命令与 kani.yml 逐字一致）；全仓检索（INDEX.md 残留、数字残留）。
- **影响**：kani-reactor job 首跑不再因含已知不可运行 proof 而必然超时；reactor PR 闸门覆盖 2 个 proof；对外数字口径 48/16/37 全面对齐。
- **验收结论**：check_doc_safety.py RC=0（R3 私有路径零残留）；check_docs_bilingual.py 通过（27 篇）；VERTICATION 拼写残留 2 处均为登记性（check_doc_safety.py 禁用清单、本日志首条 #9）；kani/tla/release/mutants 四个 workflow yml 解析通过；`cargo test --workspace --all-targets` 全绿（含 TCB build.rs 门禁，源文件注释修改零破坏）；INDEX.md 死引用清零（余 4 处为「原 INDEX.md」历史性记述，按 M7 保留）；48/37/14/23/16/11/2 七组数字在根 README 双语、两份 KANI.md、STATUS.md、kani.yml、Cargo.toml 元数据全部对齐。
- **修正去向**：本条目即修正记录。

### 2026-09-12：P0-11 `invariant_cause_queue_sync` 超时根因修复，重入 CI 闸门

- **事实**：定位并修复 P0-11 超时根因——CBMC 将 `VecDeque` 堆缓冲区中的 `JsonValue` 按任意变体建模，任何触发 `JsonValue` Drop 的路径（`pop_instruction` 返回值 Drop、`clear_queue` 的 `drop_in_place`、`ReactorState` 整体 Drop）都会展开 `Object(BTreeMap)` 红黑树析构的 `first_leaf_edge` 无界 unwind，导致状态爆炸（诊断实验：`new()`+forget 0.09s PASS、`push_back`+forget 0.49s PASS、加入单次 `pop_instruction` 即卡死于 first_leaf_edge unwind——最小复现成立）。修复三处：① proof 侧 `std::mem::forget(popped)` / `forget(state)` 跳过析构（证明 9 先例）；② `ReactorState::clear_queue`（evorule-reactor/src/state.rs）增加 `#[cfg(kani)]` 分支（`mem::take`+`forget`，长度清空语义与真实 clear 一致，与 kani_collections 轻量实现模式同理）；③ P0-11 重入 kani.yml reactor job（单独命令，不带 `--default-unwind 4`——该配置此前实测对 P0-11 无效）。修复后本地实测 1.53s PASS（WSL，Kani 0.67.0 + nightly-2025-11-21，647 断言 0 失败）；`cargo test -p evorule-reactor` 全绿（185 项，`clear_queue` 的 kani 分支不参与普通编译）。
- **依据**：诊断实验序列实测记录（上述耗时与卡死输出）；超时输出中 `NodeRef<Dying, String, JsonValue>::first_leaf_edge` 循环 unwind 迭代 1400–2600+ 次；修复前 3 份 FAIL 证据（`P0-11.invariant_cause_queue_sync_FAIL_bdfb8d4_20260912_*`，含完整输出与复现命令）。
- **影响**：P0-11 主状态由 ❌+🟡 调整为 ✅（正式 PASS 证据按 M3.4 于修复提交后归档，届时补 STATUS.md 证据列）；kani-reactor job 由 2 个增至 3 个 proof；对外数字口径「当前实跑验证」16→17（根 README 双语 badge 与正文、STATUS.md、reactor KANI.md、Cargo.toml `[package.metadata.kani]`、ROADMAP、plan v3 §五 同步更新）。
- **修正去向**：本条目即修正记录；状态见 STATUS.md（P0-11 行与附录 C）。

### 2026-09-12：P1-5 `command_does_not_decrease_queue` 超时根因修复，入 CI 闸门

- **事实**：应用与 P0-11 相同的修复（proof 末尾 `std::mem::forget(state)` 跳过 `ReactorState` 的 Drop），P1-5 由超时恢复为可运行：本地实测 0.57s PASS（默认参数）/ 0.60s PASS（`--default-unwind 4`，与 CI 参数一致），入 kani.yml reactor job（for 循环内，带 `--default-unwind 4`）。
- **依据**：与 P0-11 同根因（CBMC 对 VecDeque 堆缓冲区中 JsonValue 按任意变体建模，state 整体 Drop 展开红黑树析构无界 unwind）；`apply_command` 即 `state.push_back`（evorule-reactor/src/pure.rs），proof 路径无其他爆炸点。
- **影响**：P1-5 主状态由 🟡 调整为 ✅（正式 PASS 证据按 M3.4 于修复提交后归档，届时补 STATUS.md 证据列）；kani-reactor job 由 3 个增至 4 个 proof；对外数字口径「当前实跑验证」17→18（根 README 双语 badge 与正文、STATUS.md、reactor KANI.md、Cargo.toml `[package.metadata.kani]`、plan v3 §五 同步更新）。
- **修正去向**：本条目即修正记录；状态见 STATUS.md（P1-5 行与附录 C）。
