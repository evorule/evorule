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
| ② | 根目录 `CHANGE_REQUEST_TEMPLATE.md` 删除 | 与 `.github/` 版内容漂移 5 行，双份维护实证 | 仅保留 `.github/` 版 | DOCS_INDEX.md 登记核对（**后续**：`.github/` 版亦已于 2026-09-15 随 CR 构建校验移出公开仓一并删除，见追加区该日条目） |
| ③ | 4 个零证据价值文件（`ps_check.txt`、`ps_count.txt`、`p4567_tmp.log`、`p8_11.log`）直接 `git rm`，不随批隔离 | 进程诊断残留与仅含单行 harness 标题的残文件，无历史证据价值（M3.3）；git 历史可溯 | 无 | — |

### 初值说明

- 本日志建立于 2026-09-12；上述 12 项核对发现与 3 项处置为机制建立时的一次性补记，此后所有偏离按时间顺序追加于下方。
- STATUS.md 初值快照为 v0.5.0（commit `5fac8bd`）；表中 ✅ 项的 v0.5.0 归档证据由整改阶段 2 重跑后落盘，届时同批更新证据列。
- 机制文档（MECHANISM.md / STATUS.md / DISCLOSURE_LOG.md / verification/README.md 重写稿）以未提交状态起草，定稿后由仓库维护者提交。
- 机制建立决策记录见 [docs/adr/ADR-0001-验证状态单一真相源与诚实披露机制.md](../docs/adr/ADR-0001-验证状态单一真相源与诚实披露机制.md)。

## 追加区

（此后按时间顺序追加，格式：日期 + 事实 / 依据 / 影响 / 修正去向）

### 2026-09-24：evorule-discipline crate 抽取、排列等价 proof 入库与证据残留隔离

- **事实**：① 纪律门禁判定引擎自 `evorule-cli/src/commands/discipline_gate.rs` 抽出为独立 crate `evorule-discipline`（机制侧遍历/事实标注 + 内核求值，判定 SSOT 与 evorule-server 共享），workspace 接线、cli 切换依赖，cli 侧逻辑文件减 552 行；② `evorule-tcb/tests/kani/kani_proofs.rs` 新增排列等价 proof `verify_exec_enforce_permutation_equivalence`（上一条目④的落地——「enforce 判定与规则排列位置无关」新保证的 proof 化），按 B 档登记（STATUS.md 附录 A 对照表/附录 B 分档清单、kani.yml b-tier batch 3 三处同步，proof 总数 45→46、tcb 34→35、B 档 20→21，对外数字 README/ROADMAP/verification README 同步）；③ 两仓 `verification/evidence/kani/` 下 19 个 `.FAIL.raw` 残留（2026-09-24 本地 Kani 批验尝试产物，未入库）隔离至各自 `_invalidated/`（TCB 批次 9 / reactor 批次 4）——内容为 WSL rustup 工具链安装故障（os error 39 Directory not empty）原始输出，**验证未实际执行**，不构成证据（M3.2 不满足），涉及 A 档/CI proofs 均有现行有效替代证据（批次 8 于 `a3d728f` 复跑 18/18 PASS）。
- **依据**：cargo fmt / clippy -D warnings / test --workspace 全绿（2026-09-24 本地）；`check_status_sync.py` 12 项全 PASS（含 S4 命名、S7 证据时效——A 档既有 proof 函数本体零变更、S8 附录对齐、S9 对外数字、S11 本条目联动）。
- **影响**：A 档 14 个证据基线 `a3d728f` 有效性不变（既有 harness 函数零变更，新增证明不触及）；B 档 21 个（排列等价 proof 待人工验证，B 档整体「当前不可运行」判定不变）；`evorule-discipline` 0.6.1 以 path 依赖供本机联调，crates.io 发布与 server 侧纯 version 切换待后续批次。
- **修正去向**：本条目即披露记录；`verification/STATUS.md`（附录 A/B、P0-5 行口径）；`README.md`/`ROADMAP.md`/`verification/README.md`（proof 总数）；两仓 `_invalidated/README.md`（隔离批次 9/4）；rustup 环境故障根因（工具链目录残留冲突）留待验证环境专项处理。

### 2026-09-24：约束前置门（BUG-P0-005）执行语义变更的验证影响登记

