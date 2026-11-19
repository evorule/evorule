// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Verdict surface: export four-step / WAL three-step + chain folding
//!
//! Report-style verdicts use **first-red-stop** semantics (consistent with the
//! sentinel / independent verifier short-circuit semantics): once a link
//! breaks, the folded chain head loses its anchor and subsequent entries are
//! marked undetermined (`stopped_at`). All green = no stop point AND the chain
//! tail reconciles with the declared tail hash.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::record::{ExportDoc, WalLine, GENESIS};
use evorule_audit_chain::hash::{chain_step, fact_hash, fact_to_stable_json};

/// Short display of a hash string (first 16 characters)
fn short(h: &str) -> String {
    if h.len() <= 16 {
        String::from(h)
    } else {
        format!("{}…", &h[..16])
    }
}

/// Export entry verdict step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportStep {
    /// ① prev link
    Link,
    /// ② content hash (fact_hash formula recomputation)
    Content,
    /// ③ stable deep-compare (recomputed stable reconciled with the original + entry projection consistency)
    Stable,
    /// Tail reconciliation (last_hash)
    Tail,
}

/// Export entry rejection details
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryReject {
    pub step: ExportStep,
    pub reason: String,
}

/// Export entry verdict
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryVerdict {
    pub index: usize,
    pub ok: bool,
    pub reject: Option<EntryReject>,
}

/// Export-surface report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// Per-entry verdicts (up to the first red: green entries + the first red entry)
    pub verdicts: Vec<EntryVerdict>,
    /// Index of the first red entry (None when all green)
    pub stopped_at: Option<usize>,
    /// Folded chain tail (up to the stop point)
    pub tail_hash: String,
    /// Whether the chain tail reconciles with `last_hash`
    pub last_hash_matches: bool,
}

impl ExportReport {
    /// All-green verdict
    pub fn all_ok(&self) -> bool {
        self.stopped_at.is_none() && self.last_hash_matches
    }
}

/// Export-surface verdict (four steps per entry + tail reconciliation)
pub fn verify_export(doc: &ExportDoc) -> ExportReport {
    let mut prev = String::from(GENESIS);
    let mut verdicts = Vec::with_capacity(doc.entries.len());
    for (index, e) in doc.entries.iter().enumerate() {
        // ① Link integrity: the declared prev must equal the folded chain head
        if e.prev_hash != prev {
            verdicts.push(EntryVerdict {
                index,
                ok: false,
                reject: Some(EntryReject {
                    step: ExportStep::Link,
                    reason: format!(
                        "prev_hash={} 与折叠链头={} 不符（链接断裂）",
                        short(&e.prev_hash),
                        short(&prev)
                    ),
                }),
            });
            return finish(verdicts, Some(index), prev, doc);
        }
        // ② Content hash: fact_hash formula recomputation (implies byte-level reconciliation of the fact_json_stable original)
        let recomputed = match fact_hash(&e.fact) {
            Ok(h) => h,
            Err(err) => {
                verdicts.push(EntryVerdict {
                    index,
                    ok: false,
                    reject: Some(EntryReject {
                        step: ExportStep::Content,
                        reason: format!("fact_hash recomputation failed: {err}"),
                    }),
                });
                return finish(verdicts, Some(index), prev, doc);
            }
        };
        if recomputed != e.content_hash {
            verdicts.push(EntryVerdict {
                index,
                ok: false,
                reject: Some(EntryReject {
                    step: ExportStep::Content,
                    reason: format!(
                        "content_hash 失配: 声明={} 重算={}",
                        short(&e.content_hash),
                        short(&recomputed)
                    ),
                }),
            });
            return finish(verdicts, Some(index), prev, doc);
        }
        // ③ Stable deep-compare: Value-level reconciliation of the recomputed
        //    stable against the fact_json_stable original, plus entry projection
        //    (fact_id / fact_type) consistency
        let stable_ok = (|| {
            let stable = fact_to_stable_json(&e.fact).ok()?;
            let original: serde_json::Value = serde_json::from_str(&e.fact_json_stable).ok()?;
            (stable == original).then_some(())
        })();
        if stable_ok.is_none() {
            verdicts.push(EntryVerdict {
                index,
                ok: false,
                reject: Some(EntryReject {
                    step: ExportStep::Stable,
                    reason: String::from(
                        "fact_json_stable original does not match the recomputation of the fact field (or the original is invalid JSON)",
                    ),
                }),
            });
            return finish(verdicts, Some(index), prev, doc);
        }
        if e.fact.id().0 != e.fact_id || e.fact.type_name() != e.fact_type {
            verdicts.push(EntryVerdict {
                index,
                ok: false,
                reject: Some(EntryReject {
                    step: ExportStep::Stable,
                    reason: format!(
                        "entry projection mismatch: fact_id({})/fact_type({}) inconsistent with the fact field",
                        e.fact_id, e.fact_type
                    ),
                }),
            });
            return finish(verdicts, Some(index), prev, doc);
        }
        // ④ Chain folding
        prev = chain_step(&prev, &e.content_hash);
        verdicts.push(EntryVerdict {
            index,
            ok: true,
            reject: None,
        });
    }
    finish(verdicts, None, prev, doc)
}

