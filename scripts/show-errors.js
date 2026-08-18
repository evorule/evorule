// SPDX-License-Identifier: AGPL-3.0-or-later
// show-errors.js - Show all markdownlint errors with file and line

const { execSync } = require('child_process');

const cmd = `npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**"`;

try {
  execSync(cmd, { cwd: 'd:/evorule', encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
} catch (e) {
  console.log(e.stdout);
  console.error(e.stderr);
}
