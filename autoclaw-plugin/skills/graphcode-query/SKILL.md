---
name: graphcode-query
description: Query the knowledge graph about entities, decisions, or relationships in the project
allowed-tools: Bash
---

# Query Knowledge Graph

Use the autoclaw CLI to find information about the requested entity or topic.

## How to query

1. **Explore a specific entity**:
   ```bash
   autoclaw explore "$ARGUMENTS"
   ```

2. **Find relevant facts for a broader topic**:
   ```bash
   autoclaw relevant "$ARGUMENTS" --budget 1000
   ```

3. **Get file-specific context**:
   ```bash
   autoclaw file-context "$ARGUMENTS" --budget 500
   ```

4. **Find connections between two entities**:
   ```bash
   autoclaw connect "entity_a" "entity_b"
   ```

Present results in a readable format to the user.
