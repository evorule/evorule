<!--
SPDX-License-Identifier: CC0-1.0
Security disclosure procedures are public knowledge; we release them under CC0 so everyone knows how to report vulnerabilities safely.
-->

# Security Vulnerability Reporting Policy

**Last updated**: 2026-09-17

## ⚠️ Supported Versions

| Version                     | Support status | Notes                                        |
| --------------------------- | -------------- | -------------------------------------------- |
| `v0.6.x` (including 0.6.0)  | ✅ Supported   | Current primary support line (released 2026-09-14) |
| `v0.2.x` (including 0.2.0)  | ❌ EOL         | Public foundation stage, superseded by 0.6.x  |
| `v0.1.x` (including 0.1.0 / 0.1.1) | ❌ Unsupported | Early stage, unmaintained                     |
| `v6.0.x` (legacy internal version) | ❌ EOL         | Internal historical version, retired          |
| `< v0.1.0-alpha.1`          | ❌ Unsupported | Commits before the public repository are unmaintained |

**Alpha-stage commitments**:

- Critical / High vulnerabilities: fixed within 60 days
- Medium / Low vulnerabilities: deferred to 0.2.0
- Security advisories: publicly disclosed within 30 days after the fix (coordinated)

**Commitments after 1.0.0** (this document will be updated then):

- Critical: 7 days
- High: 30 days
- Medium: 90 days
- Low: next release

---

## Reporting a Security Vulnerability

If you discover a security vulnerability in the EvoRule project, please disclose it responsibly via the following channels:

### 📧 Contact

- **Email**: <evorulelab@gmail.com> (add `[SECURITY]` to the subject)
- **GitHub private message**: the maintainer (@evorule)
- **Encryption**: no PGP public key is currently provided (contact us if you need one)

### 📋 What to Include

Please include in your report:

1. Vulnerability type and description
2. Reproduction steps
3. Potential impact assessment
4. Suggested fix (if any)
5. Mitigations already attempted

### ⏱️ Response Time Commitments

- **Acknowledgment of receipt**: within 48 hours
- **Initial assessment**: within 5 business days
- **Fix plan**: within 10 business days
- **Public disclosure**: within 30 days after the fix (coordinated)

### 🔒 Confidentiality Commitment

Before the vulnerability is fixed and publicly disclosed, we will:

- Keep your report strictly confidential
- Not share the relevant information with third parties
- Keep you informed of fix progress in a timely manner

### 🙏 Acknowledgment

Security researchers who disclose responsibly will be acknowledged in the post-fix release announcement (with your consent).

### 🔐 EvoRule-Specific Security Considerations

EvoRule involves several security boundaries that require particular attention:

| Boundary             | Risk                                    | Mitigation                                                       |
| -------------------- | --------------------------------------- | ---------------------------------------------------------------- |
| `core_eval.json` loading | A malicious constitution could execute arbitrary transforms | Load only trusted constitution files; build.rs compile-time gate |
| `query_db` SQL       | SQL injection                           | Parameters must be bound with `?` placeholders; `audit/verify` records the full trail |
| Business-rule hot reload | Malicious JSON triggering dangerous operations | Watched-directory restriction + validation + audit log           |
| HTTP API authentication | Unauthorized access                     | Bearer token + timing-attack protection (`subtle` crate)         |
| FFI interface        | Memory safety                           | Opaque pointers + "pointer + length" string pattern              |

### 📜 Known Security Issues

There are currently **no known, unfixed security issues**.

For historically fixed issues, see [CHANGELOG.md](CHANGELOG.md).

---

**Important**: please do not publicly discuss unfixed security vulnerabilities in public forums, social media, or the issue tracker.

**Author**: EvoRule Project Team
**Email**: <evorulelab@gmail.com>

---