- **事实**：`evorule-tcb/src/transition.rs` 的 `execute_transition` 发生执行语义变更（commit `0cd1a6b`/`48cacda`，2026-09-23/24）——所有顶层 `enforce` 约束改为**先于任何状态变换/IO 路由**求值（约束前置门，修复 BUG-P0-005：L2 守卫被更早规则的 `IoRequired`「传播即停」静默遮蔽），判定上下文固定为转换前输入状态快照（`exec_state.clone()`），求值顺序仍按列表下标升序，`Halted.rule_index` 下标契约与 `rule_hits` 对外口径（等长/升序）经归并保持；同批新增 `discipline` 模块（规则集形态纪律数据化）与工具链锁定 1.98.1。proof 源码（`evorule-tcb/tests/kani/kani_proofs.rs`）**不在变更集**。CI kani.yml 于 `48cacda` 全量实跑 PASS（TCB+reactor job 全绿，主仓 7 workflow 全绿）。**无既有 PASS 证据失效需隔离**。
- **依据**：diff 实测（A 档 14 个 proof harness——resolve_path 11 个 + JsonValue 3 个——全部不经过 `execute_transition`，与变更文件作用面**零交集**；B 档 enforce 系 3 个 `verify_exec_enforce_{never_panics,halt_semantics,deterministic}` 与 P19 `verify_execute_transition_never_panics`/P21 `verify_react_io_required` 的 harness 符号化执行整个函数体，直接覆盖新增约束门路径）；终止性核验（约束门与主循环**共享同一** `&mut budget`，enforce 在主循环 continue 跳过不重复扣——M6 总预算防线保持）；CI runs（head_sha=`48cacda` kani job success）。
- **影响**：① P0-5 的确定性语义**增强**（enforce 判定不再依赖规则排列位置），B 档主状态不变（仍 ❌ 实测超时），🔵 兜底强化（新增 3 个行为回归测试：遮蔽场景 Halted/IoRequired 两向）；② P0-7/P0-8 终止性语义保持（共享预算），TLA+ 模型（N_MAX=2）无需变更；③ A 档证据基线 `a3d728f` 维持有效（harness 零交集 + CI 实跑旁证），按 M3.4 严格口径的 WSL 复跑落盘列为待办，交项目方裁定是否执行；④ 「enforce 判定与规则排列无关」这一新保证目前仅行为测试覆盖、无 proof——列为 B 档 proof 候选（排列等价性），交遗留清单。
- **修正去向**：本条目即披露记录；`verification/STATUS.md`（P0-3/P0-5/P0-6/P0-7/P0-8 五行状态依据与备注同步）；待办两项（WSL A 档复跑落盘、排列等价性 proof）登记于本条目，交项目方裁定。

### 2026-09-20：快照版本随 0.6.1 发版收口同步（v0.6.0 → v0.6.1）

- **事实**：workspace 版本收口 0.6.1（commit `c7e6266`，2026-09-20），STATUS.md 快照同步 v0.6.0 → v0.6.1。本批为文档版本号收口与历史版本锚去版本化改写（教程/参考/README 中失效的旧版本字面量清零），proof 源码、生产源码、证据库零变更——A 档证据基线 `a3d728f` 有效性不受影响（M3.4：无触及 proof 源码或所验证生产源码的变更）。
- **依据**：`c7e6266` diff 实测（18 文件均为文档/版本号/CI 加固，`verification/kani_proofs.rs` 与 `evorule-tcb/tests/kani/kani_proofs.rs` 不在变更集）；全量验证绿（fmt / clippy / cargo test --workspace / validate-version / validate-release）。
- **影响**：S10 版本对齐三方（Cargo.toml / STATUS.md / MECHANISM.md 头部声明）回到一致；无状态行变更（P0/P1/C6/DEV 各表状态与证据列不动）。
- **修正去向**：本条目即披露记录；`verification/STATUS.md`（快照行）、`verification/MECHANISM.md`（头部版本对齐声明）。

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
- **归档记录（同日）**：正式 PASS 证据已落盘 `P0-11.invariant_cause_queue_sync_PASS_03643aa_20260912_225316`（evorule-reactor/verification/evidence/kani/，基于修复提交 `03643aa`，1.60s PASS，647 断言 0 失败，CI 同参数复跑）；STATUS.md 证据列已补全。

### 2026-09-12：P1-5 `command_does_not_decrease_queue` 超时根因修复，入 CI 闸门

- **事实**：应用与 P0-11 相同的修复（proof 末尾 `std::mem::forget(state)` 跳过 `ReactorState` 的 Drop），P1-5 由超时恢复为可运行：本地实测 0.57s PASS（默认参数）/ 0.60s PASS（`--default-unwind 4`，与 CI 参数一致），入 kani.yml reactor job（for 循环内，带 `--default-unwind 4`）。
- **依据**：与 P0-11 同根因（CBMC 对 VecDeque 堆缓冲区中 JsonValue 按任意变体建模，state 整体 Drop 展开红黑树析构无界 unwind）；`apply_command` 即 `state.push_back`（evorule-reactor/src/pure.rs），proof 路径无其他爆炸点。
- **影响**：P1-5 主状态由 🟡 调整为 ✅（正式 PASS 证据按 M3.4 于修复提交后归档，届时补 STATUS.md 证据列）；kani-reactor job 由 3 个增至 4 个 proof；对外数字口径「当前实跑验证」17→18（根 README 双语 badge 与正文、STATUS.md、reactor KANI.md、Cargo.toml `[package.metadata.kani]`、plan v3 §五 同步更新）。
- **修正去向**：本条目即修正记录；状态见 STATUS.md（P1-5 行与附录 C）。
- **归档记录（同日）**：正式 PASS 证据已落盘 `P1-5.command_does_not_decrease_queue_PASS_03643aa_20260912_225328`（evorule-reactor/verification/evidence/kani/，基于修复提交 `03643aa`，`--default-unwind 4` 下 0.86s PASS）；STATUS.md 证据列已补全。

### 2026-09-13：B 档 proof 攻坚立项与 CR-20260913-003 载体修订（ADR-0002）

