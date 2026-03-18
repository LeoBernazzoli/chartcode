# Graphocode Plugin v2

Knowledge graph engine with complete reference graph. Context delivered via auto-generated `.claude/rules/` — not hook injection.

## How it works
- SessionStart: `autoclaw sync-rules` generates `.claude/rules/` from KG
- PreToolUse(Edit|Write): impact analysis with pattern-grouped report (additionalContext JSON)
- PostToolUse(Edit|Write): `autoclaw reindex` updates reference graph
- Stop: `autoclaw snapshot` extracts decisions from transcript

## Commands
- `/graphocode:start` — Bootstrap: index all code + conversations
- `/graphocode:query <entity>` — Query the KG
- `/graphocode:impact <entity>` — Impact analysis before modifications
- `/graphocode:decide <decision>` — Record a decision

## CLI
- `autoclaw sync-rules` — Regenerate .claude/rules/ from KG
- `autoclaw impact <entity>` — Show all references + breaking changes
- `autoclaw bootstrap` — Full project indexing
- `autoclaw explore <entity>` — Navigate the KG

## Compact Instructions
Minimal summary: current task and last step only. One line.
Project context comes from the knowledge graph via .claude/rules/.
