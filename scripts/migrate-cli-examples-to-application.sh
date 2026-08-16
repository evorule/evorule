#!/usr/bin/env bash
#
# Copyright 2026 EvoRule Project
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
#
# migrate-cli-examples-to-application.sh — Bash 版（WSL / Linux / macOS）
# -----------------------------------------------------------------------------
# 把 evorule-cli/examples/ 下的两套业务规则模板（hospital + law-firm，共 10 个文件）
# 从 evorule 核心仓的 Git HEAD 历史（或本地备份目录）恢复到 evorule-application 兄弟仓。
#
# 迁移背景（AGENTS.md 边界判断表 "evorule-cli 加规则模板" 项）：
#   evorule 核心仓是纯机制层 framework，不内置业务方案。
#   hospital / law-firm 属于行业业务规则，按规范应放在 evorule-application 仓。
#   本脚本是 2026-07-30 越界清理操作的配套同步工具
#   （经项目方明确决策并确认，见 AGENTS.md 规则二留痕）。
#
# 推荐使用方式：把本脚本复制到 evorule-application 仓的 scripts/ 目录下执行；
# 或直接在 evorule 核心仓运行，并通过 --app-repo 指向兄弟仓路径。
#
# 恢复优先级：
#   1) 核心仓 Git HEAD 提交中的历史文件  →  git show HEAD:<path>  导出
#   2) 若 Git 中不存在（文件从未 commit），使用 --from-backup <目录> 指定备份路径
#
# 用法示例：
#   # 最简：脚本在 evorule/scripts/，兄弟仓 evorule-application 已克隆到同级目录
#   bash scripts/migrate-cli-examples-to-application.sh
#
#   # 显式指定两个仓路径，Git 找不到时用备份
#   bash scripts/migrate-cli-examples-to-application.sh \
#       --core-repo  /path/to/evorule \
#       --app-repo   /path/to/evorule-application \
#       --from-backup /path/to/backup/evorule-2026-07-29
#
#   # 只预览（不写盘）
#   bash scripts/migrate-cli-examples-to-application.sh --dry-run
#
# 参数：
#   --core-repo    PATH   evorule 核心仓路径（默认：脚本所在目录的上一级）
#   --app-repo     PATH   evorule-application 仓路径（默认：与 CoreRepo 同级的兄弟目录）
#   --from-backup  PATH   Git 恢复失败时的本地备份目录（按相对路径找：$Backup/evorule-cli/examples/...）
#   --target-subdir PATH  AppRepo 内写入的子目录（默认：examples/evorule-cli）
#   --dry-run             仅打印将要执行的操作，不真正写盘
#   --overwrite           目标文件已存在时强制覆盖（默认 SKIP 并打印警告）

set -euo pipefail

# ── 默认值 ──────────────────────────────────────────────────────────────────
CORE_REPO=""
APP_REPO=""
FROM_BACKUP=""
TARGET_SUBDIR="examples/evorule-cli"
DRY_RUN=0
OVERWRITE=0

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── 解析参数 ──────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --core-repo)     CORE_REPO="$2";    shift 2 ;;
        --app-repo)      APP_REPO="$2";     shift 2 ;;
        --from-backup)   FROM_BACKUP="$2";  shift 2 ;;
        --target-subdir) TARGET_SUBDIR="$2";shift 2 ;;
        --dry-run)       DRY_RUN=1;         shift   ;;
        --overwrite)     OVERWRITE=1;       shift   ;;
        -h|--help)
            sed -n '2,/^# 参数：/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "未知参数: $1（用 -h 查看帮助）"; exit 2 ;;
    esac
done

# ── 路径规范化 ─────────────────────────────────────────────────────────────
if [[ -z "$CORE_REPO" ]]; then
    CORE_REPO="$(cd "$SCRIPT_DIR/.." && pwd)"   # 脚本在 evorule/scripts/ → CoreRepo = evorule
fi
if [[ -z "$APP_REPO" ]]; then
    APP_REPO="$(cd "$CORE_REPO/.." && pwd)/evorule-application"
fi
TARGET_ROOT="$APP_REPO/$TARGET_SUBDIR"

echo "========================================================================"
echo " evorule-cli 业务规则模板迁移 → evorule-application"
echo "========================================================================"
echo " CoreRepo     = $CORE_REPO"
echo " AppRepo      = $APP_REPO"
echo " TargetRoot   = $TARGET_ROOT"
[[ -n "$FROM_BACKUP" ]] && echo " FromBackup   = $(cd "$FROM_BACKUP" && pwd)"
echo " DryRun       = $DRY_RUN"
echo " Overwrite    = $OVERWRITE"
echo

