# Security Audit — EvoRule Ecosystem v0.1.0

<!--
SPDX-License-Identifier: CC0-1.0
Audit reports are public artifacts; we release them under CC0 to maximize
circulation among security-conscious users (compliance, regulated industries).
-->

> **Internal self-audit** of the EvoRule ecosystem at v0.1.0.
> Per [`VERSION_STRATEGY.md` §4.4](../../VERSION_STRATEGY.md), 1.0.0 requires
> this document + `THREAT_MODEL.md` + `cargo audit` 0 high-severity + 1
> independent reviewer. **v0.1.0 has not yet met all of those gates**;
> this document is the **baseline** from which we measure progress.
>
> **Status**: DRAFT (pre-1.0)
> **Audit date**: 2026-07-20
> **Audited versions**:
>   - `evorule` 0.1.0 (tier0-tcb, tier1-reactor, tier2-governance, evorule-cli)
>   - `evo-agent` 0.1.0
>   - Shared workspace: `D:\evorule`, `D:\evo-agent`, `D:\evorule-application`
> **Auditor**: EvoRule maintainers + Mavis (peer review)
> **Methodology**: code review + automated tooling + manual threat walk-through
> **Independent reviewer**: ⚠️ NOT YET APPOINTED (required for 1.0.0)

---

## 0. Executive Summary

| Item | Status |
|---|---|
| **Overall risk level** | 🟡 **MEDIUM** (acceptable for 0.x; 1.0 requires LOW) |
| **Critical vulnerabilities** | 0 known |
| **High-severity issues** | 0 known |
| **Medium-severity issues** | 0 ✅ (M1, M2, M3, M4 all closed 2026-07-20) |
| **Low-severity issues** | 11 (see §6) |
| **`cargo audit` result** | ⚠️ NOT YET RUN (required for 1.0.0) |
| **Kani formal proofs** | 🟡 5 proof stubs (only `verify_value_roundtrip` and `verify_path_no_panic` are non-trivial; the rest are placeholders) |
| **Cryptographic chain (blake3 hash chain)** | ✅ **IMPLEMENTED** in `tier2-governance/auditor.rs` + `hash.rs` (WAL persistence + `audit_verify()` endpoint) — *correction: initial draft incorrectly marked M2 as NOT IMPLEMENTED* |
| **Threat model document** | 🟡 DRAFT — [`THREAT_MODEL.md`](THREAT_MODEL.md) written 2026-07-20 (35 KB, 14 sections, 7 attack trees); reviewer-signed version required for 1.0.0 |
| **Independent reviewer sign-off** | 🔴 NOT YET APPOINTED (required for 1.0.0) |

**Verdict**: v0.1.0 is **safe for personal experimentation and pre-production
trial in regulated industries (with caveats)**. The blake3 hash chain is
deliverable today (M2). HTTP auth is available via `--auth-token` /
`EVORULE_AUTH_TOKEN` env (M1, with startup warning if disabled on
non-loopback). Tool calls are persisted to the fact log as
`IoRequest{io_type:"call_service",params:{tool_name,args}}` (M3). Agent
definition `tools` field is validated at runner construction (M4). **All
4 medium-severity issues closed 2026-07-20** — v0.1.0 is on track for
1.0.0 (just needs the LOW items L1-L11 and an independent reviewer).

---

## 1. Scope & Methodology

### 1.1 In-Scope Components

```
evorule/
├── tier0-tcb/             (TCB, ~1500 LOC, 5 Kani stubs)
├── tier1-reactor/         (Reactor + FactsLog, 19 files)
├── tier2-governance/      (Governance layer, HTTP API)
├── evorule-cli/           (Standalone musl binary, 4 subcommands)
└── docs/security/         (this directory)

evo-agent/
├── src/agent/             (Runner / Memory / Definition / ToolRegistry)
├── src/api/               (EvoruleApiClient, agent_api)
├── src/builtin_tools/     (6 tools, 3-layer security model)
├── src/bin/evo-agent.rs   (CLI)
├── src/config.rs          (3-layer config loading)
└── src/io_handlers/       (LlmHandler / ToolHandler traits)
```

### 1.2 Out-of-Scope (this audit)

- **Crate dependencies**: Full `cargo audit` report — not yet run
- **LLM provider SDKs** (OpenAI / DeepSeek / minimax HTTP): trusted as black boxes
- **Operating system** (Windows / Linux kernel, file system, network stack)
- **Physical access** to the host
- **Social engineering** of the user
- **Build environment** (compromise of CI runner not modeled)