- **事实**：B 档 23 个超时 proof 攻坚立项（CR-20260913-004，四梯队结构：可行性门 / 零侵入优化 / Kani 机制层 / 属性分流 / 治理登记，总预算 ≤60 次验证运行 × ≤600s，每阶段设停止准则）。CR-20260913-003（eq/clone 模型化）实施载体由 impl 级 `cfg(kani)` 双实现修订为 proof 层 `#[kani::stub(...)]`：value.rs 两处 impl 级覆写（Clone/PartialEq）回退（该 CR 代码尚未提交，无公开仓历史修正需求）。配套新增：B 档 harness 结构自检断言（Phase 1 硬前置）；cfg(kani) 偏差登记簿（K 系）"爆炸半径"列与补偿合规规则（爆炸半径全调用点映射 / 证据-实现绑定 / 补偿不得引用受该模型影响的 proof）；STATUS.md 属性表"模型偏差"列（Phase 1 随批落地）。
- **依据**：[ADR-0002](../docs/adr/ADR-0002-B档proof模型载体与stub化验证策略.md)（M5.2 建模策略变更情形）：impl 级覆写在 kani 构建下影响全部调用点（含 harness 构造层 `object_from_pairs` 的 `v.clone()`——嵌套复合输入在深度 1 处被清空，proof 实际验证退化输入）；真实 trait impl 从 kani 构建产物中消失，使真实现侧证明补偿不可成立（循环引用）。CR-20260913-001/002/003 rounds 1-12 探针记录（三层根因与乘积爆炸结论）。
- **影响**：① A 档 14 个 proof 的归档证据（`bdfb8d4`）因被验证代码（value.rs）变更而 SHA 绑定失效——Batch 1 于 WSL 重跑 14 个补新 SHA 证据（P0-3/P0-6 证据列同批更新）；② reactor 4 个闸门 proof 在 stub 载体下零影响；③ B 档状态不变（❌），攻坚期间对外口径不变（M6）；④ kani.yml B 档 job 配置不变（不加大 timeout）。
- **修正去向**：本条目即修正记录；CR-20260913-003 修订记录见 [evorule-tcb/CHANGE_REQUEST.md](../../evorule-tcb/CHANGE_REQUEST.md)（原文保留，M7）；STATUS.md 维护区登记攻坚计划。
- **归档记录（同日）**：Batch 1 核心提交 `1c6ad84`（value.rs 载体回退 + 编译面配套适配 + 探针迁出 + 四文档）后，于 WSL（Kani 0.67.0 + nightly-2025-11-21）重跑 A 档 14 个：14/14 PASS（单 proof 0.3~4.2s），M3.1 证据对 28 个文件落盘 `P0-3/P0-6.<harness>_PASS_1c6ad84_20260913_*`（evorule-tcb/verification/evidence/kani/）；旧 `bdfb8d4` 14 对证据按 M3.4 `git mv` 至 `_invalidated/`（批次 2，作废判定与替代证据指引见该目录 README）；STATUS.md P0-3/P0-6 证据列、快照注记与维护区登记同批更新。提交前以暂存树（= 提交内容）预演 kani.yml 双 job：TCB A 档 14/14 + reactor 4/4 PASS。

### 2026-09-13：Phase 0（T0-1~T0-7）可行性验证完成，方案按实测收缩

- **事实**：CR-20260913-004 Phase 0 七项可行性验证全部完成（4 次有效运行 + 3 项代码审计，机时 6/≈7 次预算，机器时间 ≈20 分钟）。结论：① T0-1 求解器选项存在（`--solver`，默认 CaDiCaL），kissat 对 eq proof 单点试跑 600s 超时无改善——Tier 1.2 裁撤；② T0-2 `-Z stubbing` 最小 stub proof 实跑 PASS——Tier 2.1 stub 默认路径确认；③ T0-3 `requires`/`ensures`/`proof_for_contract` 实跑 PASS，`stub_verified` 编译期报错（`Failed to find contract closure`）——组合验证改「stub 自带 contract + `proof_for_contract` 两步独立验证」；④ T0-4 审计确认 set/add/sub 算术路径全 `checked_add`/`checked_sub` + `IntegerOverflow` 传播——P13 可走 checked-ops 门禁卸载；⑤ T0-5 审计确认 evaluate_eq 子树内 eq 结果与 clone 内容仅流入 never-panic 的 `PartialEq` 与分支控制流——clone stub 固定值分支成立（限该子树）；⑥ T0-6 单点对比（`"payload.x"`→`"p.x"`、`unwind(24)`→`unwind(6)`）仍 600s 超时——Tier 1.1 单独无效；⑦ T0-7 复审确认 F1 中毒路径（impl 级 cfg Clone 覆写经 `object_from_pairs` 清空嵌套复合值）已随 CR-20260913-003 修订消除，B 档重跑前置门解除。
- **依据**：实测运行记录（WSL Kani 0.67.0 + nightly-2025-11-21，与 kani.yml CI 同配置；E1/E4 各 600s 上限超时，E2/E3 亚秒级）与代码审计（executor.rs 算术路径、domain.rs/executor.rs 数据流、value.rs/model.rs 构造面盘点）。实测要点：eq 族 harness 输入完全具体（无符号值）仍 600s 不收敛，证实成本在结构层建模（String/Cow/Vec/分配器），为三层根因模型外的残余因素。
- **影响**：① 方案收缩：Tier 1.2 裁撤、Tier 1.1 降级为配套动作（eq 族按 kill criteria 以首个数据点提前路由 Phase 2 stub 试点）、Tier 2.3 组合验证两步化；② Tier 2.1/2.2/Tier 3 路线确认可行；③ Phase 1 计划不变（W3-1 结构自检断言先行）；④ B 档 23 个 proof 状态不变（❌，M2/M6——转档待实测证据）。
- **修正去向**：CR-20260913-004 §3.5 测试计划勾选与 §3.7 结论追记（[evorule-tcb/CHANGE_REQUEST.md](../../evorule-tcb/CHANGE_REQUEST.md)）；可行性矩阵详细实测记录按信息分级留档于项目内部工作区（M10）；STATUS.md 维护区同批注记。

