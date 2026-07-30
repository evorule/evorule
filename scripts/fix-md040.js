// fix-md040.js - Auto-detect and add language to fenced code blocks
// Uses the markdownlint API approach: read files, find ``` without language,
// detect language, and add it.

const fs = require('fs');
const path = require('path');

function detectLanguage(code) {
  const trimmed = code.trim();
  if (!trimmed) return 'text';

  const firstLine = trimmed.split('\n')[0].trim();
  const allText = code;

  // JSON
  if (/^[\{\[]/.test(firstLine) && /"[^"]+"\s*:/.test(allText)) return 'json';

  // Rust
  if (/\bfn\s+\w+\s*\(/.test(allText)) return 'rust';
  if (/\blet\s+mut\b/.test(allText)) return 'rust';
  if (/\bimpl\s+\w+/.test(allText)) return 'rust';
  if (/\buse\s+std::/.test(allText)) return 'rust';

  // Shell/bash
  if (/^\$\s+/.test(firstLine)) return 'bash';
  if (/cargo\s+(run|test|build|check|clippy)/.test(allText)) return 'bash';

  // Python
  if (/\bdef\s+\w+\s*\(/.test(allText)) return 'python';
  if (/print\(/.test(allText)) return 'python';

  // TypeScript/JS
  if (/\bconst\s+\w+\s*=/.test(allText)) return 'typescript';
  if (/\bfunction\s+\w+\s*\(/.test(allText)) return 'typescript';

  // YAML
  if (/^\w[\w-]*\s*:\s*/.test(firstLine) && /\n\s+\w[\w-]*\s*:/.test(allText)) return 'yaml';

  // TOML
  if (/^\[[\w\.]+\]$/m.test(allText) && /\w+\s*=\s*"/.test(allText)) return 'toml';

  // ASCII diagram (box-drawing chars)
  if (/[┌─┐└┘│├┤┬┴╔╗╚╝║═]/.test(allText)) return 'text';
  if (/^\s+[├└]──/m.test(allText)) return 'text';

  // Lots of pipes = table/diagram
  const pipeCount = (allText.match(/\|/g) || []).length;
  if (pipeCount > 5) return 'text';

  return 'text';
}

function processFile(filePath) {
  const content = fs.readFileSync(filePath, 'utf8');
  const lines = content.split('\n');
  const result = [];
  let inCode = false;
  let codeStart = -1;
  let blockCount = 0;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Fenced code block start with no language
    if (!inCode && /^\s*```\s*$/.test(line)) {
      inCode = true;
      codeStart = result.length;
      result.push(line);
      continue;
    }

    // Fenced code block end
    if (inCode && /^\s*```\s*$/.test(line)) {
      inCode = false;
      const codeLines = result.slice(codeStart + 1);
      const code = codeLines.join('\n');
      const lang = detectLanguage(code);
      const indent = line.match(/^(\s*)/)[1];
      result[codeStart] = indent + '```' + lang;
      result.push(line);
      blockCount++;
      continue;
    }

    result.push(line);
  }

  if (blockCount > 0) {
    fs.writeFileSync(filePath, result.join('\n'), 'utf8');
    console.log('UPDATED:', path.basename(filePath), '(' + blockCount + ' blocks)');
  }
  return blockCount;
}

// Main
const targetFiles = [
  'evorule-tcb/tla/TLC_VERIFICATION_REPORT.md',
  'docs/security/SECURITY_AUDIT_v1.0.0.md',
  'docs/security/SECURITY_AUDIT_v0.1.0.md',
  'docs/benchmarks/EXP_1.5.md',
  'docs/security/DEPENDENCY_AUDIT_v1.0.0.md',
  'CONTRIBUTING_ZH.md',
  'docs/security/DEPENDENCY_AUDIT_v0.1.0.md',
  'ROADMAP.md',
  'VERSION_STRATEGY.md',
  'docs/security/THREAT_MODEL.md',
  'CONTRIBUTING.md',
  'README.md',
  'docs/benchmarks/EXP_1.4.md',
  'evorule-cli/README.md',
  'docs/benchmarks/EXP_1.2.md',
  'docs/benchmarks/EXP_1.3.md',
  'evorule-tcb/README.md',
  'evorule-governance/README.md',
];

const rootDir = 'd:/evorule';
let totalFixed = 0;

for (const relPath of targetFiles) {
  const fullPath = path.join(rootDir, relPath);
  if (fs.existsSync(fullPath)) {
    totalFixed += processFile(fullPath);
  } else {
    console.log('MISSING:', relPath);
  }
}

console.log('---');
console.log('Total code blocks fixed:', totalFixed);