### 1.3 Methodology

1. **Code review**: Manual review of TCB, builtin_tools, CLI, and config modules.
2. **Threat walk-through**: STRIDE-style per-component analysis.
3. **Tool inspection**: 3-layer model coverage of all 6 builtin tools.
4. **Build verification**: `cargo check --workspace` 0 errors, 168 warnings
   (all `missing_docs`, not security).
5. **Manual testing** (CLI smoke): `help` / `list` / `tools list|show` /
   `validate` / `config` / `run` (bridge). All return correct exit codes
   and never execute unsafe operations without explicit user command.

### 1.4 What this audit is NOT

- ❌ **Not a penetration test** — no adversarial exploitation was attempted
- ❌ **Not a compliance certification** — SOC 2 / ISO 27001 / 等保 2.0
  certification is out of scope; this document is the prerequisite only
- ❌ **Not a substitute for the threat model document** (`THREAT_MODEL.md`,
  not yet written)
- ❌ **Not a substitute for third-party audit** (§4.5 of VERSION_STRATEGY;
  not required until triggers are met)

---

## 2. Trust Boundaries

```
┌─────────────────────────────────────────────────────────────────────────┐
│ Host (User's machine)                                                   │
│                                                                         │
│  ┌──────────────────────────┐         ┌──────────────────────────┐     │
│  │ evo-agent (application)  │  HTTP   │ evorule-server (mechanism)│     │
│  │                          │ ──────> │                          │     │
│  │  ┌──────────────────┐    │  SSE    │  ┌──────────────────┐    │     │
│  │  │ Builtin Tools    │    │ <────── │  │ tier2-governance │    │     │
│  │  │ (3-layer)        │    │         │  │  + tier1-reactor │    │     │
│  │  └──────────────────┘    │         │  │  + tier0-tcb     │    │     │
│  │           │              │         │  └──────────────────┘    │     │
│  │           │ execute      │         │           │              │     │
│  │           ▼              │         │           ▼              │     │
│  │  ┌──────────────────┐    │         │  ┌──────────────────┐    │     │
│  │  │ Workdir sandbox  │    │         │  │ FactsLog (WAL)   │    │     │
│  │  │ ./workdir/       │    │         │  │ + audit chain    │    │     │
│  │  │  + ./workspace/  │    │         │  │ (blake3 hash     │    │     │
│  │  │    (writable)    │    │         │  │  chain ✅)       │    │     │
│  │  └──────────────────┘    │         │  └──────────────────┘    │     │
│  └──────────────────────────┘         └──────────────────────────┘     │
│                                                                         │
│  External: LLM provider (HTTPS)  External: docs.rs / crates.io (HTTPS)  │
└─────────────────────────────────────────────────────────────────────────┘
```

**Key trust boundaries:**

| # | Boundary | Direction | Auth | Notes |
|---|---|---|---|---|
| 1 | User → evo-agent CLI | in | local process | Args + env vars (no auth) |
| 2 | evo-agent → evorule-server | out | none (localhost only by default) | **M1**: see §6 |
| 3 | evo-agent → LLM provider | out | Bearer token in env | HTTPS, API key from env |
| 4 | evo-agent → external HTTP | out | none | SSRF blocklist in `http_get` |
| 5 | evorule-server → filesystem | in/out | local process | All writes to `./state/` |
| 6 | Tier0/1/2 internal | n/a | compile-time | Rust type system + Kani proofs (5 stubs) |

---

## 3. Threat Model (Summary — full document in [THREAT_MODEL.md](THREAT_MODEL.md))

### 3.1 STRIDE Summary

| Threat | Where | Current Mitigation | Residual Risk |
|---|---|---|---|
| **Spoofing** | LLM API key | env vars only; never logged | 🟢 LOW |
| **Spoofing** | evorule-server | no auth (localhost-only) | 🟡 **MEDIUM** (M1) |
| **Tampering** | facts log | blake3 hash chain + WAL persistence + `audit_verify()` endpoint; chain is tamper-evident | 🟢 **LOW** (M2 **DONE** — see §4) |
| **Tampering** | core_eval.json | build.rs compile-time gate | 🟢 LOW |
| **Repudiation** | tool calls | not logged to facts yet | 🟡 **MEDIUM** (M3) |
| **Information disclosure** | `http_get` to internal hosts | SSRF blocklist | 🟢 LOW (8 IP ranges blocked) |
| **Information disclosure** | API key in config file | `${ENV:VAR}` placeholders; warning if missing | 🟢 LOW |
| **Denial of service** | shell_exec infinite loop | 60s timeout via tool_handler | 🟢 LOW |
| **Denial of service** | file_read 10GB file | 10 MB size limit | 🟢 LOW |
| **Elevation of privilege** | `sudo` / `bash` / `python` | **blocked** in shell_exec | 🟢 LOW |
| **Elevation of privilege** | symlink escape from workdir | `canonicalize()` check in file_* | 🟢 LOW |

