#!/usr/bin/env python3
"""
Multi-document stress test: Wikipedia + corporate docs + textbook + manual.
Tests cross-document entity resolution, ontology evolution, and graph quality.

Usage:
    source .venv/bin/activate
    python examples/test_multi_document.py
"""

import json
import os
import subprocess
import sys
import time
import urllib.request

# ── Helpers ──────────────────────────────────────────────────────

def fetch_wikipedia(title: str, max_chars: int = 12000) -> str:
    url = (
        f"https://en.wikipedia.org/w/api.php?"
        f"action=query&titles={title}&prop=extracts"
        f"&explaintext=true&format=json"
    )
    req = urllib.request.Request(url, headers={"User-Agent": "autoclaw-test/0.1"})
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read())
    for page in data["query"]["pages"].values():
        return page.get("extract", "")[:max_chars]
    return ""


def ask_claude(prompt: str) -> str:
    result = subprocess.run(
        ["claude", "-p", prompt, "--output-format", "json", "--model", "sonnet"],
        capture_output=True, text=True, timeout=300,
    )
    if result.returncode != 0:
        print(f"  Claude error: {result.stderr[:300]}", file=sys.stderr)
        return '{"entities":[],"relations":[]}'
    try:
        response = json.loads(result.stdout)
        return response.get("result", "")
    except json.JSONDecodeError:
        return result.stdout


def extract_json(text: str) -> dict:
    import re
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


# ── Test Documents ───────────────────────────────────────────────

CORPORATE_DOC = """
# ACME Corporation - Q3 2025 Strategic Review

## Executive Summary
ACME Corporation achieved revenue of $2.4 billion in Q3 2025, a 15% increase year-over-year.
CEO Maria Chen led the executive team through a major organizational restructuring.
CTO James Park oversaw the launch of Project Horizon, our AI-powered supply chain platform.

## Key Initiatives

### Project Horizon
Project Horizon is ACME's flagship AI initiative, launched in July 2025. The platform uses
machine learning and knowledge graphs to optimize supply chain operations. Led by VP of Engineering
Sarah Kim, the team of 45 engineers built the system using Python, Rust, and PostgreSQL.
Project Horizon reduced supply chain costs by 23% in pilot customers including Walmart and Target.

### Project Atlas
Project Atlas is our cloud migration initiative, managed by Director of Infrastructure Tom Wilson.
The project migrates ACME's legacy systems from on-premise data centers to AWS. As of Q3,
67% of workloads have been migrated. The remaining migration is expected to complete by Q1 2026.
Atlas uses Kubernetes, Terraform, and a custom deployment pipeline called "Launchpad."

## Financial Highlights
- Revenue: $2.4B (+15% YoY)
- Operating margin: 18.3% (up from 16.1%)
- R&D spending: $380M (15.8% of revenue)
- Headcount: 12,400 employees across 23 countries

## Leadership Team
- Maria Chen, CEO - 8 years at ACME, former McKinsey partner
- James Park, CTO - joined 2023 from Google Cloud
- Sarah Kim, VP Engineering - leads Project Horizon
- Tom Wilson, Director of Infrastructure - leads Project Atlas
- David Lee, CFO - oversees $2.4B revenue operations
"""