# ── 前置校验 ───────────────────────────────────────────────────────────────
if [[ ! -d "$CORE_REPO/.git" ]]; then
    echo "ERROR: CoreRepo 不是有效的 Git 仓库：$CORE_REPO" >&2
    exit 2
fi
if [[ ! -d "$APP_REPO" ]]; then
    echo "ERROR: AppRepo 不存在：$APP_REPO" >&2
    echo "请先克隆 evorule-application 仓（与 evorule 同级兄弟目录）。" >&2
    exit 2
fi

# ── 10 个迁移文件清单 ─────────────────────────────────────────────────────
MANIFEST=(
    "evorule-cli/examples/hospital/README.md                        hospital/README.md"
    "evorule-cli/examples/hospital/payload.example.json             hospital/payload.example.json"
    "evorule-cli/examples/hospital/rules/01-access-audit.json       hospital/rules/01-access-audit.json"
    "evorule-cli/examples/hospital/rules/02-prescription-guard.json hospital/rules/02-prescription-guard.json"
    "evorule-cli/examples/hospital/rules/03-privacy-redaction.json  hospital/rules/03-privacy-redaction.json"
    "evorule-cli/examples/law-firm/README.md                        law-firm/README.md"
    "evorule-cli/examples/law-firm/payload.example.json             law-firm/payload.example.json"
    "evorule-cli/examples/law-firm/rules/01-file-access-audit.json  law-firm/rules/01-file-access-audit.json"
    "evorule-cli/examples/law-firm/rules/02-conflict-of-interest.json law-firm/rules/02-conflict-of-interest.json"
    "evorule-cli/examples/law-firm/rules/03-deadline-tracker.json   law-firm/rules/03-deadline-tracker.json"
)

# ── 辅助函数：恢复单个文件（echo 内容到 stdout）────────────────────────────
#   输入: $1 = 相对 CoreRepo 的源路径
#   成功返回 0；失败返回非零（可同时调用者读取 stderr 描述）
restore_file() {
    local rel="$1"
    local content=""

    # 1) 优先从 Git HEAD
    content="$(cd "$CORE_REPO" && git show "HEAD:$rel" 2>/dev/null)" || true
    if [[ -n "$content" ]]; then
        printf '%s' "$content"
        return 0
    fi

    # 2) 回退到 FromBackup
    if [[ -n "$FROM_BACKUP" ]]; then
        local backup_abs="$FROM_BACKUP/$rel"
        if [[ -f "$backup_abs" ]]; then
            cat "$backup_abs"
            return 0
        fi
    fi

    return 1
}

# ── 创建目标目录 ──────────────────────────────────────────────────────────
mkdirs=(
    "$TARGET_ROOT/hospital/rules"
    "$TARGET_ROOT/law-firm/rules"
)
for d in "${mkdirs[@]}"; do
    if [[ $DRY_RUN -eq 1 ]]; then
        echo "[DRY-RUN] mkdir $d"
    else
        mkdir -p "$d"
    fi
done

