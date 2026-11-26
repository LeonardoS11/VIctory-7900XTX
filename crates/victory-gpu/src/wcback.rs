//! `VICTORY_SKIP_WC_BACK`, an experiment (after fxceless/victory-fork#868):
//! what shaders wrote into a card copy of title memory that the title
//! reserved write-combined is not fetched back into title memory. The
//! title's CPU does not read write-combined memory in the ordinary way, so
//! the fetch is thought to serve nobody there.
//!
//! - `count`: nothing changes; the pieces that would be left on the card
//!   are counted and logged beside the others.
//! - `1`: those pieces are left on the card. Title memory keeps its old
//!   bytes there, so whatever the host later takes from title memory in
//!   such a piece (a new copy, a texture made over it, an upload of a page
//!   the CPU wrote part of) is stale: expect picture faults.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// The console's protection bit for write-combined memory.
const WRITE_COMBINED: u32 = 0x400;

/// Reservations of `XMemVirtualAlloc`: start to (length, the protection the
/// title asked for).
static RESERVATIONS: Mutex<BTreeMap<usize, (usize, u32)>> = Mutex::new(BTreeMap::new());

/// Pieces and bytes: left on the card (or, counting, that would be), and
/// fetched.
static LEFT: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static FETCHED: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// 0 off, 1 counting, 2 leaving the pieces on the card.
fn mode() -> u8 {
    static MODE: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("VICTORY_SKIP_WC_BACK").as_deref() {
        Ok("1") => {
            tracing::warn!("VICTORY_SKIP_WC_BACK=1: shader writes to write-combined title memory are not copied back (experiment)");
            2
        }
        Ok("count") => 1,
        _ => 0,
    })
}

/// The runtime reserved `[at, at + len)` for the title with the console
/// protection `protect` (before the runtime cleans it).
pub fn title_memory_reserved(at: usize, len: usize, protect: u32) {
    if mode() == 0 || at == 0 || len == 0 {
        return;
    }
    RESERVATIONS.lock().unwrap_or_else(|e| e.into_inner()).insert(at, (len, protect));
}

/// The smallest host copy whose pieces are left on the card
/// (`VICTORY_SKIP_WC_BACK_MIN_KB`, 1024): with every title buffer in the
/// card's memory (`VICTORY_CARD_BUFFERS_MIN_KB=64`) the small ones hold
/// what the title's CPU does read (counts, lists of what is seen), and a
/// run that left those on the card drew half the frame, black.
fn least() -> u64 {
    static LEAST: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *LEAST.get_or_init(|| {
        std::env::var("VICTORY_SKIP_WC_BACK_MIN_KB").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(1024) << 10
    })
}

/// Whether the piece `[from, to)` of title memory, which shaders wrote on
/// the card in a host copy of `copy` bytes, is left there.
pub(crate) fn skip(from: usize, to: usize, copy: u64) -> bool {
    let mode = mode();
    if mode == 0 || from >= to || copy < least() {
        return false;
    }
    let combined = RESERVATIONS.lock().unwrap_or_else(|e| e.into_inner()).range(..to).next_back().is_some_and(
        |(&base, &(len, protect))| base <= from && protect & WRITE_COMBINED != 0 && base.checked_add(len).is_some_and(|end| to <= end),
    );
    let counter = if combined { &LEFT } else { &FETCHED };
    let n = counter[0].fetch_add(1, Relaxed) + 1;
    counter[1].fetch_add((to - from) as u64, Relaxed);
    if combined && n.is_power_of_two() {
        tracing::info!(
            "VICTORY_SKIP_WC_BACK ({}): {} pieces {} MB of write-combined memory, {} pieces {} MB of other memory so far",
            if mode == 2 { "left on the card" } else { "counting only" },
            n,
            LEFT[1].load(Relaxed) >> 20,
            FETCHED[0].load(Relaxed),
            FETCHED[1].load(Relaxed) >> 20
        );
    }
    combined && mode == 2
}
