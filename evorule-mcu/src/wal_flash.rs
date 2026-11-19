// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Flash WAL — the pure-logic layer of the write-ahead fact ledger
//! (medium abstraction + ring-of-pages layout + power-cut recovery)
//!
//! # Layers
//! - [`FlashMedium`]: medium abstraction (read / program is 1→0 only /
//!   erase_page). Real board media (STM32F4 sectors / CH32V307 page erase)
//!   implement the same trait at real-hardware bring-up; for now
//!   [`RamFlashMedium`] (a `Vec<u8>` simulation initialized to all 0xFF)
//!   covers the full logic surface in tests
//! - [`FlashWalStore`]: a JSONL line ledger over a ring of pages, with line
//!   format byte-identical to the RAM backend / native `WalWriter`
//!   (same construction source [`crate::wal`])
//!
//! # Layout parameterization (kept in sync with board-level memory.x)
//! Ring = `page_count` equal-size pages; F407 = sectors 0-3 (16KB × 4 @
//! 0x08000000), V307 = tail-page ring (4KB × 8 @ 0x08038000). A page is the
//! write atomicity boundary: **a single line never spans pages** (a line
//! that does not fit the current page rotates to the next page; a line
//! larger than the page capacity fails visibly).
//!
//! # Flash semantics and the power-cut window
//! - **Programming is 1→0 only**: before program, the full check
//!   `(old & new) == new` is performed; violations fail visibly and leave
//!   the medium untouched (the check precedes the write; no half-way state)
//! - **Commit boundary = the trailing `\n` sentinel**: `append` programs
//!   byte by byte (line + `\n`); within the power-cut window at most one
//!   half-line record can remain — a trailing byte segment without `\n`
//!   does not constitute a record
//! - **Recovery = prefix scan and chain continuation**: [`FlashWalStore::open`]
//!   scans page by page and only accepts complete lines; the chain tail hash
//!   is restored from the last complete line's `chain_hash`, and subsequent
//!   appends continue the chain unchanged
//! - **Polluted page**: the half-line residue region left by a power cut
//!   cannot be reprogrammed (1→0 is irreversible); the page remains a
//!   read-only remnant and new records automatically advance to the next
//!   empty page (at most one line of page capacity is lost)
//! - **Ring full fails visibly** ([`McuError::WalFull`]): the audit chain is
//!   never silently dropped — no wrap-around overwrite of the oldest
//!   records; wait for external export / re-erase
//!
//! # Consistency invariants (violations surface as [`McuError::WalCorrupted`])
//! - Within a page: everything after the first 0xFF must be all 0xFF
//!   (sequential program)
//! - Within the ring: no holes — an empty page must not be followed by a data page
//! - Page rotation: the target page must be empty (protects data pages from accidental erase)

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use evorule_audit_chain::fact::Fact;

use crate::error::McuError;
use crate::wal::{wal_line_json, McuWalStore};

/// Flash medium abstraction (offsets are relative to the WAL ring base; real
/// boards map them to absolute Flash addresses)
pub trait FlashMedium {
    /// Page capacity (bytes); the write/erase atomicity boundary of the ring layout
    fn page_size(&self) -> usize;
    /// Number of pages
    fn page_count(&self) -> usize;
    /// Read `buf.len()` bytes into `buf`
    ///
    /// # Errors
    /// Returns [`McuError::FlashRange`] when the offset is out of range.
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<(), McuError>;
    /// Program (1→0 only)
    ///
    /// # Errors
    /// Returns [`McuError::FlashRange`] when the offset is out of range;
    /// returns [`McuError::FlashProgramViolated`] when a target byte contains
    /// a 0→1 bit (fail-visible; the check precedes the write and the medium
    /// is untouched).
    fn program(&mut self, offset: usize, bytes: &[u8]) -> Result<(), McuError>;
    /// Erase a whole page (set to 0xFF)
    ///
    /// # Errors
    /// Returns [`McuError::FlashRange`] when the page number is out of range.
    fn erase_page(&mut self, page: usize) -> Result<(), McuError>;
}

