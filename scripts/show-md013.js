// show-md013.js - Show MD013 errors with file and line

const { execSync } = require('child_process');

const cmd = `npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**"`;

try {
  execSync(cmd, { cwd: 'd:/evorule', encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
} catch (e) {
  const lines = e.stdout.split('\n');
  const files = {};
  for (const line of lines) {
    const m = line.match(/^(.+?):(\d+).*MD013.*Actual: (\d+)/);
    if (m) {
      const f = m[1];
      const len = parseInt(m[3]);
      if (!files[f]) files[f] = [];
      files[f].push({ line: m[2], len });
    }
  }
  for (const f of Object.keys(files).sort()) {
    const errs = files[f];
    const maxLen = Math.max(...errs.map(e => e.len));
    console.log(`${errs.length}\t${maxLen}\t${f}`);
  }
  console.log('---');
  const total = Object.values(files).reduce((s, arr) => s + arr.length, 0);
  console.log('Total MD013 errors:', total);
}
