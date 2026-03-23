---
name: chartcode-decide
description: Record a decision in the knowledge graph with reasoning and alternatives considered
allowed-tools: Bash
---

# Record Decision

Parse the user's decision from $ARGUMENTS and persist it in the knowledge graph.

## Steps

1. Parse the decision from the user's input. Identify:
   - **What** was decided
   - **Why** (the reasoning)
   - **Alternatives** considered (if mentioned)
   - **Tier**: critical (architectural), significant (implementation), or minor (stylistic)
   - Whether this **supersedes** an existing decision

2. Create the reconcile JSON:
   ```json
   {
     "new_facts": [{
       "name": "<concise decision name>",
       "type": "Decision",
       "tier": "<critical|significant|minor>",
       "definition": "<what was decided>",
       "reason": "<why>",
       "supersedes": "<old decision name or null>",
       "relations": [],
       "evidence_text": "<user's original statement>"
     }],
     "superseded": [],
     "promotions": [],
     "relations": []
   }
   ```

3. If this supersedes an old decision, add it to the `superseded` array:
   ```json
   "superseded": [{"old": "<old decision name>", "reason": "<why replaced>"}]
   ```

4. Pipe to reconcile:
   ```bash
   echo '<json>' | chartcode reconcile
   ```

5. Confirm to the user that the decision has been recorded.
