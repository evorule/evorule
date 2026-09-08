# 发布产物签名与验证

EvoRule 的 GitHub Release 二进制由 **cosign keyless 签名**（基于 OIDC，无需保管任何私钥）
生成 `.sig` 文件，随每个 Release 资产一同提供。

## 验证（以 `evorule-linux-x86_64` 为例）

```bash
cosign verify-blob \
  --certificate-identity-regexp 'https://github.com/evorule/evorule/\.github/workflows/release\.yml@.*' \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  evorule-linux-x86_64 \
  --signature evorule-linux-x86_64.sig
```

- 退出码 `0` = 验证通过：该二进制确由本仓库 CI 在 `release.yml` 工作流中签名，未被篡改。
- Windows 二进制对应 `evorule-windows-x86_64.exe` / `evorule-windows-x86_64.exe.sig`。
- 每个 Release 同时附带 `sha256-checks.txt` 用于完整性校验。

## 工作原理

- 签名发生在 `release.yml` 的 `sign` job，使用 GitHub OIDC（`permissions: id-token: write`）
  向 Sigstore Fulcio 申请短期证书，**零密钥管理**——不存在密钥泄露/轮换问题。
- 证书身份（certificate identity）绑定到本仓库的 `release.yml` 工作流，任何人都无法用
  其他仓库或其他工作流伪造同名签名。
- 验证命令中的 `certificate-identity-regexp` 用 `.*` 覆盖 `refs/tags/v*` 与
  `refs/heads/main` 两种触发来源，请勿收紧为正则之外的固定值以免误拒合法签名。

## 不签名就无法用吗？

不。签名是**完整性/真实性证明**，不阻断使用。未签名或被篡改的二进制在验证时会报错，
提示你从官方 Release 重新下载。
