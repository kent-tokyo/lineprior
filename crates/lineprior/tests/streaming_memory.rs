//! Regression guard: `build_prior_book_from_reader` must keep peak memory
//! bounded by the number of unique `(state, action)` pairs, not the number
//! of observations read.
//!
//! This lives in its own integration-test file (its own process) rather
//! than alongside the unit tests in `src/`, because `cargo test` runs unit
//! tests in parallel threads within a single process -- an in-process
//! peak-RSS measurement taken there would be corrupted by whatever else is
//! allocating concurrently. A lone test in its own binary measures only
//! its own work.
//!
//! Linux-only (reads `/proc/self/status`) -- our CI runs `ubuntu-latest`,
//! so this still executes there even though it no-ops on other platforms.
#![cfg(target_os = "linux")]

use lineprior::{BuildConfig, build_prior_book_from_reader};
use std::io::Read;

/// Generates JSONL lines for a small, fixed number of unique
/// `(state, action)` pairs on demand, one line at a time. Never holds
/// more than one line's worth of bytes -- if it did, this test would be
/// measuring its own generator's memory instead of the code under test.
struct SyntheticJsonl {
    next: u64,
    total: u64,
    unique_pairs: u64,
    buf: Vec<u8>,
    pos: usize,
}

impl SyntheticJsonl {
    fn new(total: u64, unique_pairs: u64) -> Self {
        Self {
            next: 0,
            total,
            unique_pairs,
            buf: Vec::new(),
            pos: 0,
        }
    }
}

impl Read for SyntheticJsonl {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            if self.next >= self.total {
                return Ok(0);
            }
            let pair = self.next % self.unique_pairs;
            self.buf = format!(
                "{{\"sequence_id\":\"s{}\",\"step\":0,\"state\":\"state_{pair}\",\"action\":\"action_{pair}\",\"outcome\":\"success\",\"weight\":1.0}}\n",
                self.next,
            )
            .into_bytes();
            self.pos = 0;
            self.next += 1;
        }
        let n = (self.buf.len() - self.pos).min(out.len());
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Peak resident set size ever reached by this process, in KB.
fn vm_hwm_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("read /proc/self/status");
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .expect("parse VmHWM value");
        }
    }
    panic!("VmHWM not found in /proc/self/status");
}

/// Like `SyntheticJsonl`, but emits 2-step sequences (state "state_a" then
/// "state_b", with step 0's action determining the small pool of
/// `(context, state, action)` tuples) -- so `context_order = 1` has real
/// context entries to build from, still bounded to `unique_pairs` distinct
/// tuples despite many observations. Emits both steps of one sequence
/// consecutively, in increasing `step` order, satisfying `context_order >
/// 0`'s sortedness precondition by construction.
struct SyntheticContextJsonl {
    next_sequence: u64,
    total_sequences: u64,
    unique_pairs: u64,
    step: u32,
    buf: Vec<u8>,
    pos: usize,
}

impl SyntheticContextJsonl {
    fn new(total_sequences: u64, unique_pairs: u64) -> Self {
        Self {
            next_sequence: 0,
            total_sequences,
            unique_pairs,
            step: 0,
            buf: Vec::new(),
            pos: 0,
        }
    }
}

impl Read for SyntheticContextJsonl {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            if self.next_sequence >= self.total_sequences {
                return Ok(0);
            }
            let pair = self.next_sequence % self.unique_pairs;
            let state = if self.step == 0 { "state_a" } else { "state_b" };
            self.buf = format!(
                "{{\"sequence_id\":\"seq{}\",\"step\":{},\"state\":\"{state}\",\"action\":\"action_{pair}\",\"outcome\":\"success\",\"weight\":1.0}}\n",
                self.next_sequence, self.step,
            )
            .into_bytes();
            self.pos = 0;
            if self.step == 0 {
                self.step = 1;
            } else {
                self.step = 0;
                self.next_sequence += 1;
            }
        }
        let n = (self.buf.len() - self.pos).min(out.len());
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[test]
fn streaming_build_with_context_order_keeps_peak_memory_bounded_by_unique_tuples() {
    const TOTAL_SEQUENCES: u64 = 1_500_000; // 3,000,000 observations total
    const UNIQUE_PAIRS: u64 = 100;

    let before_kb = vm_hwm_kb();

    let reader = SyntheticContextJsonl::new(TOTAL_SEQUENCES, UNIQUE_PAIRS);
    let config = BuildConfig {
        context_order: 1,
        ..Default::default()
    };
    let output = build_prior_book_from_reader(reader, false, &config).unwrap();

    let after_kb = vm_hwm_kb();
    let delta_kb = after_kb.saturating_sub(before_kb);

    // Correctness: order-0 has 2 states (state_a, state_b) x UNIQUE_PAIRS
    // actions each; context (order-1) has UNIQUE_PAIRS entries (one per
    // distinct step-0 action, each pointing to exactly one step-1 action) --
    // neither scales with TOTAL_SEQUENCES.
    let total_actions: u64 = output.book.entries.values().map(Vec::len).sum::<usize>() as u64;
    assert_eq!(total_actions, 2 * UNIQUE_PAIRS);
    let total_context_actions: u64 = output
        .book
        .context_entries
        .values()
        .map(Vec::len)
        .sum::<usize>() as u64;
    assert_eq!(total_context_actions, UNIQUE_PAIRS);

    // Generous threshold (same reasoning as the order-0 test above, plus
    // headroom for the extra context-tracking bookkeeping) -- still tiny
    // next to what 3,000,000 owned `Observation`s collected into a `Vec`
    // first would cost.
    assert!(
        delta_kb < 150_000,
        "peak memory grew by {delta_kb}KB while processing {} observations across only \
         {UNIQUE_PAIRS} unique (context, state, action) tuples -- this suggests \
         context_order > 0 is no longer streaming",
        TOTAL_SEQUENCES * 2,
    );
}

#[test]
fn streaming_build_keeps_peak_memory_bounded_by_unique_pairs_not_total_observations() {
    const TOTAL_OBSERVATIONS: u64 = 3_000_000;
    const UNIQUE_PAIRS: u64 = 100;

    let before_kb = vm_hwm_kb();

    let reader = SyntheticJsonl::new(TOTAL_OBSERVATIONS, UNIQUE_PAIRS);
    let output = build_prior_book_from_reader(reader, false, &BuildConfig::default()).unwrap();

    let after_kb = vm_hwm_kb();
    let delta_kb = after_kb.saturating_sub(before_kb);

    // Correctness check: still only the small number of unique pairs, not
    // one output entry per observation.
    let total_actions: u64 = output.book.entries.values().map(Vec::len).sum::<usize>() as u64;
    assert_eq!(total_actions, UNIQUE_PAIRS);

    // 3,000,000 owned `Observation` values (each holding several
    // heap-allocated `String`s) would need on the order of several
    // hundred MB if collected into a `Vec` first. Bounded aggregation
    // needs a small fraction of that. The threshold is generous to absorb
    // allocator/CI noise while still catching a reintroduced full
    // materialization.
    assert!(
        delta_kb < 100_000,
        "peak memory grew by {delta_kb}KB while processing {TOTAL_OBSERVATIONS} observations \
         across only {UNIQUE_PAIRS} unique pairs -- this suggests build_prior_book_from_reader \
         is no longer streaming (e.g. a Vec<Observation> got reintroduced somewhere)"
    );
}
