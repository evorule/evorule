#!/usr/bin/env python3
# =============================================================================
# check_doc_safety.py - 校验 git staged 文件不含 文档/ 路径
#
# 内部规则 2026-07-15: evorule/文档/ = 内部工作区. 不 git add, 不提交不发布.
#
# 用法:
#   python scripts/check_doc_safety.py            # 校验当前 staged
#   python scripts/check_doc_safety.py --all      # 校验 + 历史 (drift)
#   python scripts/check_doc_safety.py --json     # JSON 输出 (CI 用)
#
# 退出码: 0 = 干净, 1 = 发现违规, 2 = git 命令失败
# =============================================================================

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

# 匹配 文档/ 路径: 行首含 \ or / + 文档 + 结尾 (/, \, or 行尾)
DOCS_PATTERN = re.compile(r'(^|[\\/])文档($|[\\/])')

RULE_TEXT = "USER 规则 2026-07-15: 文档/ 是内部文件, 禁止 commit"

def run(cmd, cwd=None):
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    return r.stdout, r.stderr, r.returncode

def get_staged_files(cwd):
    # 用 -z 避免 core.quotepath 把 UTF-8 路径 octal-escape
    out, _, rc = run(['git', 'diff', '-z', '--cached', '--name-only', '--diff-filter=ACMRT'], cwd=cwd)
    if rc != 0:
        return None
    return [f for f in out.split('\0') if f]

def check_staged(cwd):
    staged = get_staged_files(cwd)
    if staged is None:
        return None
    return [f for f in staged if DOCS_PATTERN.search(f)]

def check_history(cwd):
    # ls-files: 用 -z 避免 quoting
    out, _, _ = run(['git', 'ls-files', '-z', '--', '文档/'], cwd=cwd)
    tracked = [f for f in out.split('\0') if f]
    # log --all: 用 -z
    out, _, _ = run(['git', 'log', '-z', '--all', '--pretty=format:', '--name-only',
                     '--diff-filter=A', '--', '文档/'], cwd=cwd)
    historical = [f for f in out.split('\0') if f]
    return tracked + historical

def main():
    p = argparse.ArgumentParser(description='校验 git staged 不含 文档/ 路径')
    p.add_argument('--all', action='store_true', help='同时检查 git history')
    p.add_argument('--json', action='store_true', help='JSON 输出')
    p.add_argument('--cwd', default='.', help='git repo 路径')
    args = p.parse_args()
    cwd = str(Path(args.cwd).resolve())

    result = {
        'rule': RULE_TEXT,
        'staged_clean': True,
        'staged_violations': [],
        'history_clean': True,
        'history_violations': [],
    }

    violations = check_staged(cwd)
    if violations is None:
        if args.json:
            print(json.dumps({'error': 'git command failed'}, ensure_ascii=False))
        else:
            print('❌ git command failed (cwd 不是 git repo?)', file=sys.stderr)
        sys.exit(2)
    result['staged_violations'] = violations
    result['staged_clean'] = not violations

    if args.all:
        hist = check_history(cwd)
        result['history_violations'] = hist
        result['history_clean'] = not hist

    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        if not result['staged_clean']:
            print('❌ STAGED CHECK FAILED: 检测到 文档/ 文件被 staged', file=sys.stderr)
            print(f"   {RULE_TEXT}", file=sys.stderr)
            print('', file=sys.stderr)
            print('违规文件:', file=sys.stderr)
            for v in violations:
                print(f'   {v}', file=sys.stderr)
            print('', file=sys.stderr)
            print('可能原因:', file=sys.stderr)
            print('  - git add . 时 .gitignore 失效 (检查是否被 -f 强制)', file=sys.stderr)
            print('  - 手动 git add 文档/<file>', file=sys.stderr)
            print('  - 误从别处 cp 过来 (含 文档/ 前缀的相对路径)', file=sys.stderr)
            sys.exit(1)

        print('✓ STAGED 干净: 无 文档/ 文件被 staged')

        if args.all:
            if not result['history_clean']:
                print('❌ HISTORY DRIFT:', file=sys.stderr)
                for v in result['history_violations']:
                    print(f'   {v}', file=sys.stderr)
                sys.exit(1)
            print('✓ HISTORY 干净: 无 文档/ 文件曾被 commit')

        sys.exit(0)

if __name__ == '__main__':
    main()