### 2026-09-14：W3-1 结构自检断言落地，A 档证据随 proof 源码变更重置（`1b340e5`）

- **事实**：B 档攻坚 Phase 1 首项 W3-1 完成（CR-20260913-004 §3.8）——`tests/kani/kani_proofs.rs` 新增 7 个结构自检助手并为 23 个 B 档 harness 全部接线（13 处构造根接线修复）。canary 本地验证：正向 3 组原语 PASS（c1 76.9s / c3 32.5s / c2b 0.55s，覆盖 5/7 助手），反向退化构造 13.9s 于预期断言点响亮失败（假通过防线成立）；`shape_full_state`/`shape_concrete_exec_state` 两复合哨兵因构造墙无法独立实跑（c4a 纯构造零断言 120s 不收敛等实证），由已验证原语复合支撑、随 W3-3/W3-4 闭环。提交 `1b340e5` 后于 WSL（Kani 0.67.0 + nightly-2025-11-21）同协议重跑 A 档 14 proof：14/14 PASS。
- **依据**：canary 实测记录（≈17 次运行 ≈60 分钟，两层根因：unwind(8)<memcmp 深度致 unwinding 断言假失败毒化公式 919/920 undetermined；构造墙——全具体构造随复杂度非线性恶化，c2 300s 不收敛 vs c2b 0.55s 最锐对照）；提交前预演（暂存树 14/14 PASS）；新证据 `P0-3/P0-6.<harness>_PASS_1b340e5_20260914_*`（evorule-tcb/verification/evidence/kani/）。
- **影响**：① 旧 `1c6ad84` 14 对证据按 M3.4 失效，`git mv` 隔离 `_invalidated/` 批次 3；② STATUS.md 快照注记与 P0-3/P0-6 证据列同批更新；③ W3-2 配套要求更新：B 档 harness unwind 必须 > 形状断言最长字符串 memcmp 深度（字节数+2）；④ 构造墙发现移交 W3-3/W3-4：P9/P10（构造 `concrete_exec_state`）Phase 1 直跑将撞同一构造墙，须 W3-3 owned 迁移或 W4-1 stub 路线先解除；⑤ B 档 23 个 proof 状态不变（❌，M2/M6）。
- **修正去向**：本条目即修正记录；执行详情见 CR-20260913-004 §3.8（[evorule-tcb/CHANGE_REQUEST.md](../../evorule-tcb/CHANGE_REQUEST.md)）；隔离批次详情见 `evorule-tcb/verification/evidence/kani/_invalidated/README.md` 批次 3；STATUS.md（快照/P0-3/P0-6/维护区）。

### 2026-09-14：W3-2 unwind 静态盘点完成，16 项校准登记为 W3-4 前置配套（零代码变更）

- **事实**：B 档攻坚 W3-2 完成（CR-20260913-004 §3.9）——23 个 B 档 harness memcmp 成功路径深度全量静态审计（3 次脚本迭代，0 次求解器运行）：合规 7 / 不合规 16；16 项 unwind 校准值列表登记为 W3-4 前置配套。A 档 14 个逐个判读为实证豁免（unwind 4 小于 memcmp 路径最长 6，但 4 字节短字符串内经无字面常量路径执行、无字面常量匹配）。STATUS.md 维护区同批登记。
- **依据**：全量审计脚本产出与逐项判读记录；CR-20260913-004 §3.9 校准表（16 项建议 unwind 值）。
- **影响**：① B 档 23 个 proof 状态不变（❌）；② W3-4 重跑以 16 项校准值起步、以 unwinding assertions 反馈逐项收敛；③ 无 proof 源码变更，A 档证据 SHA 绑定不受影响。
- **修正去向**：本条目即修正记录；执行详情见 CR-20260913-004 §3.9。

### 2026-09-14：W3-3 owned 构造迁移落地，A 档证据随 proof 源码变更再重置（`90b77aa`）

- **事实**：B 档攻坚 W3-3 完成（CR-20260913-004 §3.10，Tier 5.3 先行项，G2 构造层与 Clone 解绑）——`tests/kani/model.rs` obj() 由引用对+内部深克隆改为 `object_from_pairs_owned`（move 语义零深克隆），`tests/kani/kani_proofs.rs` 23 个 B 档 harness 构造调用点适配，旧构造形态零残留（96 处 owned 落位）。提交 `90b77aa` 后于 WSL（Kani 0.67.0 + nightly-2025-11-21）同协议重跑 A 档 14 proof：14/14 PASS。
- **依据**：grep 实证（`obj(&[` / `object_from_pairs(&[` 0 命中）；编译验收 `verify_partial_eq_never_panics` 迁移后实跑 1.0s PASS（534 断言 0 失败）；新证据 `P0-3/P0-6.<harness>_PASS_90b77aa_20260914_*`（evorule-tcb/verification/evidence/kani/）。
- **影响**：① 旧 `1b340e5` 14 对证据按 M3.4 失效，`git mv` 隔离 `_invalidated/` 批次 4；② STATUS.md 快照注记与 P0-3/P0-6 证据列同批更新；③ B 档 23 个 proof 状态不变（❌）；④ W3-4 前置就绪：16 项 unwind 校准表（W3-2）+ owned 构造（W3-3）两要素齐备。
- **修正去向**：本条目即修正记录；执行详情见 CR-20260913-004 §3.10（[evorule-tcb/CHANGE_REQUEST.md](../../evorule-tcb/CHANGE_REQUEST.md)）；隔离批次详情见 `evorule-tcb/verification/evidence/kani/_invalidated/README.md` 批次 4；STATUS.md（快照/P0-3/P0-6/维护区）。