TEXTBOOK_BIOLOGY = """
# Chapter 5: Cell Biology - The Building Blocks of Life

## 5.1 Cell Theory
All living organisms are composed of one or more cells. The cell is the basic unit of life.
All cells arise from pre-existing cells through cell division. This fundamental principle,
known as cell theory, was established by Matthias Schleiden and Theodor Schwann in 1839,
and later refined by Rudolf Virchow who added that all cells come from cells (omnis cellula e cellula).

## 5.2 Cell Structure

### The Plasma Membrane
The plasma membrane is a phospholipid bilayer that surrounds all cells. It is selectively
permeable, controlling what enters and exits the cell. The fluid mosaic model, proposed by
Singer and Nicolson in 1972, describes the membrane as a dynamic structure with proteins
floating in a sea of lipids. Integral proteins span the entire membrane, while peripheral
proteins are attached to the surface.

### The Nucleus
The nucleus is the control center of eukaryotic cells, containing the cell's DNA organized
into chromosomes. It is surrounded by a double membrane called the nuclear envelope, which
contains nuclear pores that regulate transport between the nucleus and cytoplasm.
The nucleolus, located inside the nucleus, is responsible for ribosome production.

### Mitochondria
Mitochondria are the powerhouses of the cell, responsible for cellular respiration and ATP
production. They have their own DNA (mitochondrial DNA or mtDNA), supporting the endosymbiotic
theory proposed by Lynn Margulis in 1967. This theory suggests that mitochondria were once
free-living bacteria that were engulfed by ancestral eukaryotic cells.

### Endoplasmic Reticulum
The endoplasmic reticulum (ER) is a network of membranes throughout the cytoplasm. Rough ER
is studded with ribosomes and synthesizes proteins, while smooth ER synthesizes lipids and
detoxifies harmful substances. The ER works closely with the Golgi apparatus for protein
processing and transport.

## 5.3 Cell Division

### Mitosis
Mitosis is the process of cell division that produces two genetically identical daughter cells.
It consists of four phases: prophase, metaphase, anaphase, and telophase. During prophase,
chromosomes condense and become visible. In metaphase, chromosomes align at the cell's equator.
Anaphase involves the separation of sister chromatids. Telophase completes the division.

### Meiosis
Meiosis is a specialized form of cell division that produces four genetically unique haploid
cells (gametes). Unlike mitosis, meiosis involves two rounds of division (meiosis I and II)
and includes crossing over during prophase I, which increases genetic diversity.
"""

SOFTWARE_MANUAL = """
# DataFlow Platform - Administrator Guide v3.2

## 1. System Architecture

DataFlow is a distributed data processing platform built on Apache Kafka and Apache Spark.
The system consists of three main components: the Ingestion Layer, the Processing Engine,
and the Storage Backend.

### 1.1 Ingestion Layer
The Ingestion Layer accepts data from multiple sources including REST APIs, message queues,
and file uploads. It uses Apache Kafka as the message broker, with Kafka Connect for
source connectors. The layer supports JSON, Avro, and Parquet formats.
Rate limiting is configured at 10,000 events per second per tenant.

### 1.2 Processing Engine
The Processing Engine is built on Apache Spark Structured Streaming. It processes data
in micro-batches with a default interval of 2 seconds. The engine supports SQL-based
transformations, custom UDFs (User Defined Functions), and machine learning pipelines
via MLlib. Processing jobs are managed by Apache YARN.

### 1.3 Storage Backend
Processed data is stored in a tiered storage system:
- Hot tier: Apache Cassandra for real-time queries (retention: 7 days)
- Warm tier: Apache Parquet files on HDFS (retention: 90 days)
- Cold tier: AWS S3 with Glacier transition (retention: 7 years)

## 2. Configuration

### 2.1 Kafka Configuration
- bootstrap.servers: kafka-1:9092,kafka-2:9092,kafka-3:9092
- replication.factor: 3
- min.insync.replicas: 2
- retention.ms: 604800000 (7 days)

### 2.2 Spark Configuration
- spark.executor.memory: 8g
- spark.executor.cores: 4
- spark.sql.shuffle.partitions: 200

## 3. Monitoring

DataFlow integrates with Prometheus for metrics collection and Grafana for visualization.
Key metrics include:
- Ingestion rate (events/sec)
- Processing latency (p50, p95, p99)
- Storage utilization per tier
- Error rate by pipeline

Alerting is configured through AlertManager with PagerDuty integration for critical alerts.
The SLA target is 99.9% uptime with processing latency under 5 seconds at p99.
"""

# ── Main Test ────────────────────────────────────────────────────

