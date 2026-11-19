<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule Governance Charter

**Version**: 1.0
**Effective date**: 2026-09-08
**Nature**: Public governance document (L1). The internal operations manual (GOVERNANCE-SETUP) is outside its scope.

---

## 1. Decision Authority

- **The project founder (Damu / EvoRule Project)** holds final decision authority, including:
  - Approval of all changes to the core repositories (mechanism layer);
  - Final sign-off on licensing and business-model matters (see [DUAL_LICENSE.md](DUAL_LICENSE.md) and the project's decision record);
  - Maintainer promotion and role appointments.
- The project is currently in **single-maintainer** mode (the founder is the sole maintainer). This charter reserves the structure for multiple maintainers later.

## 2. Change Review (Change Request)

- All core-module changes **must** be registered and reviewed through the CHANGE_REQUEST process (the change registration file is not published with this repository and is retained internally by the maintainer; the template is no longer published with the repository, and the self-check is enforced by the maintainer's local git pre-commit hook, see Section 5 below).
- Review status transitions:

  ```
  Pending review → Approved → Implemented
  Pending review → Rejected → Resubmitted
  Emergency pass → Retrospective review form filed → Approved / Rejected
  ```

- **Approver**: the project founder. Review conclusions are recorded in Section 6, "Review Approval", of the template.
- **Mechanism-layer gate**: policy-layer changes are forbidden at the mechanism layer (enforced by the build.rs compile-time gate, always on with no override valve; see [GATE_REFERENCE.md](GATE_REFERENCE.md) for details).

## 3. Maintainer Promotion (Reserved)

| Stage | Conditions | Appointment |
|---|---|---|
| Current stage | Single-maintainer mode | Founder |
| Contributor → Maintainer | Sustained high-quality contributions + signed CLA + founder nomination | Appointed by the founder |

> There is currently no maintainer candidate in place; this table is a placeholder for the public governance structure, with no preset timeline.

## 4. CLA Handling

- **Individual contributors**: sign [CLA-individual.md](CLA-individual.md).
- **Corporate contributors**: sign [CLA-corporate.md](CLA-corporate.md) (complementary to the Individual CLA).
- The dual-track CLA is a prerequisite for commercial re-licensing (DUAL_LICENSE), ensuring the project holds re-licensing rights over all code.
- CLA signing is verified automatically by the CLA Assistant bot (GitHub); PRs without a signed CLA are blocked by the gate.

## 5. Relationship to the CHANGE_REQUEST Process

- CR registration (`CHANGE_REQUEST.md`) is the **mandatory governance process** for mechanism-layer changes: changes must be registered and approved before implementation. The self-check validation is executed by the maintainer's local git pre-commit hook — this is an engineering-quality self-check discipline, not an anti-forgery review mechanism (settled ruling), and it is no longer a public build.rs gate (settled ruling, 2026-09-15).
- The CLA is a **contributor authorization** gate, independent of change review, but both must be satisfied before a PR merge.
- License / trademark / governance document changes are documentation-class (Class D) changes and still require CR registration, but they **do not trigger the mechanism-layer gate** (license documents at the repository root are not read into build.rs).

---

## Version History

| Version | Date | Change |
|---|---|---|
| 1.0 | 2026-09-08 | Initial version, implementing phase 1 of the project decision record |
