---
name: graphcode-impact
description: Run impact analysis to see what would be affected by changing an entity
disable-model-invocation: true
allowed-tools: Bash
---

# Impact Analysis

Shows all references to an entity and detects potential breaking changes before you make modifications.

## Usage

```bash
autoclaw impact "$ARGUMENTS" --depth 2
```

Present the full impact report to the user, highlighting any breaking changes.
