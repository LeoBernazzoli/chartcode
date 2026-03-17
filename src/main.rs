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
                eprintln!("Usage: autoclaw explore <entity_name>");
                std::process::exit(1);
            }
            cmd_explore(&kg_path, &args[2]);
        }
        "connect" => {
            if args.len() < 4 {
                eprintln!("Usage: autoclaw connect <entity_a> <entity_b>");
                std::process::exit(1);
            }
            cmd_connect(&kg_path, &args[2], &args[3]);
        }
        "recent" => cmd_recent(&kg_path),
        "export" => cmd_export(&kg_path),
        "reindex" => {
            if args.len() < 3 {
                eprintln!("Usage: autoclaw reindex <file_path>");
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
                eprintln!("Usage: autoclaw tick <transcript_path> [--snapshot-every N] [--threshold N] [--window N]");
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
                eprintln!("Usage: autoclaw monitor <transcript_path> [--threshold N] [--window N]");
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
                eprintln!("Usage: autoclaw file-context <file_path> [--budget N]");
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
                eprintln!("Usage: autoclaw relevant <query> [--budget N]");
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
        r#"autoclaw - A persistent knowledge graph engine for AI agents

Usage: autoclaw <command> [args]

Commands:
  stats                  Show graph statistics
  topics                 Show main knowledge topics
  explore <name>         Explore an entity and its connections
  connect <a> <b>        Find path between two entities
  recent                 Show recently added entities
  export                 Export graph as JSON
  context [budget]       Generate context for re-injection (default budget: 2000 tokens)

Environment:
  AUTOCLAW_KG            Path to .kg file (default: ./knowledge.kg)

The primary interface is the Python SDK. The CLI is for inspection and debugging."#
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
