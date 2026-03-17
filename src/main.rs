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