/// RAM-simulated Flash medium (`Vec<u8>`, fully faithful 0xFF semantics)
///
/// Carrier for power-cut truncation injection: tests write a half line of
/// bytes via [`FlashMedium::program`] to simulate power-cut residue, then
/// verify recovery semantics via [`FlashWalStore::open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RamFlashMedium {
    data: Vec<u8>,
    page_size: usize,
}

impl RamFlashMedium {
    /// Create the simulated medium filled with 0xFF
    ///
    /// # Panics (host-test convenience path, not the no_std runtime surface)
    /// `page_size == 0` (division-by-zero protection is left to the caller;
    /// real board media do not go through this constructor).
    #[allow(clippy::panic)]
    pub fn new(page_size: usize, page_count: usize) -> Self {
        assert!(page_size > 0, "page_size must be non-zero");
        Self {
            data: vec![0xFF; page_size.saturating_mul(page_count)],
            page_size,
        }
    }
}

impl FlashMedium for RamFlashMedium {
    fn page_size(&self) -> usize {
        self.page_size
    }

    fn page_count(&self) -> usize {
        self.data.len() / self.page_size
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<(), McuError> {
        let end = offset
            .checked_add(buf.len())
            .ok_or(McuError::FlashRange { offset })?;
        if end > self.data.len() {
            return Err(McuError::FlashRange { offset });
        }
        buf.copy_from_slice(&self.data[offset..end]);
        Ok(())
    }

    fn program(&mut self, offset: usize, bytes: &[u8]) -> Result<(), McuError> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(McuError::FlashRange { offset })?;
        if end > self.data.len() {
            return Err(McuError::FlashRange { offset });
        }
        // Full check before write (no half-way state: the medium stays untouched on violation)
        for (i, b) in bytes.iter().enumerate() {
            if self.data[offset + i] & *b != *b {
                return Err(McuError::FlashProgramViolated { offset: offset + i });
            }
        }
        for (i, b) in bytes.iter().enumerate() {
            self.data[offset + i] &= *b;
        }
        Ok(())
    }

    fn erase_page(&mut self, page: usize) -> Result<(), McuError> {
        let start = page
            .checked_mul(self.page_size)
            .ok_or(McuError::FlashRange { offset: page })?;
        let end = start + self.page_size;
        if end > self.data.len() {
            return Err(McuError::FlashRange { offset: start });
        }
        self.data[start..end].fill(0xFF);
        Ok(())
    }
}

/// Page scan result: complete line ranges (start/end within the page) + the
/// power-cut half-line residue marker
struct PageScan {
    /// Complete line ranges (terminated by \n)
    rows: Vec<(usize, usize)>,
    /// Whether the page holds power-cut half-line residue (a non-0xFF byte
    /// segment after \n)
    polluted: bool,
    /// Position of the first 0xFF within the page (None = page full)
    first_free: Option<usize>,
}

/// Scan one page: produce the complete lines within it + the power-cut
/// half-line residue marker
///
/// In-page shape constraint (sequential-program invariant): everything after
/// the first 0xFF must be all 0xFF; violations surface as `WalCorrupted`.
fn scan_page(data: &[u8], page: usize) -> Result<PageScan, McuError> {
    let mut rows = Vec::new();
    let mut row_start = 0usize;
    let mut first_free = None;
    for (i, b) in data.iter().enumerate() {
        if *b == 0xFF {
            first_free = Some(i);
            break;
        }
        if *b == b'\n' {
            rows.push((row_start, i));
            row_start = i + 1;
        }
    }
    match first_free {
        None => {
            // Page full: a residue segment after \n (if any) does not constitute
            // a record but is semantically the same as power-cut half-line
            // residue; a full page triggers rotation by itself, so the residue
            // marker does not affect the write pointer (page full already
            // forces rotation)
            Ok(PageScan {
                rows,
                polluted: false,
                first_free: None,
            })
        }
        Some(free) => {
            if data[free..].iter().any(|b| *b != 0xFF) {
                return Err(McuError::WalCorrupted(format!(
                    "page {page}: non-0xFF byte after free space boundary"
                )));
            }
            let polluted = row_start < free;
            Ok(PageScan {
                rows,
                polluted,
                first_free: Some(free),
            })
        }
    }
}