### 2026-09-14：W3-4 精确 unwind 校准落地，eq 族首数据点超时路由 Phase 2（`627330a`）

- **事实**：W3-2 校准表按"一次改完 proof 源码"纪律落地（instruction 24→32 / all 4→16 / deterministic 补 16 / domain_depth 12→16），同批清理 W3-3 临时 canary c2-clone/c2-owned（"迁移验证后删除，不入库"承诺收口）。eq 族首数据点 `verify_evaluate_domain_eq_never_panics`（unwind 24 合规 + owned 构造）**600s 超时**——kill criteria 触发，eq 族路由 Phase 2 stub 试点。提交 `627330a` 后于 WSL 重跑 A 档 14 proof：14/14 PASS。
- **依据**：eq 首数据点实测（timeout 600s，`Checking harness...` 后无验证结论输出）；W3-2 §3.9 校准表；新证据 `P0-3/P0-6.<harness>_PASS_627330a_20260914_*`（evorule-tcb/verification/evidence/kani/）。
- **影响**：① 旧 `90b77aa` 14 对证据按 M3.4 失效，`git mv` 隔离 `_invalidated/` 批次 5；② B 档 23 个 proof 状态不变（❌）；③ eq 族路由结论强化构造墙主因判断（String/KaniMap 建模开销；owned 迁移仅解 clone 分量，未解构造墙）；④ W3-4 机时 +1 次（eq），其余 8 个 P8 系首轮实测合并至历史批次 Step 10 全量重跑（post-69 基线，机时纪律，§3.11）。
- **修正去向**：本条目即修正记录；执行详情见 CR-20260913-004 §3.11；隔离批次详情见 `evorule-tcb/verification/evidence/kani/_invalidated/README.md` 批次 5；STATUS.md（快照/P0-3/P0-6/维护区）。

### 2026-09-14：规则清理（collect/merge 元指令退役），A 档证据重置 + reactor 4 proof 复跑（`25c0cc0`）

- **事实**：规则清理批次实施完成（CR-20260914-001，v0.6.0 破坏性变更）——TCB 删除 `exec_collect`/`exec_merge`/`substitute_template` 及指令分发分支，`META_INSTRUCTION_TYPES` 收窄 5 种；proof P15/P16/P17 删除（总数 37→34，B 档 23→20），`tests/kani/model.rs` any_instruction %6→%4，P12/P19 allowed 集合收窄（提交 `10c743d`；`25c0cc0` 仅差 wasm-demo test.js）。 governance/cli/server/system-rules/console/console-cloud/evo-agent 宪法/reactor 全链同步收窄，跨仓处置随主仓 CR。提交后于 WSL（Kani 0.67.0 + nightly-2025-11-21）重跑：TCB A 档 14/14 PASS（单 proof 0.3~4.0s）+ reactor 4 个 CI proof 4/4 PASS，合计 18/18。
- **依据**：新证据 `P0-3/P0-6.<harness>_PASS_25c0cc0_20260914_190211_*`（evorule-tcb/verification/evidence/kani/）与 `P0-11/P1-3/P1-5/P1-6.*.PASS_25c0cc0_20260914_190211_*`（evorule-reactor/verification/evidence/kani/）；CHANGELOG 0.6.0 破坏性变更条目；STATUS.md 附录 A/B 删号与计数更新。
- **影响**：① 旧 `627330a` 14 对证据按 M3.4 失效，`git mv` 隔离 `_invalidated/` 批次 6；② reactor 旧 4 对（`03643aa`/`bdfb8d4` 锚定，proof 源码未变更但被验证依赖 TCB 生产代码变更，谨慎起见复跑替代）隔离至该仓同级 `_invalidated/`；③ B 档 20 个 proof 状态不变（❌；eq 族已路由 Phase 2 stub 试点，CR-20260913-004 §3.11）；④历史批次 Step 10 Kani 重跑项收口。
- **修正去向**：本条目即修正记录；执行详情见 CR-20260914-001（[evorule-tcb/CHANGE_REQUEST.md](../../evorule-tcb/CHANGE_REQUEST.md)）；隔离批次详情见 `evorule-tcb/verification/evidence/kani/_invalidated/README.md` 批次 6；STATUS.md（快照/P0-3/P0-6/附录 A/B/维护区）。

### 2026-09-15：历史遗留项CI 门禁整改——证据命名修正 + reactor 隔离区 README 补建 + 文档对齐

