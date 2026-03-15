#!/usr/bin/env python3
"""
End-to-end demo: Build a knowledge graph from a Wikipedia article.

This script fetches the "Artificial Intelligence" Wikipedia article,
uses Claude to extract entities and relations, and builds a navigable
knowledge graph — all with zero external API keys beyond your existing
Claude setup.

Usage:
    source .venv/bin/activate
    python examples/demo.py
"""

import json
import os
import subprocess
import sys

# ── Fetch Wikipedia article ─────────────────────────────────────

def fetch_wikipedia(title: str, max_chars: int = 15000) -> str:
    """Fetch a Wikipedia article via the public API."""
    import urllib.request
    url = (
        f"https://en.wikipedia.org/w/api.php?"
        f"action=query&titles={title}&prop=extracts"
        f"&explaintext=true&format=json"
    )
    req = urllib.request.Request(url, headers={"User-Agent": "autoclaw-demo/0.1"})
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read())
    pages = data["query"]["pages"]
    for page in pages.values():
        text = page.get("extract", "")
        return text[:max_chars]
    return ""


# ── LLM call (uses Claude via CLI) ──────────────────────────────

def ask_claude(prompt: str) -> str:
    """Call Claude via the claude CLI. No API key needed if Claude Code is installed."""
    result = subprocess.run(
        ["claude", "-p", prompt, "--output-format", "json"],
        capture_output=True, text=True, timeout=120,
    )
    if result.returncode != 0:
        print(f"Claude error: {result.stderr}", file=sys.stderr)
        sys.exit(1)

    response = json.loads(result.stdout)
    return response.get("result", "")


def extract_json(text: str) -> dict:
    """Extract JSON from LLM response (handles markdown code blocks)."""
    import re
    # Try to find JSON in code blocks
    match = re.search(r"```(?:json)?\s*\n?(.*?)\n?\s*```", text, re.DOTALL)
    if match:
        text = match.group(1)
    # Try to find JSON object
    match = re.search(r"\{.*\}", text, re.DOTALL)
    if match:
        try:
            return json.loads(match.group(0))
        except json.JSONDecodeError:
            pass
    # Try raw parse
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        print(f"Failed to parse JSON from response:\n{text[:500]}", file=sys.stderr)
        return {"entities": [], "relations": []}


# ── Main ─────────────────────────────────────────────────────────

def main():
    from autoclaw import PyKnowledgeGraph as KnowledgeGraph

    print("=" * 60)
    print("  autoclaw Demo - Wikipedia → Knowledge Graph")
    print("=" * 60)

    # 1. Fetch article
    print("\n[1/4] Fetching Wikipedia article: Artificial Intelligence...")
    text = fetch_wikipedia("Artificial_intelligence")
    print(f"  Got {len(text)} characters")

    # 2. Create KG and analyze content for ontology
    kg = KnowledgeGraph("/tmp/demo_ai.kg")

    print("\n[2/4] Asking Claude to suggest ontology...")
    ontology_prompt = kg.analyze_content(text[:6000])
    ontology_response = ask_claude(ontology_prompt)
    ontology_json = extract_json(ontology_response)
    kg.update_ontology(json.dumps(ontology_json))
    print(f"  Domain: {ontology_json.get('domain', 'unknown')}")
    print(f"  Entity types: {len(ontology_json.get('suggested_entity_types', []))}")
    print(f"  Relation types: {len(ontology_json.get('suggested_relation_types', []))}")

    # 3. Extract entities and relations (chunk the text)
    print("\n[3/4] Extracting entities and relations...")
    chunk_size = 4000
    chunks = [text[i:i+chunk_size] for i in range(0, len(text), chunk_size)]
    total_added = 0
    total_merged = 0
    total_edges = 0

    for i, chunk in enumerate(chunks):
        print(f"  Chunk {i+1}/{len(chunks)}...", end=" ", flush=True)
        extraction_prompt = kg.prepare_extraction(chunk)
        extraction_response = ask_claude(extraction_prompt)
        extraction_json = extract_json(extraction_response)
        report = kg.ingest_document(
            json.dumps(extraction_json),
            "wikipedia_ai",
            page=i+1,
        )
        total_added += report["added"]
        total_merged += report["merged"]
        total_edges += report["edges_added"]
        print(f"+{report['added']} entities, +{report['edges_added']} edges"
              + (f", merged {report['merged']}" if report['merged'] > 0 else ""))

    print(f"\n  Total: {total_added} added, {total_merged} merged, {total_edges} edges")

    # 4. Save and demonstrate navigation
    kg.save()
    print(f"\n[4/4] Knowledge graph saved to /tmp/demo_ai.kg")

    # ── Navigation demo ──────────────────────────────────────────
    print("\n" + "=" * 60)
    print("  Navigation Demo")
    print("=" * 60)

    # Stats
    stats = json.loads(kg.stats())
    print(f"\nGraph: {stats['node_count']} nodes, {stats['edge_count']} edges")
    print(f"Node types: {stats['node_types']}")

    # Topics
    topics = json.loads(kg.topics())
    print(f"\nTopics:")
    for type_name, entities in topics.items():
        print(f"  {type_name}: {', '.join(entities[:5])}"
              + (f" (+{len(entities)-5} more)" if len(entities) > 5 else ""))

    # Explore some entities
    for name in ["artificial intelligence", "machine learning", "neural network",
                  "Alan Turing", "deep learning"]:
        result = kg.explore(name)
        if result:
            data = json.loads(result)
            entity = data["entity"]
            rels = data["relations"]
            print(f"\n  {entity['name']} ({entity['node_type']})")
            print(f"    {entity['definition'][:100]}")
            if rels:
                for r in rels[:3]:
                    dir_sym = "->" if r["direction"] == "Outgoing" else "<-"
                    print(f"    {dir_sym} [{r['relation_type']}] {r['node']['name']}")
                if len(rels) > 3:
                    print(f"    ... +{len(rels)-3} more connections")

    # Path finding
    print("\n  Path: machine learning → Alan Turing")
    path_str = kg.connect("machine learning", "Alan Turing")
    print(f"    {path_str}")

    print("\nDone!")


if __name__ == "__main__":
    main()
