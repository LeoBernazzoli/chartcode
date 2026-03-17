#!/bin/bash
set -e

TRANSCRIPT_PATH="$1"
KG_PATH="${AUTOCLAW_KG:-./knowledge.kg}"

if [ -z "$TRANSCRIPT_PATH" ]; then
    echo "Usage: extract-and-compact.sh <transcript_path>" >&2
    exit 1
fi

# 1. Run heuristic snapshot first (instant, 0 tokens)
autoclaw snapshot "$TRANSCRIPT_PATH" --all-since-last 2>/dev/null || true

# 2. Tree-sitter refresh of recently modified files
# Find files modified since last commit
git diff --name-only HEAD 2>/dev/null | while read -r file; do
    if [ -f "$file" ] && [[ "$file" == *.rs ]]; then
        autoclaw reindex "$file" 2>/dev/null || true
    fi
done

# Also check unstaged changes
git diff --name-only 2>/dev/null | while read -r file; do
    if [ -f "$file" ] && [[ "$file" == *.rs ]]; then
        autoclaw reindex "$file" 2>/dev/null || true
    fi
done

# 3. Signal that deep extraction + compact is needed
# The kg-extractor agent hook handles the Haiku extraction.
# After extraction completes, /compact should be triggered with minimal instructions.
echo "EXTRACTION_COMPLETE" >&2
echo "Context threshold reached. Run deep extraction with kg-extractor agent, then /compact." >&2
