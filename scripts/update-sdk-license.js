// SPDX-License-Identifier: AGPL-3.0-or-later
// 批量更新 SDK 的 SPDX header 和 LICENSE
// —— SDK 是 evorule 核心的衍生作品，协议与核心保持一致 AGPL-3.0-or-later
//    详见 docs/oss_strategy.md §3 §3.1

const fs = require('fs');
const path = require('path');

const sdkRoots = [
    path.join(__dirname, '..', 'sdk', 'python'),
    path.join(__dirname, '..', 'sdk', 'typescript'),
];

// 不内嵌 AGPL 完整文本，直接读取仓根 LICENSE（与核心仓保持 100% 一致）
const agplLicenseText = fs.readFileSync(
    path.join(__dirname, '..', 'LICENSE'),
    'utf8'
);

// MD 文件的新 SPDX header（AGPL 版）
const agplMdHeader = `<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under the GNU Affero General
  Public License v3.0 or later. See /LICENSE in the repository root or
  <https://www.gnu.org/licenses/agpl-3.0.html>.
-->

`;

// MD 文件的旧 SPDX header（MIT 版）- 匹配整个 HTML 注释块（包含完整 MIT 文本）
const mitMdHeaderPattern = /^<!--\s*\n[\s\S]*?SPDX-License-Identifier:\s*MIT[^\n]*\s*\n-->\n*/;

// Python 文件的 SPDX header
const pySpdxPattern = /^#\s*SPDX-License-Identifier:[^\n]*\n/;

// TypeScript 文件的 SPDX header
const tsSpdxPattern = /^\/\/\s*SPDX-License-Identifier:[^\n]*\n/;

function processFile(filePath) {
    const ext = path.extname(filePath);
    let content = fs.readFileSync(filePath, 'utf8');
    const originalContent = content;

    if (ext === '.md') {
        // 替换 MIT SPDX header 为 AGPL
        if (mitMdHeaderPattern.test(content)) {
            content = content.replace(mitMdHeaderPattern, agplMdHeader);
        }
    } else if (ext === '.py') {
        const newHeader = '# SPDX-License-Identifier: AGPL-3.0-or-later\n';
        if (pySpdxPattern.test(content)) {
            content = content.replace(pySpdxPattern, newHeader);
        } else {
            content = newHeader + content;
        }
    } else if (ext === '.ts') {
        const newHeader = '// SPDX-License-Identifier: AGPL-3.0-or-later\n';
        if (tsSpdxPattern.test(content)) {
            content = content.replace(tsSpdxPattern, newHeader);
        } else {
            content = newHeader + content;
        }
    }

    if (content !== originalContent) {
        fs.writeFileSync(filePath, content, 'utf8');
        return true;
    }
    return false;
}

function walkDir(dir, callback) {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            if (['node_modules', '__pycache__', 'dist', 'build', '.venv', 'venv', '.pytest_cache', '.hypothesis', '.mypy_cache'].includes(entry.name)) {
                continue;
            }
            walkDir(fullPath, callback);
        } else {
            callback(fullPath);
        }
    }
}

let totalChanged = 0;

for (const sdkRoot of sdkRoots) {
    const sdkName = path.basename(sdkRoot);
    console.log(`\n📦 处理: sdk/${sdkName}`);

    // SDK 目录可能还没创建（v0.x 早期版本常见），跳过而非崩溃
    if (!fs.existsSync(sdkRoot)) {
        console.log(`  ⚠️  跳过: 目录不存在（SDK 尚未初始化, 创建后再运行本脚本即可）。路径: ${sdkRoot}`);
        continue;
    }

    // 1. 替换 LICENSE 文件（直接复用仓根 LICENSE，保证与核心一致）
    const licensePath = path.join(sdkRoot, 'LICENSE');
    if (fs.existsSync(licensePath)) {
        fs.writeFileSync(licensePath, agplLicenseText, 'utf8');
        console.log(`  ✅ LICENSE → AGPL-3.0-or-later (复用仓根 LICENSE)`);
        totalChanged++;
    } else {
        console.log(`  ℹ️  SDK 根目录暂无 LICENSE 文件，跳过写入。`);
    }

    // 2. 批量更新 SPDX header
    walkDir(sdkRoot, (filePath) => {
        const ext = path.extname(filePath);
        if (['.py', '.ts', '.md'].includes(ext)) {
            if (path.basename(filePath) === 'LICENSE') return;
            const relPath = path.relative(sdkRoot, filePath);
            if (processFile(filePath)) {
                console.log(`  ✅ ${relPath}`);
                totalChanged++;
            }
        }
    });
}

console.log(`\n✅ 完成！共修改 ${totalChanged} 个文件`);
