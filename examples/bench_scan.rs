//! Benchmark SessionCache::refresh against the real ~/.claude/projects.
//! Usage: cargo run --release --example bench_scan

#![allow(dead_code)]

#[path = "../src/registry.rs"]
mod registry;
#[path = "../src/scan.rs"]
mod scan;
#[path = "../src/sessions.rs"]
mod sessions;

use std::time::Instant;

use sessions::SessionCache;

fn main() {
    let Some(home) = dirs::home_dir() else {
        eprintln!("no home dir available; skipping bench");
        return;
    };
    let claude_dir = home.join(".claude");
    if !claude_dir.join("projects").is_dir() {
        eprintln!("{} not found; skipping bench", claude_dir.display());
        return;
    }
    let mut cache = SessionCache::new();

    let t0 = Instant::now();
    cache.refresh(&claude_dir);
    let cold_elapsed = t0.elapsed();
    let n_cold = cache.entries().len();

    let t1 = Instant::now();
    cache.refresh(&claude_dir);
    let warm_elapsed = t1.elapsed();
    let n_warm = cache.entries().len();

    let t2 = Instant::now();
    let reg = registry::Registry::load_all(&claude_dir);
    let reg_elapsed = t2.elapsed();

    println!("Sessions discovered: {}", n_cold);
    println!("Registry entries:    {}", reg.len());
    println!("Cold refresh:        {:?}", cold_elapsed);
    println!("Warm refresh:        {:?}", warm_elapsed);
    println!("Registry load:       {:?}", reg_elapsed);
    if n_cold != n_warm {
        eprintln!(
            "note: entry count changed between cold ({n_cold}) and warm ({n_warm}) refresh \
             — probably a concurrent session update"
        );
    }
}
