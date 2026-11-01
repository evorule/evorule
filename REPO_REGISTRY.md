# REPO_REGISTRY — EvoRule 生态仓库注册表

> **权威登记表**：EvoRule 生态全部仓库的定位、分支、License 与 CI 现状。
> **拓扑基线**：2026-11-01 起 GitHub 单主——`github.com/evorule/*` 为唯一权威源；历史 Gitee 仓冻结（仅私仓 evorule-application / rpsm 与 evorule-agent 历史路径保留事实记录）。
> **维护铁律**：新建 / 迁移 / 改名 / 变更默认分支或可见性，**必须同步更新本表**；本表与现状不一致即视为债务。
> 最后更新：2026-11-02（GitHub 单主转换后全量核录）

## 命名空间速览

| 命名空间 | 平台 | 性质 |
|---|---|---|
| `github.com/evorule/*` | GitHub 组织 evorule | 权威主仓（公开，除 agent 为私有） |
| `gitee.com/evorulelab/*` | 历史私有路径（仅 evorule-agent，已冻结） | 私有仓事实记录 |
| `gitee.com/evorule/*` | 历史公开仓（已冻结）+ 两个仍在役私仓（evorule-application / rpsm） | 事实记录 |

## 主表

| 仓名 | 角色 | GitHub URL | 可见性 | 默认分支 | License | GitHub CI | 备注 |
|---|---|---|---|---|---|---|---|
| evorule | 核心引擎（TCB 唯一版本源） | github.com/evorule/evorule | 公开 | main | AGPL-3.0-or-later（双许可） | ci,cla,clippy,codeql,differential,fuzz,kani,mutants,release,scorecard,tla | 生态唯一版本源；工具链既定裁定（rust 1.97.1） |
| evorule-server | 自托管服务端 | github.com/evorule/evorule-server | 公开 | main | AGPL-3.0-or-later | ci,cla,release | GitHub 权威源 |
| evorule-rule | 业务规则集 | github.com/evorule/evorule-rule | 公开 | main | AGPL-3.0-or-later | ci,cla,release | GitHub 权威源 |
| evorule-system-rules | 系统规则集 | github.com/evorule/evorule-system-rules | 公开 | main | AGPL-3.0-or-later | cla | 无独立构建 CI（cla） |
| evorule-console-cloud | 云控制台 | github.com/evorule/evorule-console-cloud | 公开 | main | AGPL-3.0-or-later | ci,cla,deploy-demo,release | GitHub 权威源 |
| evorule-console | 控制台前端 | github.com/evorule/evorule-console | 公开 | main | AGPL-3.0-or-later | — | **无独立 CI（T4 缺口，待补）** |
| evorule-hash | 哈希链 | github.com/evorule/evorule-hash | 公开 | main | AGPL-3.0-or-later | ci,cla,release | GitHub 权威源 |
| evorule-bundle | 规则包 | github.com/evorule/evorule-bundle | 公开 | main | AGPL-3.0-or-later | ci,cla,release | GitHub 权威源 |
| evorule-sdk | 多语言 SDK | github.com/evorule/evorule-sdk | 公开 | main | **Apache-2.0（设计选择，非漂移）** | ci | License 政策见主仓 CONTRIBUTING |
| evorule-dsh-skill | 安装器 Skill | github.com/evorule/evorule-dsh-skill | 公开 | main | AGPL-3.0-or-later | — | 无独立 CI（T4 缺口，待补） |
| evo-agent | 应用层编排（可信 AI 工作站） | github.com/evorule/evo-agent | 公开 | **main**（2026-09-16 由 master 迁移） | AGPL-3.0-or-later | ci,cla | 与 evorule-agent **严格区分**（不同设计/功能） |
| evorule-experience-pack | 本地体验包（消费者入口·真用层：双击启动真实引擎） | github.com/evorule/evorule-experience-pack | 公开 | main | AGPL-3.0-or-later（宪法 CC0） | — | **2026-09-16 建仓**（此前仅本地目录未登记，console-cloud README 曾指向 404）：16 条合规规则 + BLAKE3 审计链 + 热重载/Bundle/WAL；Release v0.6.0 附 zip（SHA256 见 Release 说明）；data/ 运行产物已 .gitignore |
| evorule-agent | 大脑主控 agent 运行时（技术预览） | github.com/evorule/evorule-agent | 私有 | main | AGPL-3.0-or-later | ci | canonical 历史为 gitee.com/evorulelab/evorule-agent（私有，已冻结）；2026-11-01 起 GitHub 为权威源 |
| evorule-application | 大众版应用层（等保2.0 门禁演示） | —（无 GitHub） | 私有 | main | AGPL-3.0-or-later | — | canonical 仍为 gitee.com/evorule/evorule-application（私有）；无镜像，仅备份链覆盖 |
| rpsm | 实时物理仿真原型（内核纯净+PLA 审计验证，Rust） | —（无 GitHub） | 私有 | **main**（2026-09-16 由 master 迁移） | AGPL-3.0-or-later | — | canonical 仍为 gitee.com/evorule/rpsm（私有）；无镜像，仅备份链覆盖 |
| evorule.github.io | GitHub Pages 站点 | github.com/evorule/evorule.github.io | 公开 | main | CC0-1.0 | — | 站点示例与 wasm demo |
| .github | GitHub 组织健康文件 | github.com/evorule/.github | 公开 | main | CC0-1.0 | — | org profile/health 文件 |

## 维护规则

1. **新建仓**：按本表模板登记 → GitHub 建仓（唯一权威源）→ README → 提交本表。
2. **变更**：迁移 / 改名 / 默认分支 / 可见性 / License 变化 → 先改本表再执行，执行后回填实际结果。
3. **双 agent 红线**：`evo-agent` 与 `evorule-agent` 是两个独立项目，任何场景不得合并、不得将对方链接当作自身 canonical。
4. **tag 例外规则**：`checkpoint-*` / `pre-*` 开发 checkpoint tag（主仓 evorule 现有 37 个）为开发痕迹，仅存历史 Gitee 仓（已冻结），不同步 GitHub；audit 门禁按此规则放行，其余版本 tag 以 GitHub 为准。
5. **缺口跟踪**：`evorule-console`、`evorule-dsh-skill` 无独立 CI（T4）。
