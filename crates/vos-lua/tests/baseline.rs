#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// a measurement run, not a check
// the arithmetic is on counts and byte sizes far below 2^52, for a printed report
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
//! A baseline of the VM's runtime characteristics (AGOS-53), run on demand:
//!
//! ```sh
//! cargo test --release -p vos-lua --test baseline -- --ignored --nocapture
//! VOS_BASELINE_VMS=200 cargo test --release -p vos-lua --test baseline -- --ignored --nocapture
//! ```
//!
//! It loads the built Calculator (the shipped bundle: darklua, then `AppVm::load`: the engine, the host API, the
//! manifest and the window and tool declarations) into `N` VMs (default 50) and prints, per VM, how long the load
//! took and what the Lua heap holds (`AppVm::used_memory`), plus the process's resident set before and after, which
//! also counts what Rust allocated for each VM (the database handle, the tables of the host API). The numbers go
//! into the table in `docs/architecture/host-stack.md` (§1, "Measured baseline"): rerun this to see whether a change
//! made the VM slower or larger. It asserts nothing about the numbers (they depend on the machine), only that
//! every VM loaded.

mod common;

use vos_lua::{AppVm, VmConfig, UI_MODULE};
use common::{built_app, db, id};
use std::{
    process::Command,
    time::{Duration, Instant},
};

/// How many VMs to create when `VOS_BASELINE_VMS` is not set.
const DEFAULT_VMS: usize = 50;

/// The config the host gives every app: the generated `vos:ui` module, and no agent roster.
fn config() -> VmConfig {
    VmConfig {
        host_modules: std::collections::HashMap::from([("ui".to_owned(), UI_MODULE.as_bytes().to_vec())])
            .into(),
        ..VmConfig::default()
    }
}

/// The process's resident set in KiB (`ps`, so it works on macOS and Linux), or 0 when `ps` is not there.
fn rss_kib() -> u64 {
    Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// The `p`th percentile (0.0 to 1.0) of an ascending list.
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let last = sorted.len() - 1;
    sorted[((last as f64) * p).round() as usize]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measurement, run on demand (AGOS-53): cargo test --release -p vos-lua --test baseline -- --ignored --nocapture"]
async fn calculator_vm_create_time_and_memory() {
    let count: usize =
        std::env::var("VOS_BASELINE_VMS").ok().and_then(|n| n.parse().ok()).unwrap_or(DEFAULT_VMS);
    assert!(count >= 2, "VOS_BASELINE_VMS must be at least 2");
    let package = built_app("calculator");
    let bundle_bytes: usize = package.files.values().map(Vec::len).sum();

    // one VM first, dropped: the process's own warm-up (allocator, lazy statics) is not a VM's cost
    drop(AppVm::load(&id("calculator"), package.clone(), db(), config()).await.unwrap());

    let rss_before = rss_kib();
    let mut vms = Vec::with_capacity(count);
    let mut created = Vec::with_capacity(count);
    for _ in 0..count {
        let started = Instant::now();
        let vm = AppVm::load(&id("calculator"), package.clone(), db(), config()).await.unwrap();
        created.push(started.elapsed());
        vms.push(vm);
    }
    let rss_after = rss_kib();

    let heap: Vec<usize> = vms.iter().map(AppVm::used_memory).collect();
    let (first, rest) = (created[0], &created[1..]);
    let mut sorted = rest.to_vec();
    sorted.sort();
    let mean = sorted.iter().sum::<Duration>() / u32::try_from(sorted.len()).unwrap();
    let heap_kib = |bytes: usize| bytes as f64 / 1024.0;

    println!("calculator bundle: {bundle_bytes} bytes; {count} VMs");
    println!("create time per VM (ms): first {:.2}, then min {:.2}, median {:.2}, mean {:.2}, p95 {:.2}, max {:.2}",
        ms(first),
        ms(sorted[0]),
        ms(percentile(&sorted, 0.5)),
        ms(mean),
        ms(percentile(&sorted, 0.95)),
        ms(*sorted.last().unwrap()),
    );
    println!(
        "Lua heap per VM (KiB): min {:.1}, max {:.1}",
        heap_kib(*heap.iter().min().unwrap()),
        heap_kib(*heap.iter().max().unwrap()),
    );
    if rss_before > 0 {
        println!(
            "process resident set: {rss_before} KiB before, {rss_after} KiB after {count} VMs: {:.1} KiB per VM",
            (rss_after as f64 - rss_before as f64) / count as f64,
        );
    }
    assert_eq!(vms.len(), count);
}