/// Flash WAL backend (a JSONL line ledger over a ring of pages)
///
/// Line contents are fully cached in RAM (a convenience surface for parity
/// reads and auditing); the line-cache budget for board instantiation
/// (ring capacity vs kernel heap pool) is re-reviewed at real-hardware
/// bring-up. The medium is injected via [`FlashMedium`].
pub struct FlashWalStore<M: FlashMedium> {
    medium: M,
    /// Complete line contents (without `\n`, in ledger order)
    lines: Vec<String>,
    /// Start offset of each line on the medium (audit location/export surface)
    line_offsets: Vec<usize>,
    /// Chain tail hash cache (restored from the last complete line at open;
    /// appends continue the chain unchanged)
    tail_hash: String,
    /// Active page number (the write target page)
    active_page: usize,
    /// In-page write pointer (bytes); `page_size` means the page is full and
    /// pending rotation
    write_offset: usize,
    /// Whether the active page holds power-cut half-line residue (polluted:
    /// not reprogrammable; append forces rotation)
    polluted: bool,
    /// Cached page capacity (taken from the medium at open)
    page_size: usize,
}

impl<M: FlashMedium> core::fmt::Debug for FlashWalStore<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FlashWalStore")
            .field("lines", &self.lines.len())
            .field("tail_hash", &self.tail_hash)
            .field("active_page", &self.active_page)
            .field("write_offset", &self.write_offset)
            .field("polluted", &self.polluted)
            .field("page_size", &self.page_size)
            .finish_non_exhaustive()
    }
}

/// Parse a single line's `chain_hash` (used by the recovery scan: the anchor
/// for continuing the chain tail)
fn line_chain_hash(line: &str, page: usize) -> Result<String, McuError> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| McuError::WalCorrupted(format!("page {page}: line is not valid JSON: {e}")))?;
    let hash = value
        .get("chain_hash")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            McuError::WalCorrupted(format!("page {page}: line missing chain_hash field"))
        })?;
    Ok(String::from(hash))
}

impl<M: FlashMedium> FlashWalStore<M> {
    /// Open (recover) the WAL ring: prefix scan over the whole ring, accepting
    /// only complete lines, ready to continue the chain
    ///
    /// # Errors
    /// - [`McuError::WalCorrupted`]: illegal in-page shape (bytes after 0xFF) /
    ///   ring hole (a data page after an empty page) / line is not JSON /
    ///   line missing chain_hash
    /// - [`McuError::FlashRange`]: medium read out of range (theoretically
    ///   unreachable, inherent to the page traversal)
    pub fn open(medium: M) -> Result<Self, McuError> {
        let page_size = medium.page_size();
        let page_count = medium.page_count();
        let mut lines = Vec::new();
        let mut line_offsets = Vec::new();
        let mut tail_hash = String::from("genesis");
        let mut active_page = 0usize;
        let mut write_offset = 0usize;
        let mut polluted = false;
        let mut saw_empty_page = false;

        let mut buf = vec![0u8; page_size];
        for page in 0..page_count {
            let base = page * page_size;
            medium.read(base, &mut buf)?;
            if buf.iter().all(|b| *b == 0xFF) {
                saw_empty_page = true;
                continue;
            }
            if saw_empty_page {
                return Err(McuError::WalCorrupted(format!(
                    "page {page}: data page after empty page (ring hole)"
                )));
            }
            let scan = scan_page(&buf, page)?;
            for (start, end) in scan.rows {
                let line = core::str::from_utf8(&buf[start..end]).map_err(|_| {
                    McuError::WalCorrupted(format!("page {page}: line is not valid UTF-8"))
                })?;
                tail_hash = line_chain_hash(line, page)?;
                line_offsets.push(base + start);
                lines.push(String::from(line));
            }
            active_page = page;
            write_offset = scan.first_free.unwrap_or(page_size); // None = page full
            polluted = scan.polluted;
        }

        Ok(Self {
            medium,
            lines,
            line_offsets,
            tail_hash,
            active_page,
            write_offset,
            polluted,
            page_size,
        })
    }

