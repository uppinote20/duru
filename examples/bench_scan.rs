//! Benchmark SessionCache::refresh against the real ~/.claude/projects.
//! Usage: cargo run --release --example bench_scan

#![allow(dead_code)]

#[path = "../src/scan.rs"]
mod scan;
#[path = "../src/sessions.rs"]
mod sessions;

use std::time::Instant;

use sessions::SessionCache;

fn main() {
    let claude_dir = dirs::home_dir().unwrap().join(".claude");
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
    cache.refresh(&claude_dir);
    let warm2_elapsed = t2.elapsed();

    println!("Sessions discovered: {}", n_cold);
    println!("Cold refresh (first scan):  {:?}", cold_elapsed);
    println!("Warm refresh (mtime cache): {:?}", warm_elapsed);
    println!("Warm refresh (again):       {:?}", warm2_elapsed);
    println!();
    assert_eq!(n_cold, n_warm);
}
