# Release Artifact Signing and Verification

The GitHub Release binaries of EvoRule are signed with **cosign keyless signing** (OIDC-based; no private key needs to be custody-kept),
producing `.sig` files that are provided alongside every Release asset.

## Verification (using `evorule-linux-x86_64` as an example)

```bash
cosign verify-blob \
  --certificate-identity-regexp 'https://github.com/evorule/evorule/\.github/workflows/release\.yml@.*' \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  evorule-linux-x86_64 \
  --signature evorule-linux-x86_64.sig
```

- Exit code `0` = verification passed: the binary was indeed signed by this repository's CI in the `release.yml` workflow and has not been tampered with.
- The Windows binary corresponds to `evorule-windows-x86_64.exe` / `evorule-windows-x86_64.exe.sig`.
- Every Release also ships a `sha256-checks.txt` for integrity verification.

## How It Works

- Signing happens in the `sign` job of `release.yml`, which uses GitHub OIDC (`permissions: id-token: write`)
  to request a short-lived certificate from Sigstore Fulcio — **zero key management**, so there is no key-leak/rotation concern.
- The certificate identity is bound to this repository's `release.yml` workflow; nobody can forge a signature with the same identity
  from another repository or another workflow.
- The `certificate-identity-regexp` in the verification command uses `.*` to cover both trigger sources, `refs/tags/v*` and
  `refs/heads/main`; do not tighten it to a fixed value outside this regex, or legitimate signatures may be wrongly rejected.

## Can I Use an Artifact Without Signing?

Yes. Signing is a proof of **integrity/authenticity**, not a usage blocker. Unsigned or tampered binaries will fail verification,
prompting you to re-download from the official Release.