    /// Medium reference (observation/export surface)
    pub fn medium(&self) -> &M {
        &self.medium
    }

    /// Start offset of each line on the medium (audit location surface)
    pub fn line_offsets(&self) -> &[usize] {
        &self.line_offsets
    }

    /// Whether the active page holds power-cut half-line residue (audit observation surface)
    pub fn is_polluted(&self) -> bool {
        self.polluted
    }

    /// Mutable medium reference (test-only power-cut injection; not exposed in production)
    #[cfg(test)]
    pub(crate) fn medium_mut_for_test(&mut self) -> &mut M {
        &mut self.medium
    }
}

impl<M: FlashMedium> McuWalStore for FlashWalStore<M> {
    /// Append one line (write-ahead: a successful program promises persistence)
    ///
    /// A line that does not fit the current page (or a polluted active page) →
    /// verify and erase the next page, then write; a line larger than the page
    /// capacity / ring full / program violation → fail visibly.
    /// If program fails, the store pointers have already advanced (the new
    /// page is empty and writable); the line ledger `lines` stays consistent
    /// with the lines actually on the medium (unpersisted lines are not
    /// ledgered).
    fn append_record(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), McuError> {
        let line = wal_line_json(version_before, fact, content_hash, prev_hash, chain_hash)?;
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');

        if bytes.len() > self.page_size {
            return Err(McuError::WalRecordTooLarge {
                line_len: bytes.len(),
                page_size: self.page_size,
            });
        }

        // Rotation decision: page full / insufficient remaining space /
        // polluted active page (half-line residue is not reprogrammable)
        let need_new_page = self.polluted
            || self.write_offset >= self.page_size
            || self.page_size - self.write_offset < bytes.len();
        if need_new_page {
            let next = self.active_page + 1;
            if next >= self.medium.page_count() {
                return Err(McuError::WalFull);
            }
            // Rotation defense: the target page must be empty (sequential-write
            // invariant; protects data pages from accidental erase)
            let base = next * self.page_size;
            let mut probe = [0u8; 1];
            self.medium.read(base, &mut probe)?;
            if probe[0] != 0xFF {
                return Err(McuError::WalCorrupted(format!(
                    "page {next}: expected empty page before rotate, found data"
                )));
            }
            self.medium.erase_page(next)?;
            self.active_page = next;
            self.write_offset = 0;
            self.polluted = false;
        }

        let offset = self.active_page * self.page_size + self.write_offset;
        self.medium.program(offset, &bytes)?;
        self.write_offset += bytes.len();
        self.line_offsets.push(offset);
        self.lines.push(line);
        self.tail_hash = String::from(chain_hash);
        Ok(())
    }

    /// Current chain tail hash (`"genesis"` for an empty ring)
    fn chain_tail_hash(&self) -> &str {
        &self.tail_hash
    }

    /// Number of persisted complete lines
    fn len(&self) -> usize {
        self.lines.len()
    }