- **事实**：check_status_sync.py 门禁 7 项 FAIL（S2/S3/S4/S6/S8/S9/S10）整改完成：① 36 份证据文件名 `.PASS_` 笔误批量 `git mv` 修正为 `_PASS_`（TCB A 档 28 + reactor 8，均属 2026-09-14 `25c0cc0` 批次，内容零变更，STATUS.md 证据列本就按正确命名声明）；② `evorule-reactor/verification/evidence/kani/_invalidated/README.md` 补建（批次 1 =历史批次 Step 10 隔离的 4 对 `03643aa`/`bdfb8d4` 证据，原提交 `f013989` 漏建 README）；③ STATUS.md 附录 B A 档标题行批次锚 `25c0cc0` 去反引号（此前被 S8 解析为第 15 个 proof 名，致「声明 14 / 清单 15」）；④ STATUS.md P0-11 证据列更新为 `25c0cc0` 复跑证据（原 `03643aa` 引用已隔离于 _invalidated/，S2/S3 随之闭环）；⑤ MECHANISM.md 版本对齐声明 v0.5.0→v0.6.0（M4，历史批次发布漏更）；⑥ ROADMAP.md 与 verification/README.md B 档计数 23→20（历史批次 P15/P16/P17 退役漏更）。
- **依据**：check_status_sync.py 实测输出（修复前 4 PASS / 7 FAIL，修复后 11 规则全 PASS）；文件系统实测（36 份笔误命名、reactor 隔离区缺 README）；Cargo.toml workspace version = 0.6.0。
- **影响**：S1–S11 全绿；对外数字（45 total / 18 verified / TCB 34 / A 档 14 / B 档 20）与 STATUS.md 附录推导值全对齐；P0-11 最新证据（2026-09-14 PASS）与主状态 ✅ 闭环；reactor 隔离区满足 M3.5。
- **修正去向**：本条目即修正记录；证据盘面与 STATUS.md / MECHANISM.md / ROADMAP.md / verification/README.md / `evorule-reactor/verification/evidence/kani/_invalidated/README.md`。

### 2026-09-16：CR 构建校验移出公开仓，CR 模板双份清零（TCB-2026-29，既定裁定）

- **事实**：L1b 的 CHANGE_REQUEST.md 构建校验经裁定移出公开仓（该检查属提交前自查纪律，非防伪造审查机制），`EVORULE_SKIP_CR_GATE` 环境变量随之删除；策略层反模式检测改为**无阀常开**。首条处置记录② 保留的 `.github/CHANGE_REQUEST_TEMPLATE.md`（267 行）随本次整改删除（`8a8f04c`），根目录版已于 2026-09-12 删除——两个模板至此**全部离线**，CR 自查职责由维护者本地 git pre-commit hook 承接（hook 不随仓库/发布公开）。`CHANGE_REQUEST.md` 登记文件与登记纪律本身不变。
- **依据**：`8a8f04c` diff（`.github/CHANGE_REQUEST_TEMPLATE.md` −267 行）；四仓 build.rs 实跑输出（旧「变更治理门禁 PASSED」消失，策略层检测无阀常开 PASSED）；`CHANGELOG.md` [Unreleased] TCB-2026-29 条目。
- **影响**：① 首条处置记录② 的「仅保留 `.github/` 版」结论作废，该行已就地附现状注记；② 全仓源码/文档中 `CHANGE_REQUEST_TEMPLATE` 仅剩 `CHANGELOG.md` 0.4.3 历史条目（按「历史条目保留 + 现状注记」原则处理）与本日志两处登记性出现（首条②、本条目）；③ 公开仓不再存在任何 CR 模板载体，「伪门禁」质疑面消除。
- **修正去向**：本条目即修正记录；`CHANGELOG.md` [Unreleased]（TCB-2026-29）、本日志首条处置记录② 现状注记、`GATE_REFERENCE.md` §一/§二、`GOVERNANCE.md` §二/§五、四仓 SPEC·README。

### 2026-09-17：保证声明（ASSURANCE.md）成文，确立「规范 / 状态分离」并新增保证达成状态与偏离登记

- **事实**：`verification/ASSURANCE.md` 首次成文（v1.0），并按**规范与状态分离**原则定稿：该文件仅承载规范性条款——顶层声明 C1–C7、保证等级 AL0–AL4 与判定规则 R1–R4、目标保证轮廓、共享责任模型、假设登记册 H1–H6、不保证事项 NC-1–NC-16、工具信任基登记、证据要求与失效规则（§8）、效力分层与修订程序（§0.3–§0.6）。**一切项目现状表述不写入该文件**，改由 `STATUS.md` 新增的「三、保证声明达成状态与偏离登记」承载：① 各条声明的当前达成等级（由 §一/§二 属性状态按 AL 定义与 R1–R4 聚合的**派生视图**，不引入新事实）；② 偏离登记 DEV-1–DEV-7（含受影响条款、判定依据、处置方向）。`verification/README.md` §4.4 资产登记与场景导航同步改写，明确二者的强制分工与单向对齐方向。
- **依据**：ASSURANCE.md §0.4（规范与状态分离及三条禁令）、§0.5（条款准入：时效不变量测试 TIT-1/2/3 + 禁止写入清单）、§0.6（修订分类 A/B/C 与修订理由穷尽列举，明确「现状变化不构成修订理由」）、§4.3（偏离登记义务）。
  自查与实测：① `G-[1-7]` 在 ASSURANCE.md **零命中**（缺口类编号已彻底移出规范文件）；② M9 内部知识库零泄露、M11 个人身份零命中；③ M6.3 比较级词仅出现在禁令条款（§0.7、NC-15）与「不超过」类非比较语义处；④ 全部相对链接目标存在性核验通过（`../ROADMAP.md`、`DISCLOSURE_LOG.md`、`MECHANISM.md`、`plan/`、`STATUS.md`）；⑤ `check_status_sync.py` 本条目落盘前实测 10 PASS / 1 FAIL（S11 披露留痕联动——即本条目的触发原因），落盘后复跑。
