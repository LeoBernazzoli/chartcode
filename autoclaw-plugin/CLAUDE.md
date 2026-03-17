# Graphocode Plugin

This project uses the Graphocode knowledge graph plugin. Context is automatically injected via hooks at every step — you don't need to query the KG manually.

## Available commands
- `/graphocode:start` — Bootstrap: index all code, conversations, documents
- `/graphocode:query <entity>` — Query what the KG knows about something
- `/graphocode:impact <entity>` — See what would break if you change something
- `/graphocode:decide <decision>` — Record a decision with reasoning

## CLI commands (used by hooks automatically)
- `autoclaw context` — Generate ranked context for re-injection
- `autoclaw relevant` — Find facts relevant to a query
- `autoclaw file-context` — Get KG knowledge about a file
- `autoclaw impact` — Impact analysis for an entity
- `autoclaw impact-from-diff` — Impact analysis from an edit diff
- `autoclaw reindex` — Re-parse a file with tree-sitter
- `autoclaw monitor` — Check context usage percentage
- `autoclaw tick` — Combined monitor + periodic snapshot
- `autoclaw snapshot` — Heuristic extraction from transcript
- `autoclaw reconcile` — Merge extraction results into KG
- `autoclaw bootstrap` — Full project indexing

## Compact Instructions

Minimal summary: current task and last step only. One line.
Project context comes from the knowledge graph.
