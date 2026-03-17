#!/bin/bash
# Graphocode plugin setup
# Disables Claude Code auto-compact — Graphocode manages the compaction lifecycle.

CLAUDE_JSON="${HOME}/.claude.json"

echo "Graphocode plugin setup"
echo "======================="

# 1. Disable auto-compact
if [ -f "$CLAUDE_JSON" ]; then
    if command -v jq &> /dev/null; then
        if ! grep -q "autoCompactEnabled" "$CLAUDE_JSON"; then
            tmp=$(mktemp)
            jq '. + {"autoCompactEnabled": false}' "$CLAUDE_JSON" > "$tmp" && mv "$tmp" "$CLAUDE_JSON"
            echo "[OK] Disabled auto-compact in $CLAUDE_JSON"
        else
            echo "[OK] autoCompactEnabled already configured in $CLAUDE_JSON"
        fi
    else
        echo "[WARN] jq not found. Please manually add '\"autoCompactEnabled\": false' to $CLAUDE_JSON"
    fi
else
    echo '{"autoCompactEnabled": false}' > "$CLAUDE_JSON"
    echo "[OK] Created $CLAUDE_JSON with auto-compact disabled"
fi

# 2. Verify autoclaw is on PATH
if command -v autoclaw &> /dev/null; then
    echo "[OK] autoclaw found: $(which autoclaw)"
else
    echo "[WARN] autoclaw not found on PATH. Install with: maturin develop"
fi

echo ""
echo "Setup complete. Run /graphocode:start to bootstrap the knowledge graph."
