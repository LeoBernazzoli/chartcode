use std::collections::{HashMap, HashSet};

/// Simple PageRank on a directed graph.
/// Returns a map of node name → rank score.
pub fn pagerank(
    edges: &[(String, String)],
    iterations: usize,
    damping: f64,
) -> HashMap<String, f64> {
    // Collect all unique nodes
    let mut nodes: HashSet<&str> = HashSet::new();
    for (from, to) in edges {
        nodes.insert(from);
        nodes.insert(to);
    }

    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }

    let n_f64 = n as f64;
    let initial = 1.0 / n_f64;

    // Initialize ranks
    let mut rank: HashMap<&str, f64> = nodes.iter().map(|&node| (node, initial)).collect();

    // Build outgoing edge map
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in edges {
        outgoing.entry(from.as_str()).or_default().push(to.as_str());
    }

    // Iterate
    for _ in 0..iterations {
        let mut new_rank: HashMap<&str, f64> = HashMap::new();
        let base = (1.0 - damping) / n_f64;

        for &node in &nodes {
            let mut incoming_sum = 0.0;
            // Find all nodes that link TO this node
            for (from, to) in edges {
                if to.as_str() == node {
                    let out_count = outgoing
                        .get(from.as_str())
                        .map(|v| v.len())
                        .unwrap_or(1) as f64;
                    incoming_sum += rank.get(from.as_str()).unwrap_or(&initial) / out_count;
                }
            }
            new_rank.insert(node, base + damping * incoming_sum);
        }

        rank = new_rank;
    }

    // Convert to owned strings
    rank.into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagerank_ranks_highly_referenced() {
        let mut edges: Vec<(String, String)> = Vec::new();
        for i in 0..10 {
            edges.push((format!("caller_{}", i), "popular_func".into()));
        }
        edges.push(("single_caller".into(), "unpopular_func".into()));

        let ranks = pagerank(&edges, 20, 0.85);
        assert!(
            ranks["popular_func"] > ranks["unpopular_func"],
            "popular={}, unpopular={}",
            ranks["popular_func"],
            ranks["unpopular_func"]
        );
    }

    #[test]
    fn test_pagerank_empty_graph() {
        let edges: Vec<(String, String)> = Vec::new();
        let ranks = pagerank(&edges, 20, 0.85);
        assert!(ranks.is_empty());
    }

    #[test]
    fn test_pagerank_single_edge() {
        let edges = vec![("a".into(), "b".into())];
        let ranks = pagerank(&edges, 20, 0.85);
        assert!(ranks["b"] > ranks["a"]);
    }

    #[test]
    fn test_pagerank_chain() {
        // a → b → c : c should rank highest (end of chain)
        let edges = vec![
            ("a".into(), "b".into()),
            ("b".into(), "c".into()),
        ];
        let ranks = pagerank(&edges, 20, 0.85);
        assert!(ranks["c"] > ranks["a"]);
    }

    #[test]
    fn test_pagerank_hub() {
        // hub → a, hub → b, hub → c : hub has high outgoing, targets get rank
        let edges = vec![
            ("hub".into(), "a".into()),
            ("hub".into(), "b".into()),
            ("hub".into(), "c".into()),
        ];
        let ranks = pagerank(&edges, 20, 0.85);
        // a, b, c should all have similar rank
        let diff = (ranks["a"] - ranks["b"]).abs();
        assert!(diff < 0.01, "a and b should have similar rank");
    }
}
