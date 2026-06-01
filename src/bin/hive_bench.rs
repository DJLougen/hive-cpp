//! `hive-bench` — Step 2 end-to-end benchmark for the Hive native port.
//!
//! Mirrors the Python `scripts/hive_benchmark.py` in shape. The CLI
//! surface is intentionally a strict subset so the two can be diffed
//! during the Step 2 to Step 3 transition.
//!
//! Run with `cargo run --release --bin hive-bench -- --quiet`.

use std::env;
use std::process::ExitCode;
use std::time::Instant;

use hive::brain::Brain;
use hive::compress::{Message, RuleFast};
use hive::label::Label;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let quiet = args.iter().any(|a| a == "--quiet");

    let t0 = Instant::now();
    let mut comb = RuleFast::new();
    let brain = Brain::new();
    let n = 2_000usize;

    for i in 0..n {
        let role = match i % 4 {
            0 => "system",
            1 => "user",
            2 => "assistant",
            _ => "tool",
        };
        let content = synth(role, i);
        comb.process(&Message::new(role, content));
        brain
            .remember(format!("k{i:04}"), serde_json::json!({"i": i}), 0.5)
            .expect("remember");
    }
    let elapsed = t0.elapsed();

    if !quiet {
        let stats = comb.stats();
        let bs = brain.stats();
        println!("=== hive-cpp bench ===");
        println!("turns:           {n}");
        println!("wall:            {:.3} s", elapsed.as_secs_f64());
        println!(
            "compress rate:   {:.0} msg/s",
            n as f64 / elapsed.as_secs_f64()
        );
        println!(
            "brain rate:      {:.0} writes/s",
            n as f64 / elapsed.as_secs_f64()
        );
        println!(
            "compression:      {:.2}x ({} → {} tok)",
            stats.compression_ratio, stats.original_tokens, stats.total_tokens
        );
        println!("brain nodes:     {}", bs.node_count);
    }
    ExitCode::SUCCESS
}

fn synth(role: &str, i: usize) -> String {
    // Cheap content; matches the Python `make_transcript` shape just
    // enough to produce a realistic distribution of content types.
    match (i % 8, role) {
        (0, _) => format!(
            "{}",
            (0..60)
                .map(|n| format!("  test_{:04} ... {}", n, if n % 7 == 0 { "FAIL" } else { "ok" }))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        (1, _) => format!("import os\nclass Foo:\n    def bar(self):\n        return 1\n{}", "x".repeat(2000)),
        (2, "tool") => "src/auth.py:42: def login():\nsrc/auth.py:43:     return user".to_string(),
        (3, "user") => format!("fix the auth bug {}", i),
        _ => format!("step {} on path A", i),
    }
}
