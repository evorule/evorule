# REPO_REGISTRY — EvoRule 生态仓库注册表

> **权威登记表**：EvoRule 生态全部仓库的双端（Gitee 主仓 / GitHub 镜像）定位、分支、License 与 CI 现状。
> **维护铁律**：新建 / 迁移 / 改名 / 变更默认分支或可见性，**必须同步更新本表**；本表与现状不一致即视为债务。
> 最后更新：2026-09-16（P0/P1/P2 整改后全量核录）

## 命名空间速览

| 命名空间 | 平台 | 性质 |
|---|---|---|
| `gitee.com/evorule/*` | Gitee 组织 evorule | 权威主仓（公开） |
| `gitee.com/evorulelab/*` | Gitee 用户 evorulelab | 私有仓（仅 `evorule-agent`，暂不公开） |
| `github.com/evorule/*` | GitHub 组织 evorule | 镜像 / 站点 / org 健康文件（公开，除 agent 镜像为私有） |

## 主表

| 仓名 | 角色 | Gitee URL | GitHub URL | 可见性 | 默认分支 | License | GitHub CI | Gitee Go | mirror.yml | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|
| evorule | 核心引擎（TCB 唯一版本源） | gitee.com/evorule/evorule | github.com/evorule/evorule | 公开 | main | AGPL-3.0-or-later（双许可） | ci,cla,clippy,codeql,differential,fuzz,kani,mirror,mutants,release,scorecard,tla | ✅ | ✅ 参数化 | 生态唯一版本源；工具链 TCB-2026-48（rust 1.97.1） |
| evorule-server | 自托管服务端 | gitee.com/evorule/evorule-server | github.com/evorule/evorule-server | 公开 | main | AGPL-3.0-or-later | ci,cla,mirror,release | ✅ | ✅ | Gitee CI 已对齐 1.97.1-slim（2026-09-16） |
| evorule-rule | 业务规则集 | gitee.com/evorule/evorule-rule | github.com/evorule/evorule-rule | 公开 | main | AGPL-3.0-or-later | ci,cla,mirror,release | ✅ | ✅ | 跨仓 checkout 已改 Gitee 权威源（2026-09-16） |
| evorule-system-rules | 系统规则集 | gitee.com/evorule/evorule-system-rules | github.com/evorule/evorule-system-rules | 公开 | main | AGPL-3.0-or-later | cla,mirror | — | ✅ | 无独立构建 CI（cla+mirror） |
| evorule-console-cloud | 云控制台 | gitee.com/evorule/evorule-console-cloud | github.com/evorule/evorule-console-cloud | 公开 | main | AGPL-3.0-or-later | ci,cla,deploy-demo,mirror,release | ✅ | ✅ | Gitee Pages 平台已停服（2026-09-15） |
| evorule-console | 控制台前端 | gitee.com/evorule/evorule-console | github.com/evorule/evorule-console | 公开 | main | AGPL-3.0-or-later | mirror | — | ✅ | **无独立 CI（T4 缺口，待补）** |
| evorule-hash | 哈希链 | gitee.com/evorule/evorule-hash | github.com/evorule/evorule-hash | 公开 | main | AGPL-3.0-or-later | ci,cla,mirror,release | ✅ | ✅ | — |
| evorule-bundle | 规则包 | gitee.com/evorule/evorule-bundle | github.com/evorule/evorule-bundle | 公开 | main | AGPL-3.0-or-later | ci,cla,mirror,release | ✅ | ✅ | — |
| evorule-sdk | 多语言 SDK | gitee.com/evorule/evorule-sdk | github.com/evorule/evorule-sdk | 公开 | main | **Apache-2.0（设计选择，非漂移）** | ci,mirror | ✅ | ✅ | License 政策见主仓 CONTRIBUTING |
| evorule-dsh-skill | 安装器 Skill | gitee.com/evorule/evorule-dsh-skill | github.com/evorule/evorule-dsh-skill | 公开 | main | AGPL-3.0-or-later | mirror | — | ✅ | 无独立 CI（T4 缺口，待补）；安装指引已改 Gitee 主仓 |
| evo-agent | 应用层编排（可信 AI 工作站） | gitee.com/evorule/evo-agent | github.com/evorule/evo-agent | 公开 | **main**（2026-09-16 由 master 迁移） | AGPL-3.0-or-later | ci,cla,mirror | ✅ | ✅（2026-09-16 补齐） | 与 evorule-agent **严格区分**（不同设计/功能） |
| evorule-agent | 大脑主控 agent 运行时（技术预览） | **gitee.com/evorulelab/evorule-agent（私有）** | github.com/evorule/evorule-agent（私有镜像） | 私有 | main | AGPL-3.0-or-later | ci,cla,mirror | — | ⏸ 豁免（org secret 不覆盖私有仓，公开时启用） | canonical 为私有路径；未来公开时一次性双仓推送 |
| evorule-application | 大众版应用层（等保2.0 门禁演示） | **gitee.com/evorule/evorule-application（私有）** | —（无 GitHub） | 私有 | main | AGPL-3.0-or-later | — | — | N/A | 2026-09-16 补录：本地 remote 由旧路径 evo-rule-lab 修正为 SSH canonical；无镜像，仅备份链覆盖 |
| rpsm | 实时物理仿真原型（内核纯净+PLA 审计验证，Rust） | **gitee.com/evorule/rpsm（私有）** | —（无 GitHub） | 私有 | **main**（2026-09-16 由 master 迁移） | AGPL-3.0-or-later | — | — | N/A | 2026-09-16 补录：首次克隆本地；无镜像，仅备份链覆盖 |
| evorule.github.io | GitHub Pages 站点 | —（无 Gitee） | github.com/evorule/evorule.github.io | 公开 | main | CC0-1.0 | — | — | N/A | 站点示例与 wasm demo |
| .github | GitHub 组织健康文件 | —（无 Gitee） | github.com/evorule/.github | 公开 | main | CC0-1.0 | — | — | N/A | org profile/health 文件 |

## 维护规则

1. **新建仓**：按本表模板登记 → 两端建仓（Gitee 主仓 + GitHub 镜像）→ 加 mirror.yml（参数化模板，例外硬编码）→ README 顶部 Mirror 标识 → 提交本表。
2. **变更**：迁移 / 改名 / 默认分支 / 可见性 / License 变化 → 先改本表再执行，执行后回填实际结果。
3. **双 agent 红线**：`evo-agent` 与 `evorule-agent` 是两个独立项目，任何场景不得合并、不得将对方链接当作自身 canonical。
4. **缺口跟踪**：`evorule-console`、`evorule-dsh-skill` 无独立 CI（T4）；`evorule-agent` 镜像豁免（用户决策）；迁组织（品牌）降为可选。