# ── 逐文件迁移 ────────────────────────────────────────────────────────────
OK=0; SKIPPED=0; FAILED=0
TOTAL=${#MANIFEST[@]}

for line in "${MANIFEST[@]}"; do
    # 按空白把"源路径  目标路径"拆开（用 read + IFS 默认空白切分）
    read -r rel_core rel_dst <<< "$line"
    dst_abs="$TARGET_ROOT/$rel_dst"
    dst_dir="$(dirname "$dst_abs")"

    if [[ -f "$dst_abs" && $OVERWRITE -eq 0 ]]; then
        echo "[SKIP ] 目标已存在：$rel_dst（加 --overwrite 可强制覆盖）"
        SKIPPED=$((SKIPPED+1))
        continue
    fi

    tmpfile="$(mktemp)"
    if restore_file "$rel_core" >"$tmpfile"; then
        if [[ $DRY_RUN -eq 1 ]]; then
            echo "[DRY  ] $rel_dst  ←  git HEAD:$rel_core"
            rm -f "$tmpfile"
        else
            mkdir -p "$dst_dir"
            mv "$tmpfile" "$dst_abs"
            echo -e "\033[32m[ OK  ]\033[0m $rel_dst  ←  git HEAD:$rel_core"
            OK=$((OK+1))
        fi
    else
        rm -f "$tmpfile"
        echo -e "\033[31m[FAIL ]\033[0m 无法恢复 $rel_core（Git HEAD 中不存在，且未提供/未找到备份）" >&2
        FAILED=$((FAILED+1))
    fi
done

# ── 写迁移元数据 README ───────────────────────────────────────────────────
META_PATH="$TARGET_ROOT/README_MIGRATION.md"
write_meta=1
if [[ -f "$META_PATH" && $OVERWRITE -eq 0 ]]; then
    echo "[SKIP ] 迁移元数据已存在：README_MIGRATION.md（加 --overwrite 可重写）"
    write_meta=0
fi

if [[ $write_meta -eq 1 ]]; then
if [[ $DRY_RUN -eq 1 ]]; then
    echo "[DRY  ] 写迁移元数据 $META_PATH"
else
cat > "$META_PATH" <<'META_EOF'
<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule (evorule-application 应用层资产),
  licensed under GNU Affero General Public License v3 or later.
-->

# evorule-cli 业务规则模板迁移元数据

> 本目录下的 `hospital/` 与 `law-firm/` 两套行业规则模板，是 **2026-07-30 越界清理操作**
> 中从 evorule 核心仓迁出的业务内容。对应 evorule 仓根 `AGENTS.md` 边界判断表：
> **「evorule-cli 加规则模板 = 业务内容，不是机制，放 evorule-application ❌」**

## 基本信息

| 字段 | 值 |
|---|---|
| 迁移完成日期 | 2026-07-30 |
| 原位置（evorule 核心仓） | `evorule-cli/examples/hospital/`、`evorule-cli/examples/law-firm/` |
| 新位置（evorule-application 仓） | `examples/evorule-cli/hospital/`、`examples/evorule-cli/law-firm/` |
| 迁移文件数 | 10（hospital 5 + law-firm 5，含 rules/ 下 3×2 JSON，payload 样例 1×2，README 1×2） |
| 原协议 | AGPL-3.0-or-later（与 evorule 核心仓一致） |
| 恢复方式 | `git show HEAD:evorule-cli/examples/...`（核心仓 HEAD 提交快照） |

## 关联留痕（evorule 核心仓内）

1. **CHANGELOG.md v0.1.0「走神 9 拆分（机制-应用边界清理）」段**
2. **evorule-cli/README.md 顶部「业务规则模板位置说明」横幅**
3. **evorule-cli/examples/README.md 尾部「留痕声明」**

## 规则二警告确认流程（AGENTS.md §规则二）

本迁移在执行前已完成规则二警告流程：
- ❗ 对应硬规则违反：「evorule-cli 加规则模板 = 业务内容，放 evorule-application ❌」
- ❗ 负面影响：①破坏机制层纯净性；②模糊机制-策略边界；③未来业务规则改动会拉核心仓发布节奏；④与 evorule-application 方案集合重叠
- ✅ 项目方明确二次确认（2026-07-30 决策「业务规则迁移」）
- ✅ 本文件即为「记录留痕」要求的落地产物

## 维护注意

- 今后对这两套规则的**任何业务改动**，都**只在本仓（evorule-application）提交**，
  **不得回迁到 evorule 核心仓**。
- 如需新增其他行业规则（金融 AML、政务数据分级等），同样直接放在
  `examples/evorule-cli/<行业名>/` 目录下，遵循本目录现有结构。
META_EOF
    echo -e "\033[32m[ OK  ]\033[0m 写迁移元数据 README_MIGRATION.md"
fi
fi

# ── 结束报告 ───────────────────────────────────────────────────────────────
echo
echo "─────────────────────────────────────────────────────────────────────────"
echo "迁移结果汇总：OK=$OK  SKIPPED=$SKIPPED  FAILED=$FAILED（共 $TOTAL 个业务规则文件）"
echo "落位目录：$TARGET_ROOT"
echo "─────────────────────────────────────────────────────────────────────────"
echo
echo "下一步建议："
echo "  1. 进入 $APP_REPO，检查落位文件：git status --short examples/evorule-cli/"
echo "  2. 在 evorule-application 仓单独提交（不要混进核心仓的 commit）"
echo "  3. 可选：把本脚本拷贝到 evorule-application/scripts/ 下作为维护资产"

if [[ $FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