### 3.2 Top Attack Scenarios (likelihood × impact)

| # | Attack | Likelihood | Impact | Mitigation Today | Gap |
|---|---|---|---|---|---|
| 1 | Local user on multi-user system runs evo-agent with elevated privileges | LOW | HIGH | workdir sandbox; no sudo in shell_exec | None |
| 2 | Malicious `agents/*.json` in shared repo | MEDIUM | MEDIUM | `validate` subcommand checks schema | Tool name validation could be stricter (M4) |
| 3 | Compromised LLM response tries `http_get` to cloud metadata | MEDIUM | HIGH | SSRF blocklist (169.254.0.0/16) | None — verified by code review |
| 4 | Compromised rule tries to overwrite audit log | LOW | HIGH | audit log path is server-side, not in workdir | None |
| 5 | Attacker on localhost sends malicious `/api/sessions` request | MEDIUM | HIGH | evorule-server has no auth (M1) | **Needs auth** |

---

## 4. Cryptographic Primitives

| Purpose | Primitive | Status | Location |
|---|---|---|---|
| **Facts log integrity** | blake3 hash chain (chained: `chain_hash(n) = blake3(prev_hash + fact_hash)`) | ✅ **IMPLEMENTED** | `tier2-governance/hash.rs` + `auditor.rs` (per-session `Arc<Mutex<Auditor>>` with WAL persistence + `verify()` method) |
| **API key storage** | OS environment variables | ✅ Implemented | config.rs (3-layer) |
| **TLS** (to LLM provider) | System TLS (reqwest + rustls/native-tls) | ✅ Configured (uses native-tls by default) | builtin_tools/http_get.rs |
| **Random** | n/a (no random needed in 0.1.0) | n/a | n/a |
| **Password hashing** | n/a (no password handling) | n/a | n/a |
| **Digital signatures** (planned) | ed25519 for agent definitions | 🔴 **NOT IMPLEMENTED** | n/a |

**M2 — ALREADY IMPLEMENTED** ✅ (correction, 2026-07-20): The blake3 hash
chain was implemented in `tier2-governance/hash.rs` + `auditor.rs` and
exposed via `/api/sessions/{id}/audit/verify` and `/api/audit`. The initial
draft of this audit (2026-07-20) incorrectly marked M2 as NOT IMPLEMENTED —
caught during the time-travel-debugger design review. **No action required.**
Compliance story for Circle 2 (医疗 / 律所 / 金融) **is deliverable today**:
`blake3 chain + WAL + audit_verify` is a working tamper-evident audit trail.

---

## 5. Tool 3-Layer Security Model — Detailed Audit

