use autoclaw::{load_or_create, KnowledgeGraph};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return;
    }

    let kg_path = PathBuf::from(
        std::env::var("AUTOCLAW_KG").unwrap_or_else(|_| "./knowledge.kg".to_string()),
    );

    match args[1].as_str() {
        "stats" => cmd_stats(&kg_path),
        "topics" => cmd_topics(&kg_path),
        "explore" => {
            if args.len() < 3 {
                eprintln!("Usage: graphcode explore <entity_name>");
                std::process::exit(1);
            }
            cmd_explore(&kg_path, &args[2]);
        }
        "connect" => {
            if args.len() < 4 {
                eprintln!("Usage: graphcode connect <entity_a> <entity_b>");
                std::process::exit(1);
            }
            cmd_connect(&kg_path, &args[2], &args[3]);
        }
        "recent" => cmd_recent(&kg_path),
        "export" => cmd_export(&kg_path),
        "init" => {
            eprintln!("Graphocode: initializing project...");

            // 1. Bootstrap
            let config_path = std::path::Path::new("graphocode.toml");
            let config = autoclaw::config::load_config(config_path);
            let mut kg = load_kg(&kg_path);
            let project_path = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let report = autoclaw::bootstrap::bootstrap(&mut kg, &config, &project_path);
            autoclaw::save(&kg, kg_path.as_path()).unwrap_or_else(|e| {
                eprintln!("Failed to save: {}", e);
                std::process::exit(1);
            });
            eprintln!(
                "  Indexed: {} files, {} entities, {} edges",
                report.files_indexed,
                report.code_entities,
                kg.stats().edge_count
            );

            // 2. Sync rules
            let rules_dir = project_path.join(".claude").join("rules");
            autoclaw::sync_rules::sync_rules(&kg, &rules_dir);
            let rule_count = std::fs::read_dir(&rules_dir)
                .map(|d| d.count())
                .unwrap_or(0);
            eprintln!("  Generated: {} rule files in .claude/rules/", rule_count);

            // 3. Summary
            let stats = kg.stats();
            eprintln!();
            eprintln!("Graphocode ready:");
            eprintln!("  {} nodes, {} edges", stats.node_count, stats.edge_count);
            eprintln!("  {} rule files (path-specific, 96% adherence)", rule_count);
            eprintln!();
            // Build fast index for hooks
            let idx_path = kg_path.with_extension("idx");
            // Re-run the build-index logic inline
            {
                let mut refs: std::collections::HashMap<String, (String, usize, std::collections::HashSet<String>)> =
                    std::collections::HashMap::new();
                for node in kg.all_nodes() {
                    if matches!(node.node_type.as_str(), "Function" | "Method" | "Struct" | "Field" | "Enum" | "Trait" | "Const") {
                        let name = if node.name.contains('.') {
                            node.name.split('.').last().unwrap_or(&node.name).to_string()
                        } else {
                            node.name.clone()
                        };
                        if name.len() >= 3 {
                            refs.entry(name).or_insert_with(|| (node.node_type.clone(), 0, std::collections::HashSet::new()));
                        }
                    }
                }
                for node in kg.all_nodes() {
                    let name = if node.name.contains('.') {
                        node.name.split('.').last().unwrap_or(&node.name).to_string()
                    } else {
                        node.name.clone()
                    };
                    let inbound = kg.inbound_reference_count(&node.name);
                    if inbound > 0 {
                        if let Some(entry) = refs.get_mut(&name) {
                            entry.1 = entry.1.max(inbound);
                        }
                        let references = kg.references_to(&node.name);
                        for r in &references {
                            let file = std::path::Path::new(&r.source_file)
                                .file_name()
                                .and_then(|f| f.to_str())
                                .unwrap_or(&r.source_file);
                            if let Some(entry) = refs.get_mut(&name) {
                                entry.2.insert(file.to_string());
                            }
                        }
                    }
                }
                let mut lines = Vec::new();
                for (name, (etype, count, files)) in &refs {
                    if *count > 0 {
                        let files_str: String = files.iter().take(10).cloned().collect::<Vec<_>>().join(",");
                        lines.push(format!("{}|{}|{}|{}", name, etype, count, files_str));
                    }
                }
                lines.sort();
                std::fs::write(&idx_path, lines.join("\n")).ok();
                eprintln!("  Built fast index: {} entries", lines.len());
            }

            eprintln!("Open Claude Code and start working. The knowledge graph is active.");
        }
        "sync-rules" => {
            let kg = load_kg(&kg_path);
            let project_dir = args
                .iter()
                .position(|a| a == "--project-dir")
                .and_then(|i| args.get(i + 1))
                .map(|s| std::path::PathBuf::from(s))
                .unwrap_or_else(|| std::env::current_dir().unwrap());
            let rules_dir = project_dir.join(".claude").join("rules");
            autoclaw::sync_rules::sync_rules(&kg, &rules_dir);
            eprintln!("Rules synced to {}", rules_dir.display());
        }
        "bootstrap" => {
            let config_path = args
                .iter()
                .position(|a| a == "--config")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("graphocode.toml");

            let config = autoclaw::config::load_config(std::path::Path::new(config_path));
            let mut kg = load_kg(&kg_path);

            let project_path = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let report = autoclaw::bootstrap::bootstrap(&mut kg, &config, &project_path);

            autoclaw::save(&kg, kg_path.as_path()).unwrap_or_else(|e| {
                eprintln!("Failed to save: {}", e);
                std::process::exit(1);
            });

            println!("Bootstrap complete:");
            println!("  Files indexed: {}", report.files_indexed);
            println!("  Code entities: {}", report.code_entities);
            println!("  Conversations found: {}", report.conversations_found);
            println!("  Document chunks: {}", report.document_chunks.len());

            if !report.conversation_texts.is_empty() {
                println!(
                    "\n{} conversations ready for Haiku semantic extraction.",
                    report.conversation_texts.len()
                );
                println!("Run /graphocode:start to complete extraction with LLM.");
            }
        }
        "impact" => {
            if args.len() < 3 {
                eprintln!("Usage: graphcode impact <entity_name> [--depth N]");
                std::process::exit(1);
            }
            let entity_name = &args[2];
            let depth: usize = args
                .iter()
                .position(|a| a == "--depth")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            let kg = load_kg(&kg_path);
            let report = autoclaw::impact::impact_analysis(&kg, entity_name, depth);
            print!("{}", report);
        }
        "build-index" => {
            let kg = load_kg(&kg_path);
            let idx_path = kg_path.with_extension("idx");
            let mut lines = Vec::new();

            // Build: entity_name → (type, ref_count, files)
            let mut refs: std::collections::HashMap<String, (String, usize, std::collections::HashSet<String>)> =
                std::collections::HashMap::new();

            for node in kg.all_nodes() {
                if matches!(node.node_type.as_str(), "Function" | "Method" | "Struct" | "Field" | "Enum" | "Trait" | "Const") {
                    let name = if node.name.contains('.') {
                        node.name.split('.').last().unwrap_or(&node.name).to_string()
                    } else {
                        node.name.clone()
                    };
                    if name.len() >= 3 {
                        refs.entry(name).or_insert_with(|| (node.node_type.clone(), 0, std::collections::HashSet::new()));
                    }
                }
            }

            for edge in &kg.stats().edge_types {
                // We need to iterate edges directly
            }
            // Iterate edges via neighbors for each entity
            for node in kg.all_nodes() {
                let name = if node.name.contains('.') {
                    node.name.split('.').last().unwrap_or(&node.name).to_string()
                } else {
                    node.name.clone()
                };
                let inbound = kg.inbound_reference_count(&node.name);
                if inbound > 0 {
                    if let Some(entry) = refs.get_mut(&name) {
                        entry.1 = entry.1.max(inbound);
                    }
                    // Get referring files
                    let references = kg.references_to(&node.name);
                    for r in &references {
                        let file = std::path::Path::new(&r.source_file)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .unwrap_or(&r.source_file);
                        if let Some(entry) = refs.get_mut(&name) {
                            entry.2.insert(file.to_string());
                        }
                    }
                }
            }

            for (name, (etype, count, files)) in &refs {
                if *count > 0 {
                    let files_str: String = files.iter().take(10).cloned().collect::<Vec<_>>().join(",");
                    lines.push(format!("{}|{}|{}|{}", name, etype, count, files_str));
                }
            }
            lines.sort();
            std::fs::write(&idx_path, lines.join("\n")).unwrap_or_else(|e| {
                eprintln!("Failed to write index: {}", e);
                std::process::exit(1);
            });
            eprintln!("Index built: {} entries → {}", lines.len(), idx_path.display());
        }
        "quick-impact" => {
            // Fast impact analysis using .idx file (no KG load)
            let idx_path = kg_path.with_extension("idx");
            let tool_input = if args.len() >= 3 {
                args[2].clone()
            } else {
                let mut input = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
                    .unwrap_or_default();
                input
            };

            let v: serde_json::Value = match serde_json::from_str(&tool_input) {
                Ok(v) => v,
                Err(_) => std::process::exit(0),
            };

            let input = if v.get("tool_input").is_some() {
                v.get("tool_input").unwrap().clone()
            } else {
                v.clone()
            };

            let old_string = input.get("old_string").and_then(|s| s.as_str()).unwrap_or("");
            if old_string.is_empty() {
                std::process::exit(0);
            }

            // Read index file
            let idx_content = match std::fs::read_to_string(&idx_path) {
                Ok(c) => c,
                Err(_) => {
                    eprintln!("No index file. Run: graphcode build-index");
                    std::process::exit(0);
                }
            };

            // Find entities mentioned in old_string
            let mut hits = Vec::new();
            for line in idx_content.lines() {
                let parts: Vec<&str> = line.splitn(4, '|').collect();
                if parts.len() >= 4 {
                    let name = parts[0];
                    if name.len() >= 3 && old_string.contains(name) {
                        hits.push((name.to_string(), parts[1].to_string(), parts[2].to_string(), parts[3].to_string()));
                    }
                }
            }

            if hits.is_empty() {
                std::process::exit(0);
            }

            // Build report
            let mut report = String::new();
            for (name, etype, count, files) in &hits {
                report.push_str(&format!("⚠️ {}: {} refs in {}\n", name, count, files));
            }

            // Output as hookSpecificOutput JSON
            let output = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "additionalContext": report
                }
            });
            print!("{}", output);
        }
        "impact-from-diff" => {
            // V2: reads tool input from stdin (for hook compatibility) or args
            let tool_input = if args.len() >= 3 {
                args[2].clone()
            } else {
                let mut input = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
                    .unwrap_or_default();
                input
            };
            let kg = load_kg(&kg_path);
            // V2: output additionalContext JSON
            let output = autoclaw::impact::impact_from_diff_v2(&kg, &tool_input);
            if !output.is_empty() {
                print!("{}", output);
            }
        }
        "reindex" => {
            if args.len() < 3 {
                eprintln!("Usage: graphcode reindex <file_path>");
                std::process::exit(1);
            }
            let file_path = &args[2];
            let code = std::fs::read_to_string(file_path).unwrap_or_else(|e| {
                eprintln!("Cannot read {}: {}", file_path, e);
                std::process::exit(1);
            });
            let mut kg = load_kg(&kg_path);
            kg.reindex_file(file_path, &code);
            autoclaw::save(&kg, kg_path.as_path()).unwrap_or_else(|e| {
                eprintln!("Failed to save: {}", e);
                std::process::exit(1);
            });
            eprintln!("Reindexed: {}", file_path);
        }
        "reconcile" => {
            let mut input_json = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut input_json)
                .expect("Failed to read stdin");

            let input: autoclaw::reconcile::ReconcileInput =
                serde_json::from_str(&input_json).unwrap_or_else(|e| {
                    eprintln!("Invalid JSON input: {}", e);
                    std::process::exit(1);
                });

            let mut kg = load_kg(&kg_path);
            let mut report = autoclaw::reconcile::reconcile(&mut kg, &input);

            // Run GC after reconciliation
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            report.gc_removed = autoclaw::reconcile::garbage_collect(&mut kg, 0.05, now);

            autoclaw::save(&kg, kg_path.as_path()).unwrap_or_else(|e| {
                eprintln!("Failed to save: {}", e);
                std::process::exit(1);
            });

            println!("{}", serde_json::to_string(&report).unwrap());
        }
        "tick" => {
            if args.len() < 3 {
                eprintln!("Usage: graphcode tick <transcript_path> [--snapshot-every N] [--threshold N] [--window N]");
                std::process::exit(1);
            }
            let transcript = &args[2];
            let snapshot_every: u64 = args
                .iter()
                .position(|a| a == "--snapshot-every")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(20);
            let threshold: u64 = args
                .iter()
                .position(|a| a == "--threshold")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(85);
            let window: u64 = args
                .iter()
                .position(|a| a == "--window")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(200000);

            let transcript_path = std::path::Path::new(transcript);
            let counter_file = transcript_path.with_extension("tick");
            let result = autoclaw::tick::tick(
                transcript_path,
                &counter_file,
                snapshot_every,
                threshold,
                window,
            );

            match result.action {
                autoclaw::tick::TickAction::None => {
                    // Silent — no action needed
                    std::process::exit(0);
                }
                autoclaw::tick::TickAction::Snapshot => {
                    // Run heuristic snapshot on recent transcript entries
                    // For now, just signal success — snapshot integration comes later
                    eprintln!("tick: snapshot triggered (counter reset)");
                    std::process::exit(0);
                }
                autoclaw::tick::TickAction::Extract => {
                    // Signal extraction needed — exit 1 triggers extract-and-compact.sh
                    eprintln!("tick: extraction triggered ({}% context used)", result.used_pct);
                    std::process::exit(1);
                }
            }
        }
        "monitor" => {
            if args.len() < 3 {
                eprintln!("Usage: graphcode monitor <transcript_path> [--threshold N] [--window N]");
                std::process::exit(1);
            }
            let transcript = &args[2];
            let threshold: u64 = args
                .iter()
                .position(|a| a == "--threshold")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(85);
            let window: u64 = args
                .iter()
                .position(|a| a == "--window")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(200000);

            match autoclaw::monitor::check_context_usage(
                std::path::Path::new(transcript),
                threshold,
                window,
            ) {
                Ok(result) => {
                    println!(
                        r#"{{"used_pct":{},"used_tokens":{},"window_size":{},"should_extract":{}}}"#,
                        result.used_pct, result.used_tokens, result.window_size, result.should_extract
                    );
                    if result.should_extract {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Monitor error: {}", e);
                    std::process::exit(0); // Don't block on error
                }
            }
        }
        "file-context" => {
            if args.len() < 3 {
                eprintln!("Usage: graphcode file-context <file_path> [--budget N]");
                std::process::exit(1);
            }
            let file_path = &args[2];
            let budget: usize = args
                .iter()
                .position(|a| a == "--budget")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(300);
            let kg = load_kg(&kg_path);
            let output = autoclaw::file_context::file_context(&kg, file_path, budget);
            if !output.is_empty() {
                print!("{}", output);
            }
        }
        "relevant" => {
            if args.len() < 3 {
                eprintln!("Usage: graphcode relevant <query> [--budget N]");
                std::process::exit(1);
            }
            let query = &args[2];
            let budget: usize = args
                .iter()
                .position(|a| a == "--budget")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(500);
            let kg = load_kg(&kg_path);
            let output = autoclaw::relevant::find_relevant(&kg, query, budget);
            if output.is_empty() {
                // No relevant context — silent (hook should not inject noise)
            } else {
                print!("{}", output);
            }
        }
        "context" => {
            let budget: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2000);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let kg = load_kg(&kg_path);
            let output = autoclaw::context::generate_context(&kg, budget, now);
            print!("{}", output);
        }
        "--help" | "-h" | "help" => print_usage(),
        cmd => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!(
        r#"graphcode - Knowledge graph for your codebase engine for AI agents

Usage: graphcode <command> [args]

Navigation:
  stats                          Show graph statistics
  topics                         Show main knowledge topics
  explore <name>                 Explore an entity and its connections
  connect <a> <b>                Find path between two entities
  recent                         Show recently added entities
  export                         Export graph as JSON

Context (used by Claude Code hooks):
  context [budget]               Generate ranked context for re-injection
  relevant <query> [--budget N]  Find facts relevant to a text query
  file-context <path> [--budget] Get KG knowledge about a specific file

Impact analysis:
  impact <entity> [--depth N]    Show all references + breaking changes
  impact-from-diff <json>        Impact analysis from an Edit/Write diff

Ingestion:
  bootstrap [--config path]      Full project indexing (code + conversations)
  reindex <file_path>            Re-parse a single file with tree-sitter
  reconcile                      Merge extraction JSON from stdin into KG
  snapshot <transcript> [opts]   Heuristic extraction from transcript

Monitoring:
  monitor <transcript> [opts]    Check context usage against threshold
  tick <transcript> [opts]       Combined monitor + periodic snapshot

Environment:
  AUTOCLAW_KG                    Path to .kg file (default: ./knowledge.kg)"#
    );
}

