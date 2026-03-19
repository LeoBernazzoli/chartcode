#!/bin/bash
# Graphocode setup — runs at SessionStart. Must be FAST (<100ms).

# 1. Find autoclaw binary
if command -v autoclaw &>/dev/null; then
    BIN="autoclaw"
elif [ -x "${CLAUDE_PLUGIN_DATA}/bin/autoclaw" ]; then
    BIN="${CLAUDE_PLUGIN_DATA}/bin/autoclaw"
else
    echo "Graphocode: autoclaw not found. Run: cargo install autoclaw" >&2
    exit 0
fi

# 2. If no KG exists, user needs to run init
KG_PATH="${AUTOCLAW_KG:-./knowledge.kg}"
if [ ! -f "$KG_PATH" ]; then
    echo "Graphocode: no knowledge graph found. Run: autoclaw init" >&2
    exit 0
fi

# 3. If rules already exist, skip (fast path — no KG load)
if [ -d ".claude/rules" ] && [ "$(ls .claude/rules/ 2>/dev/null | wc -l)" -gt 2 ]; then
    exit 0
fi

# 4. Only sync-rules if rules don't exist yet (slow path — loads KG)
"$BIN" sync-rules 2>/dev/null
echo "Graphocode: rules generated." >&2
