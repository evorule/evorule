<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule Free Commercial License (FCL)

**Version**: 2.0
**Effective date**: 2026-09-08
**Scope of application**: Class A open-source repositories of the EvoRule ecosystem (see [DUAL_LICENSE.md](DUAL_LICENSE.md))

---

## 1. Purpose

The base license of EvoRule is **AGPL-3.0-or-later**. To support individuals, small and medium enterprises, the public sector, and non-profit activities, EvoRule Project **grants free of charge** a **commercial exemption** to eligible entities: closed-source use, modification, embedding, and distribution of EvoRule, free from AGPL contagion obligations. This license **charges no licensing fee**.

---

## 2. Eligibility (any one suffices)

1. **Natural persons** (including freelancers);
2. **Enterprises with annual revenue < CNY 100 million (CNY ¥100,000,000)**, computed as the **parent company and all affiliates aggregated** (blocking shell-splitting free-riding); entities reporting revenue in currencies other than CNY convert at the RMB central parity rate published by the authorized provider on the determination date;
3. **Government agencies and public institutions**;
4. **Universities and research institutions**;
5. **Non-governmental non-profit organizations**.

Entities not meeting any of the above conditions (typically other enterprises / organizations with annual revenue ≥ CNY 100 million) must purchase the [Commercial License](COMMERCIAL_LICENSE.md).

---

## 3. Grant

Subject to eligibility and compliance with the obligations in Section 4, the grantee is granted a **worldwide, non-exclusive, royalty-free, irrevocable** license, exempting it from the obligations of AGPL-3.0 §4-6 (distribution copyleft) and §13 (the network clause), and entitling it to:

- Modify EvoRule, embed and link it into the grantee's proprietary code to form a **closed-source combined work**, without the combined work being subject to the AGPL;
- Offer services based on a modified version of EvoRule over an internal network or the internet, without publishing the server-side code;
- **Copy and deploy in unlimited quantities** within the scope of the grant.

This license may be sublicensed for use **within the same legal entity**, but it is **non-transferable** to third-party entities.

---

## 4. Boundary of Combined Works (Explicitly Stated)

- **Constituting a combined work (the paid anchor that triggers this exemption)**: library-level linking, in-process embedding, static / dynamic linking — typically embedded firmware and edge-device runtimes. Such closed-source distribution requires this FCL or the Commercial License.
- **Explicitly excluded (does not trigger copyleft; no license needed)**: separate processes calling EvoRule services through a **public network API (HTTP / gRPC etc.)**. This situation is treated as **separate works**; the "corresponding source" obligation of AGPL §13 applies only to the EvoRule service itself (which is already open-sourced) and **does not affect the caller's code**.

---

## 5. Obligations

- Retain the original EvoRule copyright notices, disclaimers, and this grant statement;
- **Include the text of this license** when redistributing copies;
- **Do not remove or conceal** the EvoRule audit-chain compliance markers (used for post-hoc compliance forensics);
- Trademark use must still comply with [TRADEMARK.md](TRADEMARK.md).

---

## 6. Self-Attestation

This exemption adopts a **self-attestation scheme**: **no prior approval, no license keys, no mandatory telemetry**. The grantee determines its own eligibility and **retains a self-declaration record** (entity type, revenue band, or public-sector status).

EvoRule Project reserves the right to require a grantee to confirm its eligibility upon reasonable doubt; if eligibility is untrue, the grant is terminated per Section 7.

---

## 7. Term and Termination

- While eligibility continues to be met and no breach occurs, the grant remains in effect.
- **Ordinary breach**: correction within **15 days (cure period)** of written breach notice preserves the grant; failure to correct within the period terminates the grant automatically, and the grantee must cease use and destroy the relevant copies.
- **Willful or repeated breach**: a second material breach after one breach correction results in **permanent termination** of the grant.
- **Patent litigation**: if the grantee initiates patent infringement litigation against the copyright holder or its licensees over EvoRule, this license (including the patent grant) **terminates immediately**.
- Termination does not affect copies already distributed under the AGPL before termination.

---

## 8. FAQ

### Q1: I already use EvoRule under the AGPL — do I still need the FCL?

**A**: If you are an eligible entity and wish to be **exempt from publishing your modified code**, you need the FCL. If you open-source all modifications under the AGPL, you do not.

### Q2: Can the FCL cover multiple affiliated institutions?

**A**: It may be sublicensed within the same legal entity; independent legal entities must each qualify individually (affiliates aggregate revenue for the threshold, but each entity's own use must itself be eligible).

### Q3: Can a commercial enterprise obtain the FCL through donations?

**A**: No. Regardless of donations, a commercial enterprise with annual revenue ≥ CNY 100 million must purchase the Commercial License; the FCL is limited to the eligible entities listed above.

### Q4: Can open-source projects use EvoRule?

**A**: Yes, directly under AGPL-3.0-or-later. Open-source projects do not need the FCL, but their code must also be AGPL-compatible.

### Q5: My institution later becomes for-profit — does the grant terminate?

**A**: Yes. An eligibility change (e.g. a public institution becoming for-profit) requires written notice to EvoRule Project, and the original FCL terminates; a Commercial License may be purchased instead.

---

## 9. Contact and Version

**Questions**: <evorulelab@gmail.com> (add `[Free Commercial License]` to the subject)

---

## Version History

| Version | Date | Change |
|---|---|---|
| 1.0 | 2026-07-19 | Initial version (form-based approval scheme) |
| 2.0 | 2026-09-08 | Converted to self-attestation (no keys / no telemetry); clarified the combined-work boundary (network API exclusion clause); revenue threshold set at $10M (with affiliates); 15-day cure period / immediate termination on patent litigation |
| 2.1 | 2026-09-08 | Revenue threshold converted to a CNY basis: CNY 100 million (with affiliate aggregation); added the foreign-currency conversion clause at the central parity rate on the determination date |

---

**The final right of interpretation of this policy belongs to EvoRule Project. This document does not constitute legal advice; it is recommended to consult a qualified attorney before formal use.**
