#!/usr/bin/env python3
"""
Test ingestion A: ingest Claude Code conversations from this project
into a knowledge graph.

Usage:
    source .venv/bin/activate
    python examples/test_ingest_project.py
"""

import json
import os
import subprocess
import sys
import time
import re

def ask_claude(prompt: str) -> str:
    result = subprocess.run(
        ["claude", "-p", prompt, "--output-format", "json", "--model", "sonnet"],
        capture_output=True, text=True, timeout=300,
    )
    if result.returncode != 0:
        return '{"entities":[],"relations":[]}'
    try:
        response = json.loads(result.stdout)
        return response.get("result", "")
    except json.JSONDecodeError:
        return result.stdout


def extract_json(text: str) -> dict:
    match = re.search(r"```(?:json)?\s*\n?(.*?)\n?\s*```", text, re.DOTALL)
    if match:
        text = match.group(1)
    match = re.search(r"\{.*\}", text, re.DOTALL)
    if match:
        try:
            return json.loads(match.group(0))
        except json.JSONDecodeError:
            pass
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {"entities": [], "relations": []}


def main():
    from autoclaw import (
        PyKnowledgeGraph as KnowledgeGraph,
        parse_claude_project,
        get_claude_conversation_text,
    )

    PROJECT = "/Users/leobernazzoli/autoclaw"
    KG_PATH = "/tmp/test_project_ingest.kg"

    print("=" * 60)
    print("  Project Ingestion Test: autoclaw")
    print("=" * 60)

    # Clean start
    if os.path.exists(KG_PATH):
        os.unlink(KG_PATH)
    kg = KnowledgeGraph(KG_PATH)

    # 1. List conversations
    convs = parse_claude_project(PROJECT)
    print(f"\nFound {len(convs)} conversations")

    # Filter: skip 1-2 message automated calls (claude -p from tests)
    substantive = [(sid, count, preview) for sid, count, preview in convs
                   if count > 3 and not preview.strip().startswith("[User]: Extract entities")
                   and not preview.strip().startswith("[User]: Analyze this text")]
    print(f"Substantive (>3 msgs, non-automated): {len(substantive)}")

    # Take top 5 longest conversations for test
    substantive.sort(key=lambda x: -x[1])
    to_process = substantive[:5]

    for sid, count, preview in to_process:
        print(f"  [{sid[:8]}] {count} msgs: {preview[:80]}...")

    # 2. Analyze ontology from first conversation
    print(f"\n[1] Analyzing ontology from conversations...")
    first_text = get_claude_conversation_text(PROJECT, to_process[0][0], 6000, "user")
    if not first_text:
        print("  ERROR: no text from first conversation")
        return

    ontology_prompt = kg.analyze_content(first_text)
    ontology_response = ask_claude(ontology_prompt)
    ontology_json = extract_json(ontology_response)
    kg.update_ontology(json.dumps(ontology_json))
    print(f"  Domain: {ontology_json.get('domain', '?')}")
    print(f"  Entity types: {[t['name'] for t in ontology_json.get('suggested_entity_types', [])]}")

    # 3. Process each conversation
    print(f"\n[2] Extracting from {len(to_process)} conversations...")
    total_start = time.time()

    for i, (sid, count, _) in enumerate(to_process):
        print(f"\n  Conversation {i+1}/{len(to_process)} [{sid[:8]}] ({count} msgs)")
        start = time.time()

        text = get_claude_conversation_text(PROJECT, sid, 50000, "user")
        if not text:
            print("    SKIP: empty")
            continue

        chunks = kg.chunk_text(text, 4000, 500)
        if not chunks:
            chunks = [text[:4000]]
        print(f"    {len(text)} chars, {len(chunks)} chunks")

        conv_added = 0
        conv_merged = 0
        conv_edges = 0

        for j, chunk in enumerate(chunks):
            prompt = kg.prepare_extraction(chunk)
            response = ask_claude(prompt)
            result = extract_json(response)
            report = kg.ingest_document(
                json.dumps(result),
                f"conversation_{sid[:8]}",
                page=j+1,
            )
            conv_added += report["added"]
            conv_merged += report["merged"]
            conv_edges += report["edges_added"]

        elapsed = time.time() - start
        print(f"    +{conv_added} entities, +{conv_edges} edges, "
              f"{conv_merged} merged ({elapsed:.0f}s)")

    # 4. Post-process
    print(f"\n[3] Post-processing...")
    orphans = kg.connect_orphans()
    discoveries = kg.discover_connections()
    print(f"  Orphan connections: {orphans}")
    print(f"  Cross-conversation discoveries: {discoveries}")

    kg.save()
    total_elapsed = time.time() - total_start

    # 5. Quality
    print(f"\n{'=' * 60}")
    print(f"  Results")
    print(f"{'=' * 60}")

    q = json.loads(kg.quality_metrics())
    stats = json.loads(kg.stats())

    print(f"\n  Total time: {total_elapsed:.0f}s")
    print(f"  Nodes: {q['total_nodes']}")
    print(f"  Edges: {q['total_edges']}")
    print(f"  Documents: {stats['document_count']}")
    print(f"  Orphan ratio: {q['orphan_ratio']:.1%}")
    print(f"  related_to ratio: {q['related_to_ratio']:.1%}")
    print(f"  Avg degree: {q['avg_degree']:.1f}")

    print(f"\n  Node types:")
    for t, count in sorted(stats["node_types"].items(), key=lambda x: -x[1]):
        print(f"    {t}: {count}")

    # 6. Navigation tests
    print(f"\n{'=' * 60}")
    print(f"  Navigation Tests")
    print(f"{'=' * 60}")

    # Key things that should be in the KG from our conversations
    test_lookups = [
        "knowledge graph",
        "Rust",
        "Python",
        "entity resolution",
        "Claude Code",
        "RAG",
        "VOPA",
        "ontology",
        "SQLite",
        "FalkorDB",
        "SDK",
        "PyO3",
        "MessagePack",
    ]

    found = 0
    for name in test_lookups:
        result = kg.explore(name)
        if result:
            data = json.loads(result)
            n_rels = len(data["relations"])
            print(f"  ✓ '{name}' -> {data['entity']['name']} ({data['entity']['node_type']}) "
                  f"- {n_rels} connections")
            found += 1
        else:
            print(f"  ✗ '{name}' NOT FOUND")

    print(f"\n  Found: {found}/{len(test_lookups)} ({found/len(test_lookups)*100:.0f}%)")

    # Path finding
    print(f"\n  Paths:")
    test_paths = [
        ("knowledge graph", "RAG"),
        ("Rust", "Python"),
        ("entity resolution", "knowledge graph"),
        ("VOPA", "FalkorDB"),
    ]
    for a, b in test_paths:
        result = kg.connect(a, b)
        print(f"    {a} → {b}: {result}")

    # Topics
    print(f"\n  Topics:")
    topics = json.loads(kg.topics())
    for type_name, entities in sorted(topics.items()):
        sample = ", ".join(entities[:5])
        more = f" (+{len(entities)-5})" if len(entities) > 5 else ""
        print(f"    {type_name}: {sample}{more}")

    print(f"\n  Saved to {KG_PATH}")


if __name__ == "__main__":
    main()