def process_document(kg, text: str, doc_name: str, doc_type: str):
    """Process a single document through the full pipeline."""
    start = time.time()

    # Chunk the text
    chunks = kg.chunk_text(text, 4000, 500)
    if not chunks:
        chunks = [text]
    print(f"  Chunks: {len(chunks)}")

    # Analyze content for ontology (use first chunk)
    sample = chunks[0] if chunks else text[:4000]
    ontology_prompt = kg.analyze_content(sample)
    ontology_response = ask_claude(ontology_prompt)
    ontology_json = extract_json(ontology_response)
    kg.update_ontology(json.dumps(ontology_json))

    n_entity_types = len(ontology_json.get("suggested_entity_types", []))
    n_rel_types = len(ontology_json.get("suggested_relation_types", []))
    print(f"  Ontology: +{n_entity_types} entity types, +{n_rel_types} relation types")

    # Extract from each chunk
    total_added = 0
    total_merged = 0
    total_edges = 0
    total_deduped = 0

    for i, chunk in enumerate(chunks):
        extraction_prompt = kg.prepare_extraction(chunk)
        extraction_response = ask_claude(extraction_prompt)
        extraction_json = extract_json(extraction_response)
        report = kg.ingest_document(json.dumps(extraction_json), doc_name, page=i+1)
        total_added += report["added"]
        total_merged += report["merged"]
        total_edges += report["edges_added"]
        total_deduped += report["edges_deduped"]

    elapsed = time.time() - start
    print(f"  Result: +{total_added} entities, +{total_edges} edges, "
          f"{total_merged} merged, {total_deduped} deduped ({elapsed:.1f}s)")


