---
name: graphocode-start
description: Bootstrap the knowledge graph by indexing all code, conversations, and documents. Run this when starting on a new project or to refresh the index.
disable-model-invocation: true
allowed-tools: Bash, Read
---

# Bootstrap Knowledge Graph

Full project indexing. This scans all source code with tree-sitter (deterministic, 0 LLM tokens), parses past Claude Code conversations, and optionally processes business documents.

## Steps

1. **Run setup** (first time only):
   ```bash
   bash autoclaw-plugin/scripts/setup.sh
   ```

2. **Run bootstrap**:
   ```bash
   autoclaw bootstrap --config graphocode.toml
   ```

3. **Report results** to the user:
   - How many files indexed
   - How many code entities extracted
   - How many conversations found
   - Whether Haiku extraction is needed for conversations

4. **If conversations were found**, the bootstrap outputs text ready for Haiku semantic extraction. For each conversation text, use the kg-extractor agent to extract semantic knowledge, then pipe the result to `autoclaw reconcile`.

5. **Verify** the KG is working:
   ```bash
   autoclaw stats
   autoclaw context 500
   ```
