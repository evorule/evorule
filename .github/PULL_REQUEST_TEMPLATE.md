# Pull Request

## 变更说明
<!-- 简述本次 PR 改了什么、为什么。跨 crate 改动请先完成 CHANGE_REQUEST 登记。 -->

## 关联
- 关联 Issue / CHANGE_REQUEST: #

## 检查清单
- [ ] 已通过 `cargo fmt --all -- --check`、`cargo test --workspace --all-targets` 与 `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] 已更新相关文档（若改动源码/配置，参见 `docs/CODE_DOC_MAP.md` 的映射）
- [ ] 宪法联动：触及语义路径（`evorule-tcb/`、`evorule-discipline/` —— 指令集 / 域 / `__exec__` 结构）时，已同批升级 [evorule-system-rules](https://gitee.com/evorule/evorule-system-rules) 宪法（semver minor+，条文 [CON-R-003]），并在本行改写为宪法仓 PR/commit 链接；未涉语义路径可不勾选
- [ ] 若涉及验证资产/验证文档：已同步 `verification/STATUS.md`，且其快照版本与 `Cargo.toml` workspace version 一致（[MECHANISM.md](verification/MECHANISM.md) M4）
- [ ] 已阅读并同意 [Individual CLA](CLA-individual.md) —— 签署方式：在 PR 评论
      `I have read the CLA Document and I hereby sign the CLA`

## 备注
<!-- 任何需要 reviewer 注意的点 -->
