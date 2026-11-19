# Pull Request

## Change description
<!-- Briefly state what this PR changes and why. For cross-crate changes, complete the CHANGE_REQUEST registration first. -->

## Related
- Related Issue / CHANGE_REQUEST: #

## Checklist
- [ ] `cargo fmt --all -- --check`, `cargo test --workspace --all-targets` and `cargo clippy --workspace --all-targets -- -D warnings` all pass
- [ ] Relevant docs updated (for source/config changes see the mapping in `docs/CODE_DOC_MAP.md`)
- [ ] Constitution linkage: when semantic paths (`evorule-tcb/`, `evorule-discipline/` — instruction set / domains / `__exec__` structure) are touched, the [evorule-system-rules](https://github.com/evorule/evorule-system-rules) constitution has been upgraded in the same batch (semver minor+, clause [CON-R-003]), and this line rewritten as the constitution-linkage marker line with the constitution repo PR/commit link (exact marker format enforced by `scripts/check_constitution_linkage.py`); leave unchecked if no semantic path is involved
- [ ] If verification assets/docs are involved: `verification/STATUS.md` synced, and its snapshot version matches the `Cargo.toml` workspace version ([MECHANISM.md](verification/MECHANISM.md) M4)
- [ ] Read and agreed to the [Individual CLA](CLA-individual.md) — sign by commenting on the PR:
      `I have read the CLA Document and I hereby sign the CLA`

## Notes
<!-- Anything the reviewer should pay attention to -->
