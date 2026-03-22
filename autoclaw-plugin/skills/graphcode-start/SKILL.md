---
name: graphcode-start
description: Bootstrap the knowledge graph by indexing all code and generating .claude/rules/. Run on first use or to refresh.
disable-model-invocation: true
allowed-tools: Bash, Read
---

# Bootstrap Knowledge Graph

Run these steps in order:

1. **Bootstrap** (indexes all code with tree-sitter, 0 LLM tokens):
   ```bash
   graphcode init
   ```

2. **Verify**:
   ```bash
   graphcode stats
   ```

3. Report the results to the user: nodes, edges, rule files generated.

That's it. The `init` command does bootstrap + sync-rules automatically.
If `graphcode` is not found, try: `~/.graphcode/bin/graphcode init` or run the setup script: `bash "${CLAUDE_PLUGIN_ROOT}/scripts/setup.sh"`
