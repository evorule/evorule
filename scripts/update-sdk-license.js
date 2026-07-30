// SPDX-License-Identifier: MIT
// 批量更新 SDK 的 SPDX header 和 LICENSE

const fs = require('fs');
const path = require('path');

const sdkRoots = [
    path.join(__dirname, '..', 'sdk', 'python'),
    path.join(__dirname, '..', 'sdk', 'typescript'),
];

const mitLicenseText = `MIT License

Copyright (c) 2026 EvoRule Project

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
`;

// MD 文件的新 SPDX header（MIT 版）
const mitMdHeader = `<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: MIT
-->

`;

// MD 文件的旧 SPDX header（AGPL 版）- 匹配整个 HTML 注释块（包含完整 AGPL 文本）
const agplMdHeaderPattern = /^<!--\s*\n[\s\S]*?SPDX-License-Identifier:\s*AGPL-[^\n]*\s*\n-->\n*/;

// Python 文件的 SPDX header
const pySpdxPattern = /^#\s*SPDX-License-Identifier:[^\n]*\n/;

// TypeScript 文件的 SPDX header
const tsSpdxPattern = /^\/\/\s*SPDX-License-Identifier:[^\n]*\n/;

function processFile(filePath) {
    const ext = path.extname(filePath);
    let content = fs.readFileSync(filePath, 'utf8');
    const originalContent = content;

    if (ext === '.md') {
        // 替换 AGPL SPDX header 为 MIT
        if (agplMdHeaderPattern.test(content)) {
            content = content.replace(agplMdHeaderPattern, mitMdHeader);
        }
    } else if (ext === '.py') {
        if (pySpdxPattern.test(content)) {
            content = content.replace(pySpdxPattern, '# SPDX-License-Identifier: MIT\n');
        } else {
            content = '# SPDX-License-Identifier: MIT\n' + content;
        }
    } else if (ext === '.ts') {
        if (tsSpdxPattern.test(content)) {
            content = content.replace(tsSpdxPattern, '// SPDX-License-Identifier: MIT\n');
        } else {
            content = '// SPDX-License-Identifier: MIT\n' + content;
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

    // 1. 替换 LICENSE 文件
    const licensePath = path.join(sdkRoot, 'LICENSE');
    if (fs.existsSync(licensePath)) {
        fs.writeFileSync(licensePath, mitLicenseText, 'utf8');
        console.log(`  ✅ LICENSE → MIT`);
        totalChanged++;
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