fn finish(
    verdicts: Vec<EntryVerdict>,
    stopped_at: Option<usize>,
    tail: String,
    doc: &ExportDoc,
) -> ExportReport {
    let last_hash_matches = tail == doc.last_hash;
    ExportReport {
        verdicts,
        stopped_at,
        tail_hash: tail,
        last_hash_matches,
    }
}

/// WAL line rejection details
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalReject {
    /// Step number (1=link 2=content hash 3=chain folding)
    pub step: u8,
    pub reason: String,
}

/// WAL line verdict
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalLineVerdict {
    pub index: usize,
    pub ok: bool,
    pub reject: Option<WalReject>,
}

/// WAL-surface report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalReport {
    pub verdicts: Vec<WalLineVerdict>,
    pub stopped_at: Option<usize>,
    pub tail_hash: String,
}

impl WalReport {
    /// All-green verdict
    pub fn all_ok(&self) -> bool {
        self.stopped_at.is_none()
    }
}

/// WAL-surface verdict (three steps per line, first-red-stop)
pub fn verify_wal_lines(lines: &[WalLine]) -> WalReport {
    let mut prev = String::from(GENESIS);
    let mut verdicts = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        // ① Link integrity
        if line.prev_hash != prev {
            verdicts.push(WalLineVerdict {
                index,
                ok: false,
                reject: Some(WalReject {
                    step: 1,
                    reason: format!(
                        "prev_hash={} does not match the chain tail={} (broken link)",
                        short(&line.prev_hash),
                        short(&prev)
                    ),
                }),
            });
            return WalReport {
                verdicts,
                stopped_at: Some(index),
                tail_hash: prev,
            };
        }
        // ② Content hash: fact_hash formula recomputation
        match fact_hash(&line.fact) {
            Ok(h) if h == line.content_hash => {}
            Ok(h) => {
                verdicts.push(WalLineVerdict {
                    index,
                    ok: false,
                    reject: Some(WalReject {
                        step: 2,
                        reason: format!(
                            "content_hash mismatch: declared={} recomputed={}",
                            short(&line.content_hash),
                            short(&h)
                        ),
                    }),
                });
                return WalReport {
                    verdicts,
                    stopped_at: Some(index),
                    tail_hash: prev,
                };
            }
            Err(err) => {
                verdicts.push(WalLineVerdict {
                    index,
                    ok: false,
                    reject: Some(WalReject {
                        step: 2,
                        reason: format!("fact_hash recomputation failed: {err}"),
                    }),
                });
                return WalReport {
                    verdicts,
                    stopped_at: Some(index),
                    tail_hash: prev,
                };
            }
        }
        // ③ Chain folding integrity
        let folded = chain_step(&line.prev_hash, &line.content_hash);
        if folded != line.chain_hash {
            verdicts.push(WalLineVerdict {
                index,
                ok: false,
                reject: Some(WalReject {
                    step: 3,
                    reason: format!(
                        "chain_hash mismatch: declared={} recomputed={}",
                        short(&line.chain_hash),
                        short(&folded)
                    ),
                }),
            });
            return WalReport {
                verdicts,
                stopped_at: Some(index),
                tail_hash: prev,
            };
        }
        prev = line.chain_hash.clone();
        verdicts.push(WalLineVerdict {
            index,
            ok: true,
            reject: None,
        });
    }
    WalReport {
        verdicts,
        stopped_at: None,
        tail_hash: prev,
    }
}
