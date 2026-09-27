#!/usr/bin/env bash
# Scan the tracked working tree for .secret-patterns.txt matches.
#
# This is the CI-side consumer of the pattern file (the hooks scan diffs; CI
# scans the tree, and also catches anything committed with --no-verify).
# Exit 0 clean, 1 findings.
#
# Deliberately NOT a git-history scan: this fork's history still contains the
# pre-fix keys.json blob (issue #134 history scrub is tracked separately), so a
# history gate here would be permanently red and would train people to ignore
# the job. The tip is what must stay clean.
set -e

ROOT="$(git rev-parse --show-toplevel)"
PATTERNS_FILE="${1:-$ROOT/.secret-patterns.txt}"

if [ ! -f "$PATTERNS_FILE" ]; then
    echo "no patterns file at $PATTERNS_FILE — nothing to do"
    exit 0
fi

FOUND=0
while IFS= read -r pattern; do
    [ -z "$pattern" ] && continue
    case "$pattern" in \#*) continue ;; esac
    MATCH=$(git grep -E -n -I -e "$pattern" -- . 2>/dev/null | head -5 || true)
    if [ -n "$MATCH" ]; then
        echo "SECRET PATTERN in tracked tree (pattern: $pattern):"
        echo "$MATCH"
        FOUND=1
    fi
done < "$PATTERNS_FILE"

if [ $FOUND -eq 1 ]; then
    echo ""
    echo "FAIL — secret pattern(s) present in the tracked tree."
    exit 1
fi

echo "OK — no secret pattern match in the tracked tree."
exit 0
