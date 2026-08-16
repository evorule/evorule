// show-md051.js - Show MD051 errors

const { execSync } = require('child_process');

const cmd = `npx --yes markdownlint-cli "docs/**/*.md" "*.md" "evorule-tcb/**/*.md" "evorule-reactor/**/*.md" "evorule-governance/**/*.md" "evorule-cli/**/*.md" ".gitee/*.md" --ignore "**/node_modules/**" --ignore "_PRIVATE_zh_docs/**" --ignore ".trae/**" --ignore ".gate-logs/**"`;

try {
  execSync(cmd, { cwd: 'd:/evorule', encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
} catch (e) {
  const lines = e.stdout.split('\n');
  for (const line of lines) {
    if (line.includes('MD051')) {
      console.log(line);
    }
  }
}
