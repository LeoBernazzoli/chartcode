<p align="center">
  <h1 align="center">Graphcode</h1>
</p>

<p align="center">
  <strong>Your AI coding agent doesn't understand your codebase. This fixes that.</strong>
</p>

<p align="center">
  <a href="#the-problem">The Problem</a> &nbsp;&bull;&nbsp;
  <a href="#what-graphcode-does">What It Does</a> &nbsp;&bull;&nbsp;
  <a href="#how-it-works">How It Works</a> &nbsp;&bull;&nbsp;
  <a href="#accuracy">Accuracy</a> &nbsp;&bull;&nbsp;
  <a href="#get-started">Get Started</a>
</p>

<p align="center">
  <a href="https://github.com/LeoBernazzoli/graphcode/stargazers"><img src="https://img.shields.io/github/stars/LeoBernazzoli/graphcode?style=flat" alt="Stars"></a>
  <a href="https://github.com/LeoBernazzoli/graphcode/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue" alt="License"></a>
  <a href="https://github.com/LeoBernazzoli/graphcode"><img src="https://img.shields.io/badge/lang-Rust-orange" alt="Rust"></a>
</p>

---

## The Problem

AI coding agents break things. Not because they're bad at writing code, but because they're blind.

They don't know that the function they're renaming is called from 14 other files. They don't know that the field they're deleting is read by a component on the other side of the codebase. They don't know that the class they're modifying has 122 fields and is the most connected entity in the entire project.

They write code in the dark, and you pay the price.

## What Graphcode Does

Graphcode builds a **knowledge graph of your entire codebase** before your AI agent touches a single line.

Every function, class, field, import, type annotation, keyword argument, and re-export is mapped. Every cross-file dependency is tracked. When your agent is about to edit a file, Graphcode tells it exactly what will break.

No embeddings. No LLM calls. No cloud. Just deterministic code analysis powered by tree-sitter, running locally in milliseconds.

## How It Works

Graphcode runs as a **Claude Code plugin** (Codex support coming). Three things happen:

**1. Bootstrap** &mdash; On first run, Graphcode parses every source file with tree-sitter and builds the dependency graph. A 900-file TypeScript + Python project takes about 10 seconds.

**2. Path-specific rules** &mdash; For every file in your project, Graphcode generates a `.claude/rules/` file listing its entities, their reference counts, and which files depend on them. These rules load automatically when the AI opens that file.

**3. Live impact analysis** &mdash; Before every Edit or Write, a PreToolUse hook runs impact analysis in ~50ms. The AI sees which files will be affected before it makes the change.

The result: your AI agent knows, before writing a single character, that renaming `password_hash` will affect `auth/__init__.py`, `routes/auth.py`, and `tests/test_auth.py`.

## Accuracy

We tested against three open-source projects with manually verified ground truth:

| Project | Entity | Files Found | True Refs | Accuracy |
|---------|--------|------------|-----------|----------|
| **FastAPI** | `Depends` | 128+ | 128 | 100% |
| **FastAPI** | `FastAPI` | 435+ | 435 | 100% |
| **tRPC** | `TRPCError` | 56 | 56 | 100% |
| **tRPC** | `initTRPC` | 165 | 166 | 99% |
| **httpx** | `Request` | 33 | 33 | 100% |
| **httpx** | `TimeoutException` | 3 | 3 | 100% |

Zero false positives. No LLM involved. Pure static analysis.

Supported languages: **Python, TypeScript, JavaScript, Java, Go, Rust, C#**.

## What It Catches

Things your AI agent currently misses:

- Cross-file field access (`user.password_hash` in `routes/auth.py` references `User.password_hash` in `models.py`)
- Re-exports through `__init__.py` and barrel files (`from ._exceptions import *`)
- Monorepo package imports (`import { TRPCError } from '@trpc/server'`)
- Transitive dependencies (A imports B which re-exports from C)
- Keyword argument writes (`create_user(password_hash=value)`)
- Named import tracking (`from models import User, APIKey`)
- TypeScript export specifiers (`export { TRPCError }`)

## Get Started

**As a Claude Code plugin:**

```
/plugin marketplace add LeoBernazzoli/graphcode
/plugin install graphcode
/graphcode:start
```

**As a standalone CLI:**

```
cargo install --path .
cd your-project
autoclaw init
```

That's it. The knowledge graph is built, rules are generated, and your AI agent now understands your codebase.

## Architecture

Built in Rust. Single binary. No dependencies beyond tree-sitter.

- **Bootstrap**: tree-sitter AST parsing for 7 languages
- **Import resolution**: tiered system (same-file, import-scoped, transitive, global) inspired by GitNexus
- **Impact analysis**: ~50ms per query on 30K+ node graphs
- **Storage**: single `.kg` MessagePack file, no database
- **Plugin**: 4 hooks (SessionStart, PreToolUse, PostToolUse, Stop) + 4 skills

## License

Apache 2.0
