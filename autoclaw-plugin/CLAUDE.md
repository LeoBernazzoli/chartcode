# Graphocode

This project has a knowledge graph with every function, class, field, and their cross-file dependencies indexed.

Before modifying code: the PreToolUse hook automatically shows which files depend on what you're changing.

When working on a file: `.claude/rules/` has path-specific context that loads automatically with dependency counts.

For manual queries:
- `graphcode impact <entity>` — what depends on this entity
- `graphcode explore <entity>` — navigate the knowledge graph
- `graphcode stats` — overview of the codebase graph

## Compact Instructions
Minimal summary: current task and last step only. One line.