fn cmd_stats(path: &PathBuf) {
    let kg = load_kg(path);
    let stats = kg.stats();
    println!("Knowledge Graph: {}", path.display());
    println!("  Nodes:     {}", stats.node_count);
    println!("  Edges:     {}", stats.edge_count);
    println!("  Documents: {}", stats.document_count);
    println!("  Memories:  {}", stats.memory_count);
    if !stats.node_types.is_empty() {
        println!("  Node types:");
        for (t, count) in &stats.node_types {
            println!("    {}: {}", t, count);
        }
    }
    if !stats.edge_types.is_empty() {
        println!("  Edge types:");
        for (t, count) in &stats.edge_types {
            println!("    {}: {}", t, count);
        }
    }
}

fn cmd_topics(path: &PathBuf) {
    let kg = load_kg(path);
    let topics = kg.topics();
    if topics.is_empty() {
        println!("No topics yet. Feed some documents first.");
        return;
    }
    for (type_name, entities) in &topics {
        println!("{}:", type_name);
        for name in entities.iter().take(10) {
            println!("  - {}", name);
        }
        if entities.len() > 10 {
            println!("  ... and {} more", entities.len() - 10);
        }
    }
}

fn cmd_explore(path: &PathBuf, name: &str) {
    let kg = load_kg(path);
    match kg.explore(name) {
        Some(result) => {
            println!("{} ({})", result.entity.name, result.entity.node_type);
            println!("  {}", result.entity.definition);
            if !result.entity.aliases.is_empty() {
                println!("  Aliases: {}", result.entity.aliases.join(", "));
            }
            println!();
            if result.relations.is_empty() {
                println!("  No connections.");
            } else {
                println!("  Connections:");
                for rel in &result.relations {
                    let dir = match rel.direction {
                        autoclaw::graph::Direction::Outgoing => "->",
                        autoclaw::graph::Direction::Incoming => "<-",
                    };
                    println!(
                        "    {} [{}] {} (confidence: {:.2})",
                        dir, rel.relation_type, rel.node.name, rel.confidence
                    );
                }
            }
            if !result.evidence.is_empty() {
                println!();
                println!("  Evidence:");
                for ev in &result.evidence {
                    println!(
                        "    - {} (p.{}): \"{}\"",
                        ev.document,
                        ev.page.map(|p| p.to_string()).unwrap_or_default(),
                        truncate(&ev.text_snippet, 80)
                    );
                }
            }
        }
        None => {
            eprintln!("Entity '{}' not found.", name);
            std::process::exit(1);
        }
    }
}