- **影响**：① `STATUS.md` 的状态权威范围扩展至「保证声明达成等级 + 偏离登记」，新增 §三；② `ASSURANCE.md` 成为纯规范文件，其修订不再受项目进度驱动（项目进展、偏离消除、证据补足**只改 STATUS.md**）；③ 规范条款的准入新增机器可检约束（TIT + 禁止写入清单）；④ 现有对外数字与 §一/§二 属性状态**未经本次改动**；⑤ 偏离登记义务生效：实现与规范条款的偏离须附依据与处置方向，「长期存在且不处置」明确不被允许。
- **修正去向**：本条目即修正记录；`verification/ASSURANCE.md`（v1.0）、`verification/STATUS.md` §三、`verification/README.md` §4.4 与场景导航。

### 2026-09-17：C6 守卫强制运行时装配批次——STATUS 初稿失实更正 + C6 属性状态落档 + DEV-4/DEV-5 关闭

- **事实**：C6 运行时装配批次完成（事后补录立项，见内部工作区记录，工作区不随仓公开）：① server 三处 `IoSubscriber` 注入 `PermissionGate`（主干全局路径 + 两条 per-session 路径）+ 启动种子 `default-human-allow-io` + `GUARD_ASSEMBLED` 装配信号（`/api/health` `guard_assembled` 字段与启动日志）；② governance 新增穷尽决策表测试 `exhaustive_decision_table_default_deny`（C6.1/C6.3，枚举 CallerRole×io_type 全组合）；③ 验证：`cargo check` RC=0、双仓全量测试 0 失败（governance lib 156 含新测试、server lib 362 等 58 目标）、wasm-host 4 个 e2e 脚本回归全 PASS（v6_offline / t5_concurrency / t6_sensitive_guard / v79_auto_discover，真实起进程，敏感守卫与直调语义零回归）。**同批状态更正**：STATUS.md 本批初稿（工作树版、未曾提交）存在三处失实——(a) 归因于一个不存在的提交 SHA；(b) C6.1/C6.3/C6.4 主状态使用非五档词汇「in-progress」（M2 五档之外的自造状态）；(c) DEV-5 关闭条件写成既成事实（代码落地时验证尚未执行）。经专项核查发现后于本批更正为最终事实。
- **依据**：双仓全量测试实测输出；4 个 e2e 脚本实测输出（全 PASS）；git 状态核查（初稿未经提交，三处注入均为本批工作树新增）；STATUS.md 维护区 2026-09-17 条目（含更正声明）。
- **影响**：① STATUS C6 表：C6.1/C6.3 主状态 🔵→**✅**（本地实跑证据落定 + CI `test` job 常驻），C6.4 保持 🔵（编译级验证，运行时观测待 e2e 断言补强）；② §3.1 C6 行保持 **AL2（部分）**（判定依据更新为实跑证据），关联偏离收窄为 DEV-6；③ **DEV-4 关闭移出**（C6 属性条目已建于 §一/§二 之间的 C6 表）、**DEV-5 关闭移出**（装配信号已实施并验证）——§3.2 现存 DEV-1/2/3/6/7；④ 行为变更登记：装配后 LLM/Unknown 调用者 I/O 直调由 fail-open 转 fail-closed（有意变更，既有 e2e 场景实证未受影响）。
- **修正去向**：本条目即修正记录；`verification/STATUS.md`（C6 表、§3.1 C6 行、§3.2 偏离登记、维护区 2026-09-17 条目含更正声明）。

### 2026-09-17：C6.5 链路测试实跑验证 + snapshot_at 版本围栏缺陷修复 + DEV-6 差分证据部分归档（T1/T2/T5/T6）

- **事实**：① C6.5 账本链路测试组 5 项实跑 PASS（`cargo test -p evorule-governance permission::table` 5/5；lib 全量 161 全绿）；链路 2/3 回归暴露**产品缺陷**：`PermissionTable::snapshot_at` 版本围栏 off-by-one——账本 history 记录 `version_before` 而 append 返回写入后版本，原 `> v_shared` 剔除会把 `version_before == v_shared` 的事实错误纳入历史时点投影；已修为 `>=`（table.rs 同批，附回归注释）。当前时点判定（`v_trigger = version()`）语义不受影响，受影响面为历史时点重建（审计回放/决策可追溯）。② DEV-6 差分 harness 落地：`evorule-tcb/tests/carrier_diff.rs`（T1/T2）与 `evorule-reactor/tests/carrier_diff.rs`（T5/T6）9 测全 PASS，证据归档 `verification/carrier-identity/evidence/DIFF-T1_T2_*` 与 `DIFF-T5_T6_*`（载体 `d9ceb9c`）；T3/T4 须触达 reactor 私有内部，待 src 内 `#[cfg(test)]` 落位。③ 载体同一性两文档修订：去工作环境表述、修正 unwind 计数 22→23（实测 `kani_proofs.rs`）、差分运行命令修正（stable `cargo test` 下 `cfg(not(kani))` 恒真，无需 cfg 旗标）。
- **依据**：实测测试输出（governance 5/5 + 161 全绿；tcb 4/4 + reactor 5/5）；git 事实核查（d9ceb9c 载体、governance 零 `cfg(kani)`）；`facts_by_path_prefix`/`append` 源码链路核读。
- **影响**：① STATUS：C6.5 ⏳→**✅**（AL1），C6 表五子命题中四项 ✅、C6.4 🔵；§3.1 C6 行判读依据同步（C6.2 证据产出矛盾消除）；② snapshot_at 缺陷修复属产品代码变更，回归由链路 2/3 常驻守护；③ DEV-6 维持「未关闭」（T3/T4 待补齐 + 保真损失项边界化）；④ C6.5 尾部条目自「待验证」状态收口为已验证。
- **修正去向**：本条目即披露记录；`verification/STATUS.md`（C6.5 行、§3.1 C6 行、维护区同日条目）、`evorule-governance/src/permission/table.rs`（围栏修复）、`verification/carrier-identity/`（两文档修订 + evidence/ 两份归档）。