def main():
    from autoclaw import PyKnowledgeGraph as KnowledgeGraph

    print("=" * 70)
    print("  Multi-Document Stress Test")
    print("=" * 70)

    kg = KnowledgeGraph("/tmp/test_multi.kg")

    # ── Document 1: Wikipedia - Machine Learning ─────────────────
    print("\n[1/5] Wikipedia: Machine Learning")
    text = fetch_wikipedia("Machine_learning")
    if text:
        process_document(kg, text, "wikipedia_ml", "encyclopedia")
    else:
        print("  SKIP: failed to fetch")

    # ── Document 2: Wikipedia - Knowledge Graph ──────────────────
    print("\n[2/5] Wikipedia: Knowledge Graph")
    text = fetch_wikipedia("Knowledge_graph")
    if text:
        process_document(kg, text, "wikipedia_kg", "encyclopedia")
    else:
        print("  SKIP: failed to fetch")

    # ── Document 3: Corporate Strategic Review ───────────────────
    print("\n[3/5] Corporate: ACME Q3 Strategic Review")
    process_document(kg, CORPORATE_DOC, "acme_q3_review.pdf", "corporate")

    # ── Document 4: Biology Textbook ─────────────────────────────
    print("\n[4/5] Textbook: Cell Biology Chapter")
    process_document(kg, TEXTBOOK_BIOLOGY, "biology_ch5.pdf", "textbook")

    # ── Document 5: Software Manual ──────────────────────────────
    print("\n[5/5] Manual: DataFlow Administrator Guide")
    process_document(kg, SOFTWARE_MANUAL, "dataflow_admin_guide.pdf", "manual")

    # ── Post-processing ────────────────────────────────────────
    print("\n[6/6] Post-processing: connecting orphans and discovering cross-doc links...")
    orphans_connected = kg.connect_orphans()
    cross_doc = kg.discover_connections()
    print(f"  Orphan connections: {orphans_connected}")
    print(f"  Cross-doc discoveries: {cross_doc}")

    kg.save()

    # ── Quality Analysis ─────────────────────────────────────────
    print("\n" + "=" * 70)
    print("  Quality Analysis")
    print("=" * 70)

    q = json.loads(kg.quality_metrics())
    print(f"\nQuality Metrics:")
    print(f"  Orphan ratio: {q['orphan_ratio']:.1%}")
    print(f"  related_to ratio: {q['related_to_ratio']:.1%}")
    print(f"  Avg degree: {q['avg_degree']:.1f}")

    stats = json.loads(kg.stats())
    print(f"\nTotal: {stats['node_count']} nodes, {stats['edge_count']} edges, "
          f"{stats['document_count']} documents")

    print(f"\nNode types ({len(stats['node_types'])}):")
    for t, count in sorted(stats["node_types"].items(), key=lambda x: -x[1]):
        print(f"  {t}: {count}")

    print(f"\nEdge types ({len(stats['edge_types'])}):")
    for t, count in sorted(stats["edge_types"].items(), key=lambda x: -x[1]):
        print(f"  {t}: {count}")

    # ── Cross-Document Tests ─────────────────────────────────────
    print("\n" + "=" * 70)
    print("  Cross-Document Entity Resolution")
    print("=" * 70)

    # Test: "machine learning" should appear in both Wikipedia articles AND corporate doc
    ml = kg.explore("machine learning")
    if ml:
        data = json.loads(ml)
        print(f"\n  'machine learning' - {len(data['relations'])} connections")
        for r in data["relations"][:5]:
            dir_sym = "->" if r["direction"] == "Outgoing" else "<-"
            print(f"    {dir_sym} [{r['relation_type']}] {r['node']['name']} ({r['node']['node_type']})")
        if len(data["relations"]) > 5:
            print(f"    ... +{len(data['relations'])-5} more")
    else:
        print("\n  'machine learning' NOT FOUND")

    # Test: "knowledge graph" should connect Wikipedia + corporate doc
    kgraph = kg.explore("knowledge graph")
    if kgraph:
        data = json.loads(kgraph)
        print(f"\n  'knowledge graph' - {len(data['relations'])} connections")
        for r in data["relations"][:5]:
            dir_sym = "->" if r["direction"] == "Outgoing" else "<-"
            print(f"    {dir_sym} [{r['relation_type']}] {r['node']['name']} ({r['node']['node_type']})")
    else:
        print("\n  'knowledge graph' NOT FOUND")

    # ── Path Finding Across Documents ────────────────────────────
    print("\n" + "=" * 70)
    print("  Cross-Document Path Finding")
    print("=" * 70)

    test_paths = [
        ("Maria Chen", "machine learning"),    # corporate → wikipedia
        ("mitochondria", "Apache Kafka"),       # textbook → manual (should fail - unrelated)
        ("cell theory", "meiosis"),             # within textbook
        ("Project Horizon", "Apache Spark"),    # corporate → manual (maybe via tech)
        ("Sarah Kim", "Project Horizon"),       # within corporate
    ]
    for a, b in test_paths:
        result = kg.connect(a, b)
        print(f"\n  '{a}' → '{b}':")
        print(f"    {result}")

    # ── Navigation Demo ──────────────────────────────────────────
    print("\n" + "=" * 70)
    print("  Entity Navigation Samples")
    print("=" * 70)

    for name in ["Maria Chen", "mitochondria", "Apache Kafka", "neural network",
                  "DNA", "Prometheus", "cell division"]:
        result = kg.explore(name)
        if result:
            data = json.loads(result)
            e = data["entity"]
            n_rels = len(data["relations"])
            print(f"\n  {e['name']} ({e['node_type']}) - {n_rels} connections")
            print(f"    {e['definition'][:100]}")
        else:
            print(f"\n  '{name}' - NOT FOUND")

    # ── Topics Overview ──────────────────────────────────────────
    print("\n" + "=" * 70)
    print("  Topics Overview")
    print("=" * 70)

    topics = json.loads(kg.topics())
    for type_name, entities in sorted(topics.items()):
        sample = ", ".join(entities[:5])
        more = f" (+{len(entities)-5})" if len(entities) > 5 else ""
        print(f"  {type_name}: {sample}{more}")

    print(f"\nSaved to /tmp/test_multi.kg")
    print("Done!")


if __name__ == "__main__":
    main()