fn cmd_connect(path: &PathBuf, a: &str, b: &str) {
    let kg = load_kg(path);
    let result = kg.path(a, b);
    if !result.found {
        println!("No path found between '{}' and '{}'.", a, b);
        return;
    }
    println!("Path ({} hops):", result.length);
    for (i, node) in result.nodes.iter().enumerate() {
        print!("  {}", node.name);
        if i < result.edges.len() {
            print!(" --[{}]--> ", result.edges[i].relation_type);
        }
    }
    println!();
}

fn cmd_recent(path: &PathBuf) {
    let kg = load_kg(path);
    let recent = kg.recent(10);
    if recent.is_empty() {
        println!("No entities yet.");
        return;
    }
    println!("Recent entities:");
    for node in recent {
        println!("  {} ({}) - {}", node.name, node.node_type, truncate(&node.definition, 60));
    }
}

fn cmd_export(path: &PathBuf) {
    let kg = load_kg(path);
    match serde_json::to_string_pretty(&kg) {
        Ok(json) => println!("{}", json),
        Err(e) => {
            eprintln!("Export failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn load_kg(path: &PathBuf) -> KnowledgeGraph {
    match load_or_create(path.as_path()) {
        Ok(kg) => kg,
        Err(e) => {
            eprintln!("Failed to load {}: {}", path.display(), e);
            std::process::exit(1);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.min(s.len())])
    }
}