### 2026-09-17：C6.4 装配可观测 e2e 运行时断言——真实起进程观测 guard_assembled==true（P0 收尾②闭环）

- **事实**：C6.4 运行时断言批次完成（形式化验证 roadmap P0 收尾②，收尾① C6.2/C6.5 登记批次已于同日早批闭环）：新增 `verify_c64_guard_assembled.py`（evorule-server 仓 plugins/wasm-host/tests/，与既有 wasm-host e2e 回归族同 harness 模式），真实起 server 进程（与 HEAD `9e6f574` 同源二进制）两轮——首启 + 换全新数据目录重启——逐轮断言 `GET /api/health` `guard_assembled` 字段存在、类型 bool、值 == true，且 server.log 含启动装配日志「入口守卫（PermissionGate）已装配」（信号与实际装配动作相关）；两轮 **8/8 PASS**（2026-09-17 实测）。负向 `false` 分支在当前构建不可达（装配为无条件 fail-closed），负向信号存在性/类型由编译级验证承载（脚本 docstring 留痕）。STATUS 同步：C6.4 🔵→**✅**（AL1）——C6 表五子命题全部 ✅；§3.1 C6 行维持 **AL2（部分）**（AL4 仍受 DEV-6 阻断）。
- **依据**：脚本实测输出（两轮 8/8 PASS）；server 二进制经 `cargo build` 与仓 HEAD 同源后实跑；`main.rs` 装配调用链（无条件 `mark_guard_assembled`）源码核读。
- **影响**：① STATUS C6 表五子命题全部 ✅；§3.1 C6 行判读依据更新、达成等级不变；② 形式化验证 roadmap P0 两项收尾全部闭环，P0 整体完成；③ 无行为变更（纯测试脚本新增 + 状态文档，产品代码零改动）。
- **修正去向**：本条目即披露记录；`verification/STATUS.md`（C6.4 行、§3.1 C6 行、维护区同日条目）；evorule-server 仓 `plugins/wasm-host/tests/verify_c64_guard_assembled.py`（新增）。

### 2026-09-18：仓库提交历史整理（公开历史 commit message 规范化清洗）与 A 档证据基线重置

- **事实**：① 主仓执行仓库提交历史整理：全链 commit 重写（公开历史 commit message 规范化清洗）+ 版本 tag 重打，新基线 `a3d728f`，树内容与整理前末端 `34c841d` 完全一致（`git diff 34c841d a3d728f` 为空）；整理前链（旧历史 hash）：`bdfb8d4` → `1c6ad84` → `1b340e5` → `90b77aa` → `627330a` → `25c0cc0` → `34c841d`。② A 档 Kani 证据基线随历史整理重置：旧证据 SHA 锚定全部失效（旧 hash 不在新历史），按 M3.4 复跑替代（拒绝修改既有证据内容）——18 个 kani proof 于 `a3d728f` 复跑全 PASS（TCB 14：`20260918_105558`；reactor 4：`20260918_110527`；WSL Kani 0.67.0），旧证据 18 对（`34c841d` 锚定）按 M3.5 移入各仓 `_invalidated/`（TCB README 批次 8 / reactor README 批次 3）。③ 复跑前置修复：reactor `src/invariants.rs` 测试辅助 `set_io_result` 做了 cfg 兼容适配（原 `BTreeMap::entry()` 在 kani cfg 下 Object 的替身实现上无此 API，E0599）——属 `#[cfg(test)]` 测试辅助，不触及 proof 源码（`verification/kani_proofs.rs`），不影响证据锚定。④ 首轮复跑暴露证据生成脚本两处缺陷（stdout 配对文件未落盘、文件名硬编码 `_PASS_`）：修复后由既有 `.log` 内嵌全量 stdout 逐字节重建 18 个 `.stdout.txt` 配对；4 个误名文件（文件名 PASS、内容为编译失败输出，零证据价值）废弃删除。
- **依据**：`git diff 34c841d a3d728f` 树零变更实测；18 proof 复跑实测输出（全 PASS）；check_status_sync.py S7 追踪范围源码核读（仅 `evorule-reactor/verification/kani_proofs.rs` 与 `evorule-tcb/tests/kani/kani_proofs.rs`）；配对重建前后文件计数与首尾行抽检（reconstructed 18 / moved 18，主目录旧锚残留 0）。
- **影响**：① STATUS.md 快照基线更新为 `a3d728f`，P0-3/P0-6/P0-11/P1-3/P1-5/P1-6 六行证据列与状态依据同步（旧 `34c841d` 版指针全部改指隔离批次）；② B 档 20 个不变量与差分证据不在本轮重置范围（无本批变更触及）；③ 门禁 12 规则复跑全绿；④ 旧证据全部可溯（M3.5 `git log --follow`）。
- **修正去向**：本条目即披露记录；`verification/STATUS.md`（快照、六行证据列/状态依据、维护区同日条目）；`evorule-tcb/verification/evidence/kani/_invalidated/README.md` 批次 8、`evorule-reactor/verification/evidence/kani/_invalidated/README.md` 批次 3；新证据 18 对（`*_PASS_a3d728f_20260918_*`）。