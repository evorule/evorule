// update-spdx.js - Update SPDX headers in markdown files

const fs = require('fs');
const path = require('path');

const ROOT = 'd:/evorule';

function readFileUtf8(filePath) {
  return fs.readFileSync(filePath, 'utf8');
}

function writeFileUtf8(filePath, content) {
  fs.writeFileSync(filePath, content, 'utf8');
}

// Find the first HTML comment block that contains SPDX-License-Identifier
// Returns { start, end, headerText } or null
function findSpdxBlock(content) {
  // Match HTML comment blocks: <!-- ... -->
  const commentRegex = /<!--([\s\S]*?)-->/g;
  let match;
  while ((match = commentRegex.exec(content)) !== null) {
    if (/SPDX-License-Identifier:/.test(match[1])) {
      return {
        start: match.index,
        end: match.index + match[0].length,
        text: match[0]
      };
    }
  }
  return null;
}

function convertToCCO(filePath, description) {
  let content = readFileUtf8(filePath);
  const cc0Header = `<!--
SPDX-License-Identifier: CC0-1.0
${description}
-->

`;

  const existing = findSpdxBlock(content);
  if (existing) {
    // Check if already CC0
    if (/CC0-1\.0/.test(existing.text)) {
      console.log('ALREADY CC0:', path.basename(filePath));
      return false;
    }
    // Replace existing SPDX block
    const before = content.slice(0, existing.start);
    const after = content.slice(existing.end);
    // Skip any blank lines after the old header
    const afterTrimmed = after.replace(/^\s*\n/, '');
    content = before + cc0Header + afterTrimmed;
    writeFileUtf8(filePath, content);
    console.log('CONVERTED TO CC0:', path.basename(filePath));
    return true;
  }

  // No existing header - add at top
  content = cc0Header + content;
  writeFileUtf8(filePath, content);
  console.log('ADDED CC0:', path.basename(filePath));
  return true;
}

function addAgplHeader(filePath) {
  let content = readFileUtf8(filePath);

  const existing = findSpdxBlock(content);
  if (existing) {
    console.log('ALREADY HAS SPDX:', path.basename(filePath));
    return false;
  }

  const agplHeader = `<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

`;

  content = agplHeader + content;
  writeFileUtf8(filePath, content);
  console.log('ADDED AGPL:', path.basename(filePath));
  return true;
}

// A-class: convert to CC0
const aClass = [
  { file: 'CODE_OF_CONDUCT.md', desc: 'Code of Conduct documents are community norms; we release them under CC0 for maximum adoption and reuse.' },
  { file: 'SECURITY.md', desc: 'Security disclosure procedures are public knowledge; we release them under CC0 so everyone knows how to report vulnerabilities safely.' },
  { file: 'docs/constitution.md', desc: 'Governance documents are the "constitution" of the project — they belong to the community and are released into the public domain.' },
  { file: 'docs/oss_strategy.md', desc: 'Open source strategy documents define the rules of engagement for the community; they are public norms, not proprietary assets.' },
  { file: 'docs/benchmarks/EVAL_2026-07-20.md', desc: 'Benchmark reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.' },
  { file: 'docs/benchmarks/EXP_1.1.md', desc: 'Benchmark reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.' },
  { file: 'docs/benchmarks/EXP_1.2.md', desc: 'Benchmark reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.' },
  { file: 'docs/benchmarks/EXP_1.3.md', desc: 'Benchmark reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.' },
  { file: 'docs/benchmarks/EXP_1.4.md', desc: 'Benchmark reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.' },
  { file: 'docs/benchmarks/EXP_1.5.md', desc: 'Benchmark reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.' },
  { file: 'tier0-tcb/tla/TLC_VERIFICATION_REPORT.md', desc: 'Formal verification reports are public artifacts; we release them under CC0 for maximum transparency and reproducibility.' },
];

// B-class: add AGPL SPDX header
const bClass = [
  'README.md',
  'AGENTS.md',
  'evorule-cli/CLI_SPEC.md',
  'tier0-tcb/docs/KANI.md',
  'tier0-tcb/docs/MUTANTS.md',
  'tier1-reactor/README.md',
  'tier2-governance/README.md',
  'evorule-cli/README.md',
  'evorule-cli/CHANGELOG.md',
  '.gitee/PULL_REQUEST_TEMPLATE.md',
  'tier0-tcb/TCB_SPEC.md',
  'tier1-reactor/REACTOR_SPEC.md',
  'tier2-governance/GOVERNANCE_SPEC.md',
  'EVORULE_FORMAL_VERTIFICATION_PLAN.md',
  'tier0-tcb/README.md',
];

console.log('=== A-Class: Converting to CC0 ===');
let aCount = 0;
for (const item of aClass) {
  const fullPath = path.join(ROOT, item.file);
  if (fs.existsSync(fullPath)) {
    if (convertToCCO(fullPath, item.desc)) aCount++;
  } else {
    console.log('MISSING:', item.file);
  }
}
console.log('A-class converted:', aCount);

console.log('');
console.log('=== B-Class: Adding AGPL SPDX ===');
let bCount = 0;
for (const relPath of bClass) {
  const fullPath = path.join(ROOT, relPath);
  if (fs.existsSync(fullPath)) {
    if (addAgplHeader(fullPath)) bCount++;
  } else {
    console.log('MISSING:', relPath);
  }
}
console.log('B-class added:', bCount);