    /// JSONL line content of line `index` (without `\n`, byte-identical to the RAM backend)
    fn line_json(&self, index: usize) -> Result<String, McuError> {
        self.lines.get(index).cloned().ok_or(McuError::RecordIndex {
            index,
            len: self.lines.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::exec::parity_fixtures::command_fact;
    use evorule_tcb::JsonValue;

    /// Deterministic LCG (reproducible random power-cut injection; no rand
    /// dependency on the no_std surface)
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    /// Clean empty medium (page size 256 / 8 pages, line ~170B → exactly one
    /// line per page, naturally exercising rotation)
    fn medium() -> RamFlashMedium {
        RamFlashMedium::new(256, 8)
    }

    /// Build the full line bytes of the record at version `version_before`
    /// (line + \n, using the RAM backend to compute the line)
    fn full_line_bytes(version_before: u64, prev_hash: &str, chain_hash: &str) -> String {
        let fact = command_fact(version_before + 1, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        let mut tmp = crate::wal::RamWalStore::new();
        tmp.append_record(version_before, &fact, &content, prev_hash, chain_hash)
            .unwrap();
        format!("{}\n", tmp.line_json(0).unwrap())
    }

    /// Append a tagged record via the trait (chain_hash = "{tag}-{version}", unique per record)
    fn append_tagged(store: &mut FlashWalStore<RamFlashMedium>, version_before: u64, tag: &str) {
        let fact = command_fact(version_before + 1, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        let prev = String::from(store.chain_tail_hash());
        let chain = alloc::format!("{tag}-{version_before}");
        store
            .append_record(version_before, &fact, &content, &prev, &chain)
            .unwrap();
    }

    #[test]
    fn test_program_only_1_to_0() {
        let mut m = medium();
        // Write 0b1010 then 0b1000: every new bit falls on an old 1 bit
        // (legal, result 0b1000)
        m.program(0, &[0b1010]).unwrap();
        m.program(0, &[0b1000]).unwrap();
        let mut out = [0u8; 1];
        m.read(0, &mut out).unwrap();
        assert_eq!(out[0], 0b1000);
        // Write a byte containing a 0→1 bit (bit2 old 0, new 1): fails
        // visibly and the medium stays untouched
        let err = m.program(0, &[0b0100]).unwrap_err();
        assert!(matches!(err, McuError::FlashProgramViolated { offset: 0 }));
        m.read(0, &mut out).unwrap();
        assert_eq!(out[0], 0b1000, "a violating program must not modify the medium");
    }

    #[test]
    fn test_erase_page_resets_to_ff() {
        let mut m = medium();
        m.program(0, &[0x00, 0x00]).unwrap();
        m.erase_page(0).unwrap();
        let mut out = [0u8; 2];
        m.read(0, &mut out).unwrap();
        assert_eq!(out, [0xFF, 0xFF]);
        // Out-of-range page number fails visibly
        assert!(matches!(
            m.erase_page(8).unwrap_err(),
            McuError::FlashRange { .. }
        ));
    }

    #[test]
    fn test_range_checks() {
        let m = medium();
        let mut out = [0u8; 4];
        assert!(matches!(
            m.read(256 * 8 - 2, &mut out).unwrap_err(),
            McuError::FlashRange { .. }
        ));
        let mut m = medium();
        assert!(matches!(
            m.program(256 * 8, &[0xFF]).unwrap_err(),
            McuError::FlashRange { .. }
        ));
    }

    #[test]
    fn test_append_and_line_json_matches_ram_backend() {
        let mut ram = crate::wal::RamWalStore::new();
        let mut store = FlashWalStore::open(medium()).unwrap();

        let fact = command_fact(1, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        ram.append_record(0, &fact, &content, "genesis", "chain-1")
            .unwrap();
        store
            .append_record(0, &fact, &content, "genesis", "chain-1")
            .unwrap();

        // Line contents are byte-identical (same construction source + same input)
        assert_eq!(store.line_json(0).unwrap(), ram.line_json(0).unwrap());
        assert_eq!(store.len(), 1);
        assert_eq!(store.chain_tail_hash(), "chain-1");
    }

    #[test]
    fn test_multi_line_page_rotation() {
        let mut store = FlashWalStore::open(medium()).unwrap();
        for v in 0..6u64 {
            append_tagged(&mut store, v, "t");
        }
        assert_eq!(store.len(), 6);
        // A line ~200B (with \n) > page 256B fits only one line → 6 lines
        // should span 6 pages
        for (i, off) in store.line_offsets().iter().enumerate() {
            assert_eq!(off / 256, i, "line {i} should land on page {i}");
        }
        assert_eq!(store.chain_tail_hash(), "t-5");
    }

    #[test]
    fn test_ring_full_fails_visible() {
        let mut store = FlashWalStore::open(medium()).unwrap();
        for v in 0..8u64 {
            append_tagged(&mut store, v, "t");
        }
        assert_eq!(store.len(), 8);
        // The 9th line → ring full fails visibly (no wrap-around overwrite)
        let fact = command_fact(9, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        let prev = String::from(store.chain_tail_hash());
        let err = store
            .append_record(8, &fact, &content, &prev, "chain-9")
            .unwrap_err();
        assert!(matches!(err, McuError::WalFull), "got: {err}");
        // The original chain is unaffected
        assert_eq!(store.len(), 8);
        assert_eq!(store.chain_tail_hash(), "t-7");
    }

    #[test]
    fn test_line_too_large_fails_visible() {
        let m = RamFlashMedium::new(64, 4);
        let mut store = FlashWalStore::open(m).unwrap();
        let fact = command_fact(1, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        let err = store
            .append_record(0, &fact, &content, "genesis", "chain-1")
            .unwrap_err();
        assert!(
            matches!(err, McuError::WalRecordTooLarge { .. }),
            "got: {err}"
        );
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_reopen_restores_state() {
        let mut store = FlashWalStore::open(medium()).unwrap();
        for v in 0..3u64 {
            append_tagged(&mut store, v, "t");
        }
        let lines_before: Vec<String> = (0..3).map(|i| store.line_json(i).unwrap()).collect();
        let tail_before = String::from(store.chain_tail_hash());

        // Reopen after drop (process restart simulation)
        let m = store.medium().clone();
        drop(store);
        let mut reopened = FlashWalStore::open(m).unwrap();
        assert_eq!(reopened.len(), 3);
        assert_eq!(reopened.chain_tail_hash(), tail_before);
        for (i, line) in lines_before.iter().enumerate() {
            assert_eq!(&reopened.line_json(i).unwrap(), line);
        }

        // Continued appends continue the chain unchanged
        append_tagged(&mut reopened, 3, "t");
        assert_eq!(reopened.len(), 4);
        assert_eq!(reopened.chain_tail_hash(), "t-3");
    }

    #[test]
    fn test_power_cut_truncated_line_is_not_a_record() {
        // Power-cut injection: the last record only gets its prefix bytes
        // written (no \n) → not recognized at recovery, active page polluted
        let mut store = FlashWalStore::open(medium()).unwrap();
        for v in 0..2u64 {
            append_tagged(&mut store, v, "t");
        }
        let full = full_line_bytes(2, store.chain_tail_hash(), "t-2");
        let offset = store.medium().page_size() * 2; // the 3rd line should land on page 2
        store
            .medium_mut_for_test()
            .program(offset, &full.as_bytes()[..40])
            .unwrap();
        // Snapshot the medium state after injection (process restart
        // simulation: the medium persists, the store is dropped)
        let m = store.medium().clone();
        drop(store);

        let reopened = FlashWalStore::open(m).unwrap();
        assert_eq!(reopened.len(), 2, "a half line does not constitute a record");
        assert_eq!(reopened.chain_tail_hash(), "t-1");
        assert!(reopened.is_polluted(), "the active page should be marked polluted");
    }

    #[test]
    fn test_power_cut_then_continue_appends_to_next_page() {
        // Power-cut half line → reopen → continued appends automatically
        // advance to the next empty page; the chain stays complete
        let mut store = FlashWalStore::open(medium()).unwrap();
        for v in 0..2u64 {
            append_tagged(&mut store, v, "t");
        }
        let full = full_line_bytes(2, store.chain_tail_hash(), "t-2");
        let offset = store.medium().page_size() * 2;
        store
            .medium_mut_for_test()
            .program(offset, &full.as_bytes()[..40])
            .unwrap();
        let m = store.medium().clone();
        drop(store);

        let mut reopened = FlashWalStore::open(m).unwrap();
        append_tagged(&mut reopened, 2, "t");
        assert_eq!(reopened.len(), 3);
        assert_eq!(reopened.chain_tail_hash(), "t-2");
        // The new line lands on the empty page (page 3) after the polluted page (page 2)
        assert_eq!(
            reopened.line_offsets()[2] / reopened.medium().page_size(),
            3
        );
    }

    #[test]
    fn test_random_power_cut_injection_matrix() {
        // Random power-cut truncation matrix: LCG-driven truncation points
        // (zero residue / half line / complete line), verifying per point the
        // recovered line count / chain tail / pollution marker / append
        // capability
        let mut rng = Lcg(0x5EED_1234_ABCD_0001);
        for case in 0..32u32 {
            let mut store = FlashWalStore::open(medium()).unwrap();
            for v in 0..2u64 {
                append_tagged(&mut store, v, "t");
            }
            let full = full_line_bytes(2, store.chain_tail_hash(), "t-2");
            let k = (rng.next() as usize) % (full.len() + 1);
            let offset = store.medium().page_size() * 2;
            if k > 0 {
                store
                    .medium_mut_for_test()
                    .program(offset, &full.as_bytes()[..k])
                    .unwrap();
            }
            let m = store.medium().clone();
            drop(store);

            let mut reopened = FlashWalStore::open(m).unwrap();
            let expected_len = if k == full.len() { 3 } else { 2 };
            assert_eq!(
                reopened.len(),
                expected_len,
                "case {case}: k={k}/{} recovered line count mismatch after truncation",
                full.len()
            );
            let expected_tail = if k == full.len() { "t-2" } else { "t-1" };
            assert_eq!(
                reopened.chain_tail_hash(),
                expected_tail,
                "case {case}: k={k} chain tail mismatch"
            );
            assert_eq!(
                reopened.is_polluted(),
                k > 0 && k < full.len(),
                "case {case}: k={k} pollution marker mismatch"
            );
            // After recovery, appends must continue (this case always has a
            // spare page in the ring)
            let fact = command_fact(9, JsonValue::empty_object());
            let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
            let prev = String::from(reopened.chain_tail_hash());
            reopened
                .append_record(2, &fact, &content, &prev, "chain-resume")
                .unwrap();
        }
    }

    #[test]
    fn test_corrupted_page_fails_visible() {
        // A non-0xFF byte appears after the 0xFF region: the sequential-write
        // invariant is broken → fails visibly
        let mut m = medium();
        m.program(10, &[0xAA]).unwrap(); // written after the 0xFF region within page 0
        m.program(100, &[0xBB]).unwrap();
        let err = FlashWalStore::open(m).unwrap_err();
        assert!(matches!(err, McuError::WalCorrupted(_)), "got: {err}");
    }

    #[test]
    fn test_ring_hole_fails_visible() {
        // A data page after an empty page: ring hole → fails visibly
        let mut m = medium();
        m.program(0, &[0x11]).unwrap(); // page 0 has data
        m.program(256 * 2, &[0x22]).unwrap(); // page 1 empty, page 2 has data
        let err = FlashWalStore::open(m).unwrap_err();
        assert!(matches!(err, McuError::WalCorrupted(_)), "got: {err}");
    }
}