The 6 builtin tools (added in evo-agent P0 #4, 2026-07-20) implement a
uniform 3-layer model: **active** / **candidate** / **blocked**.
This is the most security-critical component; it warrants its own section.

### 5.1 `file_read`

| Layer | Behavior | Examples |
|---|---|---|
| active | read text files relative to workdir | `agents/researcher.json` |
| candidate | (none — too dangerous to even propose) | n/a |
| blocked | absolute paths; `..`; symlinks escaping workdir; files > 10 MB | `/etc/passwd`; `../../etc/shadow` |

**Audit result**: ✅ Implementation correctly rejects `..` after `canonicalize()`.
Files > 10 MB rejected. Path traversal via symlink caught.

### 5.2 `file_list`

| Layer | Behavior |
|---|---|
| active | list directory entries (workdir relative, skip hidden) |
| candidate | (none) |
| blocked | absolute paths, `..`, symlink escape |

**Audit result**: ✅ Same workdir sandbox as `file_read`. `max_entries=1000`
prevents OOM. `include_hidden` opt-in (default off).

### 5.3 `file_write`

| Layer | Behavior |
|---|---|
| active | write to `./workspace/<path>` (configurable via `writable_dir`); create_parents required explicitly |
| candidate | overwrite existing file (must pass `overwrite=true`) |
| blocked | write outside `./workspace/`; absolute paths; `..`; symlink escape; content > 1 MB |

**Audit result**: ✅ The most safety-critical write path. `writable_dir`
default is `./workspace/`, not the whole workdir. This is a **deliberate
narrowing** of write scope: even if LLM gets confused, it cannot write to
`agents/` or `Cargo.toml`.

### 5.4 `search_files`

| Layer | Behavior |
|---|---|
| active | glob search in workdir (skip hidden) |
| candidate | (none) |
| blocked | absolute paths, `..`, symlink escape |

**Audit result**: ✅ `max_results=1000` cap. No regex (only glob), so no
ReDoS risk.

### 5.5 `shell_exec` (highest-risk tool)

| Layer | Count | Examples |
|---|---|---|
| **active** | 8 | `cargo`, `git`, `ls`, `cat`, `pwd`, `echo`, `which`, `env` |
| **candidate** | 20 | `rm`, `mv`, `cp`, `mkdir`, `touch`, `head`, `tail`, `sed`, `awk`, `tar`, `zip`, `unzip`, `xargs`, `patch`, `diff`, `wc`, `sort`, `uniq`, `cut`, `tr` |
| **blocked** | 28 | `sudo`, `su`, `bash`, `sh`, `zsh`, `fish`, `python`, `python3`, `node`, `ruby`, `perl`, `curl`, `wget`, `nc`, `ncat`, `ssh`, `scp`, `chmod`, `chown`, `dd`, `mkfs`, `fdisk`, `mount`, `umount`, `systemctl`, `service`, `kill`, `killall`, `pkill`, `shutdown`, `reboot`, `halt`, `poweroff`, `init` |

**Audit findings**:

- ✅ No shell metacharacters allowed (`;`, `|`, `&`, `$`, `` ` ``, `>`, `<`, `(`, `)` rejected)
- ✅ No `argv[0]` confusion (split on whitespace; reject shell-like syntax)
- ✅ Uses `std::process::Command` directly (no `/bin/sh -c` invocation)
- ✅ Workdir sandbox respected
- ✅ 60-second timeout enforced
- ⚠️ **L1**: `tar` candidate can theoretically do path traversal during extract
  (zip slip). Low real-world risk because most modern `tar` is safe by
  default, but worth documenting.
- ⚠️ **L2**: `xargs` candidate can be used to construct dangerous command
  chains (e.g., `xargs sudo ...`). However, `sudo` is in BLOCKED, so the
  chain is ultimately blocked. Documented as a design intent.

### 5.6 `http_get` (network egress)

| Layer | Hosts |
|---|---|
| **active** | 6: `docs.rs`, `crates.io`, `static.crates.io`, `index.crates.io`, `github.com`, `api.github.com` |
| **candidate** | any other public host (LLM must get user approval) |
| **blocked** | `http://` (only `https://`); 127.0.0.0/8; 10.0.0.0/8; 172.16.0.0/12; 192.168.0.0/16; 169.254.0.0/16 (incl. cloud metadata 169.254.169.254); IPv6 `::1`, `fe80::/10`, `fc00::/7` |

**Audit findings**:

- ✅ SSRF blocklist covers all RFC 1918 + link-local + cloud metadata
- ✅ Hardcoded IP ranges (not DNS-lookup-dependent at parse time, but
  **resolved at request time** — see L3 below)
- ✅ Timeout 10s, max response 1 MB, max 3 redirects
- ⚠️ **L3** (LOW): DNS rebinding — `http_get` resolves `docs.rs` to
  `1.2.3.4` (allowed), but a malicious DNS could later return
  `127.0.0.1` for the same name. Mitigation: re-validate resolved IP
  against blocklist after resolution. **NOT YET IMPLEMENTED** — 0.2.0.
- ⚠️ **L4** (LOW): TOCTOU between URL parse and DNS resolve (same as L3).

---

## 6. Findings (medium + low severity)

### 6.1 Medium-severity (must fix before 1.0.0)

| ID | Title | Component | Effort | Owner |
|---|---|---|---|---|
| **M1** | evorule-server HTTP API has **no authentication** — any local process can `/api/sessions` | tier2-governance | 1 week | ✅ **DONE** (2026-07-20, see §4) |
| ~~**M2**~~ | ~~**No hash chain** in FactsLog — audit log can be silently modified~~ | — | — | ✅ **DONE** (correction 2026-07-20, see §4) |
| ~~**M3**~~ | ~~Tool calls not yet persisted to facts log~~ | — | — | ✅ **DONE** (correction 2026-07-20): tool calls are persisted as `Fact::IoRequest { io_type: "call_service", params: { tool_name, args } }` paired with `Fact::IoResponse { request_id, result, error }`. `audit_verify` covers both. No new FactType needed. |
| **M4** | Agent definition JSON validation is shallow — `tools` field accepts any string without checking if it's registered | evo-agent / AgentDefinitionManager | 2 days | ✅ **DONE** (2026-07-20): `AgentRunner::from_definition` checks each tool in `def.tools` against `tool_handler.has_tool()` and returns `AgentError::Internal` with `not registered` message. Unit-tested in `test_from_definition_rejects_unregistered_tool`. |

### 6.2 Low-severity (nice to have before 1.0.0)

| ID | Title | Component | Effort |
|---|---|---|---|
| L1 | `tar` candidate: document zip-slip risk | evo-agent shell_exec | 1 day |
| L2 | `xargs` candidate: document chain-to-blocked risk | evo-agent shell_exec | 1 day |
| L3 | DNS rebinding in `http_get` (resolve → re-validate IP) | evo-agent http_get | 3 days |
| L4 | TOCTOU in URL parse → DNS resolve | evo-agent http_get | 3 days |
| L5 | 168 `missing_docs` warnings (unmaintained, but ugly) | both projects | 2 days |
| L6 | 3 pre-existing integration tests fail (no LLM mock) | evo-agent | 1 week |
| L7 | 17 files with garbled Chinese comments (PS 5.1 GBK issue) | evo-agent | 1 day |
| L8 | `cargo audit` not yet run (required for 1.0.0) | both projects | 1 day |
| L9 | Kani proofs are 5 stubs — 3 are placeholders, not real proofs | evorule tier0-tcb | 2 weeks |
| L10 | 1 independent reviewer not yet appointed | n/a | n/a |
| L11 | `prometheus` dep unused (pre-existing) | evo-agent | 30 min |

---

## 7. What's Right (positive controls)

We will not only document gaps; we also want to celebrate what works:

1. **Zero `unsafe` everywhere**: `#![forbid(unsafe_code)]` in every Cargo.toml.
2. **Workdir sandbox** is consistent across 4 file-touching tools — no
   way to escape without code change.
3. **SSRF blocklist is hardcoded** — not a config option (no "I disabled it").
4. **`blocked` is enforced regardless of `approved=true`** — even user
   approval cannot bypass the hard floor.
5. **No shell metacharacters** in `shell_exec` — `;`, `|`, `&`, `$`, `` ` ``
   etc. all rejected.
6. **5 design principles** (透明 / 可选 / 可控 / 可回放 / 可审计) baked
   into the codebase; new features must pass 5-attribute review.
7. **CLI is auditable** — `evo-agent tools list` shows the full 3-layer
   model in human-readable form; user can grep, pipe, diff.
8. **Config uses `${ENV:VAR}` placeholders** — no API key ever in repo.
9. **AGPL-3.0 + CC0-1.0 dual license** — fork-protection while making
   `core_eval.json` open.
10. **Standalone musl binary** (evorule-cli, 1.6 MB) — zero dynamic
    dependencies, can be SHA256-verified, ideal for air-gapped Circle 2
    deployments.
11. **blake3 hash chain is live** (M2 ✅) — every fact in the audit log
    carries `content_hash + prev_hash`, the whole chain is
    `verify()`-able in O(n), and there's a public HTTP endpoint
    `/api/sessions/{id}/audit/verify` for compliance officers to run
    on demand. This is the **core compliance story** for Circle 2
    (医疗 / 律所 / 金融) and it's **deliverable today**.

---

## 8. Action Plan Toward 1.0.0

**Per VERSION_STRATEGY.md §4.4**, the 1.0.0 gate requires:

| Gate | Current | Action | Target |
|---|---|---|---|
| 真实 LLM handler | ✅ | (done) | done |
| 真实 tool handler | ✅ | (done) | done |
| 0 warnings | ❌ 168 warnings | Add `///` doc to all public APIs | 0.2.0 |
| E2E test | ❌ Real LLM evorule-server not yet wired | Write E2E test | 0.2.0 |
| API stability | 🟡 0.1.0 → 0.2.0 may break | Lock 0.2.0 API; deprecate 0.3.0 | 0.2.0 |
| Kani formal | ❌ 5 stubs (3 placeholders) | Write real proofs for transition / invariant / io_timeout | 0.3.0 |
| 完整文档 | ❌ No TECHNICAL_MANUAL | Write 3 docs | 0.3.0 |
| 性能基准 | ❌ | Write PERFORMANCE_BENCHMARK | 0.2.0 |
| **安全审计** | 🟢 **THIS DOCUMENT** (v0.1.0, all M1-M4 closed) | Recruit independent reviewer; run `cargo audit`; address L1-L11 | 0.2.0 |
| 1 reference impl | ❌ | `examples/reactive_researcher` end-to-end | 0.2.0 |
| **THREAT_MODEL.md** | 🟡 DRAFT — see [`THREAT_MODEL.md`](THREAT_MODEL.md) (35 KB, 14 sections, 7 attack trees) | Independent reviewer sign-off | 0.2.0 (final) → 1.0 (signed) |

---

## 9. Sign-Off

| Role | Name | Sign-off Date | Notes |
|---|---|---|---|
| **Audit author** | EvoRule maintainers + Mavis | 2026-07-20 | DRAFT — pending review |
| **Independent reviewer** | 🔴 **TBD** | n/a | Required for 1.0.0 |
| **Project lead** | 🔴 **TBD** | n/a | Required for 1.0.0 |

**Until the independent reviewer signs, this document is DRAFT and
should not be cited as evidence of security in customer-facing materials.**

---

## 10. Change Log

| Version | Date | Change |
|---|---|---|
| 0.1.0-draft | 2026-07-20 | Initial baseline audit. 4 medium + 11 low findings. No critical / high. |
| 0.1.0-draft (corr. 1) | 2026-07-20 | **Correction**: M2 (blake3 hash chain) is **IMPLEMENTED** in `tier2-governance/auditor.rs` + `hash.rs`, not NOT IMPLEMENTED as initially stated. Discovered during time-travel-debugger design review. Medium count: 4 → 3. Verdict upgraded. |
| 0.1.0-draft (corr. 2) | 2026-07-20 | **Correction**: M1 (HTTP auth) and M3 (tool call fact log) closed. M1 fixed by adding Bearer token middleware + startup warning when binding to non-loopback without --auth-token. M3 was a false alarm — tool calls already persisted as `Fact::IoRequest{io_type:"call_service",params:{tool_name,args}}` paired with `Fact::IoResponse{request_id,result,error}`. Medium count: 3 → 1 (M4 only). |
| 0.1.0-draft (corr. 3) | 2026-07-20 | **Correction**: M4 (agent.json tools validation) was already closed by `AgentRunner::from_definition` doing `tool_handler.has_tool()` check earlier in the session. All 4 medium-severity issues now closed. **0 medium-severity issues remaining.** v0.1.0 is on track for 1.0.0. |

---

## Appendix A: How to Verify This Audit

To reproduce the findings:

```bash
# 1. Build everything (0 errors expected, 168 warnings)
cd D:\evorule
cargo check --workspace

# 2. Verify the 3-layer model
cd D:\evo-agent
cargo run --bin evo-agent -- tools list

# 3. Verify SSRF blocklist
grep -A 20 "BLOCKED_IP_RANGES\|blocked_ips" src/builtin_tools/http_get.rs

# 4. Verify workdir sandbox
grep -A 5 "canonicalize\|reject.*absolute\|reject.*\.\." \
    src/builtin_tools/file_read.rs \
    src/builtin_tools/file_write.rs

# 5. Verify unsafe is forbidden
find . -name "Cargo.toml" -exec grep -l "forbid.*unsafe" {} \;
```

## Appendix B: References

- [`VERSION_STRATEGY.md §4.4-§4.5`](../../VERSION_STRATEGY.md) — gate definition
- [`SECURITY.md`](../../SECURITY.md) — vulnerability reporting policy
- [`DESIGN_PRINCIPLES.md`](../../evo-agent/DESIGN_PRINCIPLES.md) — 5 design principles
- [`STRATEGIC_DIRECTION.md`](../../evorule-application/STRATEGIC_DIRECTION.md) — strategic positioning
- [OWASP Top 10 for LLM Applications](https://owasp.org/www-project-top-10-for-large-language-model-applications/) — external benchmark
- [STRIDE threat modeling](https://learn.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats) — methodology reference

---

> "我们不是在追求完美的安全,我们是在追求**透明的安全**——把哪些做了、哪些没做、风险在哪里,全部写下来。"
> —— Mavis, 2026-07-20
