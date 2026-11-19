// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Audit chain sentinel: periodically scan a WAL directory, verify every chain
//! end-to-end, and compare across scans against a baseline
//!
//! # Two-layer comparison model
//! - **Ruler self-check** (golden vectors): before scanning, assert the built-in
//!   7-fact hash snapshot and the anchor golden vector — proving the verifier
//!   itself is faithful (metrology-calibration thinking). Self-check failure =
//!   broken tool (exit code 2); no data verdict is produced.
//! - **Data comparison** (baseline prefix anchoring): the baseline records, per
//!   file, `chain tail + record count`. The append verdict = replay this round's
//!   chain up to the previous record count and assert the tail equals the previous
//!   baseline value — precisely distinguishing "pure append" from "history
//!   rewritten and the whole chain recomputed" (the latter always fails prefix
//!   anchoring). Same-position tail change / regression / file disappearance map
//!   to exit codes 3/3/4 respectively.
//!
//! # Exit code protocol (the scheduler only looks at exit codes)
//! `0` all green / normal append / first run creates the baseline; `2` tool
//! self-check failed (broken ruler); `3` chain broken (verification mismatch /
//! prefix anchor mismatch / same-position tail change / record regression); `4`
//! in baseline but missing on disk (file gone); `5` partial line / corruption
//! rejected (pairs with the `truncate-partial-line` controlled recovery).
//!
//! # Unhashed ledger policy
//! `shared_facts.wal` (the shared fact ledger, currently without a hash chain)
//! gets structural-integrity recording (line count / bytes / content BLAKE3
//! digest into the baseline); change detection works as usual, chain
//! verification is skipped and honestly labeled `unhashed-ledger`.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use clap::Args;
use evorule_reactor::{chain_step, fact_hash, read_wal_with_hash};

use crate::error::CliError;

/// Kernel SSOT snapshot values (evorule-audit-chain/hash_snapshot.txt; intentional changes must be synced in the same batch)
const KERNEL_HASH_SNAPSHOT: [&str; 7] = [
    "56de9ce95d41896edf081f452b4ea5d3ad6508de3a0781a36fa5fbd13c586aa1",
    "902f22869450a78851d5bf1396b64d4bc7be0666468a39d2dac53b7d176f8047",
    "6c4a12ee89a692181a07ede80b0864f0774286427fce67156944f6f33290ddf7",
    "4855b360063f11eeffd7b8e2bd5720c641451012a688ebec489b5f8f059058aa",
    "867c9f708b7469ffb25682f997792b8811f34ef61eb2ee4ba23964e1c20f7909",
    "ac02d9e15a497b30c18ce5019a50aeced2d5d186a973ee3a0985ee728054a015",
    "42ca4e53611616707514550f1d2ab8a89be5ef8bd1a5f6cbd07e016bdb931afb",
];

const EXIT_OK: u8 = 0;
const EXIT_SELF_TEST: u8 = 2;
const EXIT_CHAIN_BROKEN: u8 = 3;
const EXIT_FILE_GONE: u8 = 4;
const EXIT_CORRUPT_REJECT: u8 = 5;

#[derive(Debug, Args)]
pub struct ScanWalDir {
    /// WAL directory (session_*.wal* / shared_facts.wal / governance_single_reactor.wal)
    pub wal_dir: PathBuf,

    /// Baseline file (cross-scan comparison; created on the first run, updated every run after)
    #[arg(long)]
    pub baseline: Option<PathBuf>,

    /// Sentinel log (append-only, one line per file per scan; a dedicated directory is recommended)
    #[arg(long)]
    pub sentinel_log: Option<PathBuf>,

    /// Machine-readable JSON report output file
    #[arg(long)]
    pub json: Option<PathBuf>,
}

/// This round's observation for a single file
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FileObs {
    records: u64,
    /// Chain tail (for unhashed ledgers, the BLAKE3 digest of the content)
    tail: String,
    bytes: u64,
    verdict: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Baseline {
    files: BTreeMap<String, FileObs>,
}

/// Golden vector self-check: 7-value hash snapshot + genesis semantics + anchor golden vector
fn self_test() -> Result<(), CliError> {
    use evorule_reactor::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

    // ① 7-fact hash snapshot (byte-for-byte with the kernel SSOT)
    let facts = [
        Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        },
        Fact::PayloadUpdate {
            id: FactId(2),
            path: "test.path".into(),
            value: JsonValue::string("test_value"),
        },
        Fact::StateTransition {
            id: FactId(3),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        },
        Fact::IoRequest {
            id: FactId(4),
            cause: FactId(3),
            io_type: IoType::http_get(),
            params: JsonValue::empty_object(),
        },
        Fact::IoResponse {
            id: FactId(5),
            request_id: FactId(4),
            result: JsonValue::string("response"),
            error: None,
        },
        Fact::Stable {
            id: FactId(6),
            version: 0,
        },
        Fact::Error {
            id: FactId(7),
            message: "test error".into(),
        },
    ];
    for (i, f) in facts.iter().enumerate() {
        let h = fact_hash(f)
            .map_err(|e| crate::error::CliError::other(format!("self-test hash: {e}")))?;
        if h != KERNEL_HASH_SNAPSHOT[i] {
            return Err(CliError::other(format!(
                "self-test FAILED: golden fact hash #{}/7 mismatch (verifier drifted from kernel SSOT)",
                i + 1
            )));
        }
    }

    // ② genesis semantics (chain tail of an empty chain)
    // compute_chain_hash(&[]) == "genesis" is guarded by kernel tests; here we verify the chain_step starting point
    let first = chain_step("genesis", KERNEL_HASH_SNAPSHOT[0]);
    if first.len() != 64 || !first.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CliError::other("self-test FAILED: chain_step output shape"));
    }

    // ③ Anchor golden vector (fixed-seed golden anchor): the v2 payload is independently
    // reconstructed from the anchor fields + locally verified with ed25519 — checking the
    // cryptographic elements of the self-check and the payload-format faithfulness (anchor
    // chain construction correctness is guarded by the upstream governance golden vector
    // tests, not duplicated here)
    use crate::signing::{hex_decode, verify_signature, AuditSigner};
    const GOLDEN_PK: &str = "248acbdbaf9e050196de704bea2d68770e519150d103b587dae2d9cad53dd930";
    let signer = AuditSigner::from_bytes([0xabu8; 32]);
    let pk = signer.verifying_bytes();
    if crate::signing::hex_encode(&pk) != GOLDEN_PK {
        return Err(CliError::other(
            "self-test FAILED: anchor golden pubkey mismatch",
        ));
    }
    // Golden anchor 0: seq=0, range [0,5), chain_head=a×64 (same origin as the governance golden vector)
    let payload0 = format!(
        "evorule-anchor/2|vector-sess-1|0|0|5|{}|anchor-key-1|vector-engine/0.1|0",
        "a".repeat(64)
    );
    let sig0_hex = "bb9fe4f555b816edef9e0f1438e07a3dfa944688324b489304ba0923a371024217683c56f0d0d30a6bbf1efda8dad0ba0bca829a6ca4e6598b2cf4df167e2507";
    let sig0: [u8; 64] = hex_decode(sig0_hex)
        .map_err(|e| CliError::other(format!("self-test hex: {e}")))?
        .try_into()
        .map_err(|_| CliError::other("self-test sig length"))?;
    let ok = verify_signature(pk, payload0.as_bytes(), &sig0)
        .map_err(|e| CliError::other(format!("self-test verify: {e}")))?;
    if !ok {
        return Err(CliError::other(
            "self-test FAILED: anchor golden signature verify",
        ));
    }
    Ok(())
}

/// Parse a session WAL file name: session_{n}.wal or rotation shard session_{n}.wal.{seq}
fn session_wal_key(name: &str) -> Option<String> {
    let rest = name.strip_prefix("session_")?;
    let pos = rest.find(".wal")?;
    let id_part = &rest[..pos];
    let after = &rest[pos + 4..];
    let valid_suffix = after.is_empty()
        || (after.len() > 1
            && after.starts_with('.')
            && after[1..].bytes().all(|b| b.is_ascii_digit()));
    if !valid_suffix {
        return None;
    }
    id_part
        .parse::<u64>()
        .ok()
        .map(|id| format!("session:{id}"))
}

/// Merge shards of the same session (the baseline is keyed by session: the shard set is observed as a whole)
fn classify(dir: &Path) -> std::result::Result<BTreeMap<String, Vec<PathBuf>>, String> {
    let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("read wal dir: {e}"))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(key) = session_wal_key(&name) {
            groups.entry(key).or_default().push(entry.path());
        } else if name == "shared_facts.wal" {
            groups
                .entry("shared_facts".into())
                .or_default()
                .push(entry.path());
        } else if name == "governance_single_reactor.wal" {
            groups
                .entry("governance_single_reactor".into())
                .or_default()
                .push(entry.path());
        }
    }
    for v in groups.values_mut() {
        v.sort();
    }
    Ok(groups)
}

/// Full-chain triple check + chain hash trace (used for prefix anchoring)
fn verify_group(paths: &[PathBuf]) -> std::result::Result<(Vec<String>, u64, u64), String> {
    // paths is sorted: main file → .1 → .2 (consistent with write-time order)
    let mut prev = "genesis".to_string();
    let mut chain_hashes: Vec<String> = Vec::new();
    let mut bytes = 0u64;
    for p in paths {
        let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        bytes += meta.len();
        let records = read_wal_with_hash(p).map_err(|e| format!("{}: {e}", p.display()))?;
        for rec in &records {
            let content = fact_hash(&rec.fact).map_err(|e| format!("{}: {e}", p.display()))?;
            if let Some(stored) = &rec.content_hash {
                if stored != &content {
                    return Err(format!(
                        "{}: content_hash mismatch at fact {}",
                        p.display(),
                        rec.fact.id().0
                    ));
                }
            }
            if let Some(stored_prev) = &rec.prev_hash {
                if stored_prev != &prev {
                    return Err(format!(
                        "{}: chain broken at fact {} (stored_prev != expected)",
                        p.display(),
                        rec.fact.id().0
                    ));
                }
            }
            let chain = chain_step(&prev, &content);
            if let Some(stored_chain) = &rec.chain_hash {
                if stored_chain != &chain {
                    return Err(format!(
                        "{}: chain_hash mismatch at fact {}",
                        p.display(),
                        rec.fact.id().0
                    ));
                }
            }
            prev = chain;
            chain_hashes.push(prev.clone());
        }
    }
    let count = chain_hashes.len() as u64;
    Ok((chain_hashes, count, bytes))
}

/// Unhashed ledger: structural integrity digest (no active caller currently; kept for future unchained ledgers)
#[allow(dead_code)]
fn digest_group(paths: &[PathBuf]) -> std::result::Result<(String, u64, u64), String> {
    let mut bytes = 0u64;
    let mut lines = 0u64;
    let mut hasher = blake3::Hasher::new();
    for p in paths {
        let data = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        bytes += data.len() as u64;
        lines += data.iter().filter(|&&b| b == b'\n').count() as u64;
        hasher.update(&data);
    }
    Ok((hasher.finalize().to_hex().to_string(), lines, bytes))
}

fn bump(code: u8, worst: &mut u8) {
    // Severity order: 3 > 4 > 5 > 2 > 0 (the self-check short-circuits before run; code 2 never appears here)
    let rank = |c: u8| match c {
        EXIT_CHAIN_BROKEN => 4,
        EXIT_FILE_GONE => 3,
        EXIT_CORRUPT_REJECT => 2,
        _ => 0,
    };
    if rank(code) > rank(*worst) {
        *worst = code;
    }
}

/// Baseline roll-forward + JSON report + human-readable summary and remediation guidance
fn finalize_and_report(
    cmd: &ScanWalDir,
    now: &Baseline,
    worst: &mut u8,
    report_lines: &[String],
    first_run: bool,
    baseline_path: &Path,
) {
    // Write the new baseline (roll forward only when nothing is broken; on breakage keep the old
    // baseline for forensic comparison; a write failure degrades to a warning — output-side
    // failures must not change the exit-code semantics of the scan verdict)
    if *worst == EXIT_OK {
        match serde_json::to_string_pretty(&now) {
            Ok(out) => {
                if let Err(e) = std::fs::write(baseline_path, out) {
                    eprintln!("[SENTINEL] baseline write failed: {e}");
                }
            }
            Err(e) => eprintln!("[SENTINEL] baseline serialize failed: {e}"),
        }
    }

    if let Some(json_path) = &cmd.json {
        let report = serde_json::json!({
            "wal_dir": cmd.wal_dir.display().to_string(),
            "first_run": first_run,
            "exit_code": worst,
            "files": now.files,
            "lines": report_lines,
        });
        match serde_json::to_string_pretty(&report)
            .map_err(|e| e.to_string())
            .and_then(|j| std::fs::write(json_path, j).map_err(|e| e.to_string()))
        {
            Ok(()) => {}
            Err(e) => eprintln!("[SENTINEL] json report write failed: {e}"),
        }
    }

    println!(
        "[SENTINEL] scan {} files/groups, exit={worst}",
        now.files.len()
    );
    for l in report_lines {
        println!("  {l}");
    }
    if *worst == EXIT_CORRUPT_REJECT {
        println!("  Remediation: a partial/corrupt line can be recovered in a controlled way with `evorule truncate-partial-line <file> --confirm` (prefix chain verification + backup)");
    }
}

/// Sentinel log (append-only): `ts | key | records | tail | verdict | delta`
fn append_sentinel_log(
    path: &Option<PathBuf>,
    key: &str,
    obs: &FileObs,
    verdict_label: &str,
    comparison: &str,
) {
    let Some(sl) = path else { return };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Some(parent) = sl.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(sl)
    {
        let _ = writeln!(
            f,
            "{ts} | {key} | {} | {} | {} | {comparison}",
            obs.records, obs.tail, verdict_label
        );
    }
}

/// Baseline comparison (prefix anchoring): obs = this round's observation, prev = the previous baseline entry (None = new file)
fn compare_with_baseline(
    obs: &FileObs,
    prev: Option<&FileObs>,
    unhashed: bool,
    paths: &[PathBuf],
    worst: &mut u8,
) -> String {
    match prev {
        None => "new-file".to_string(),
        Some(prev_obs) => {
            if unhashed {
                if prev_obs.tail == obs.tail {
                    "unchanged".to_string()
                } else {
                    // Unhashed ledger: report content changes faithfully (append vs. tampering
                    // indistinguishable — the ledger has no chain to anchor)
                    bump(EXIT_CHAIN_BROKEN, worst);
                    "UNHASHED-CHANGED (content changed: append vs. tampering indistinguishable, manual confirmation required)".to_string()
                }
            } else if obs.records < prev_obs.records {
                bump(EXIT_CHAIN_BROKEN, worst);
                format!(
                    "REGRESSED (record count regressed: previous {} → current {})",
                    prev_obs.records, obs.records
                )
            } else if obs.records == prev_obs.records {
                if obs.tail == prev_obs.tail {
                    "unchanged".to_string()
                } else {
                    bump(EXIT_CHAIN_BROKEN, worst);
                    "TAIL-CHANGED-SAME-LENGTH (same-position tail change = tampering)".to_string()
                }
            } else {
                // Record count grew: prefix anchoring — replaying up to the previous record
                // count, the chain tail must equal the previous tail
                match verify_group(paths) {
                    Ok((chain, _, _)) => {
                        let anchor_idx = (prev_obs.records as usize).saturating_sub(1);
                        match chain.get(anchor_idx) {
                            Some(h) if h == &prev_obs.tail => {
                                "appended (prefix anchor verified)".to_string()
                            }
                            _ => {
                                bump(EXIT_CHAIN_BROKEN, worst);
                                "PREFIX-ANCHOR-MISMATCH (history rewritten and recomputed, or shard misalignment)".to_string()
                            }
                        }
                    }
                    Err(e) => {
                        bump(EXIT_CHAIN_BROKEN, worst);
                        format!("RECHECK-FAILED: {e}")
                    }
                }
            }
        }
    }
}

pub fn run(cmd: &ScanWalDir) -> Result<(), CliError> {
    // ===== Layer A: ruler self-check (failure = code 2, no data verdict produced) =====
    if let Err(e) = self_test() {
        eprintln!("[SENTINEL] SELF-TEST FAILED (verifier untrusted, no data verdict produced this round): {e}");
        return Err(CliError::SentinelExit(EXIT_SELF_TEST));
    }

    let groups = classify(&cmd.wal_dir).map_err(CliError::other)?;
    let baseline_path = cmd
        .baseline
        .clone()
        .unwrap_or_else(|| cmd.wal_dir.join("sentinel-baseline.json"));
    let old: Baseline = match std::fs::read_to_string(&baseline_path) {
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| crate::error::CliError::other(format!("baseline parse: {e}")))?,
        Err(_) => Baseline::default(), // first run
    };
    let first_run = old.files.is_empty();

    let mut now = Baseline::default();
    let mut worst: u8 = EXIT_OK;
    let mut report_lines: Vec<String> = Vec::new();

    for (key, paths) in &groups {
        // Since kernel WAL v2, shared_facts.wal is chained via FactsLog::append (observed
        // to carry the hash triple) — it takes the chain triple-check + prefix anchoring
        // path; the unhashed-ledger policy is reserved only for ledgers genuinely without
        // a hash chain (none currently)
        let unhashed = false;
        let obs_result = if unhashed {
            digest_group(paths).map(|(digest, lines, bytes)| FileObs {
                records: lines,
                tail: digest,
                bytes,
                verdict: "unhashed-ledger".into(),
            })
        } else {
            verify_group(paths).map(|(chain, records, bytes)| FileObs {
                records,
                tail: chain.last().cloned().unwrap_or_else(|| "genesis".into()),
                bytes,
                verdict: "verified".into(),
            })
        };

        match obs_result {
            Ok(obs) => {
                let verdict_label = obs.verdict.clone();
                // ===== Layer B: baseline comparison (prefix anchoring) =====
                let comparison =
                    compare_with_baseline(&obs, old.files.get(key), unhashed, paths, &mut worst);
                report_lines.push(format!(
                    "{key}: {} records, tail {}..{}, verdict={verdict_label}, delta={comparison}",
                    obs.records,
                    &obs.tail[..8.min(obs.tail.len())],
                    &obs.tail[obs.tail.len().saturating_sub(8)..]
                ));
                append_sentinel_log(&cmd.sentinel_log, key, &obs, &verdict_label, &comparison);
                now.files.insert(key.clone(), obs);
            }
            Err(e) => {
                // Parse/verify failure: distinguish "mismatch" (code 3) from
                // "partial line / corruption rejected" (code 5)
                let partial = e.contains("JSON parse error") || e.contains("EOF while parsing");
                let code = if partial {
                    EXIT_CORRUPT_REJECT
                } else {
                    EXIT_CHAIN_BROKEN
                };
                bump(code, &mut worst);
                report_lines.push(format!("{key}: REJECTED ({e})"));
                // The rejected file still enters this round's observation (verdict=rejected placeholder):
                // the file physically exists and must not be misreported by the "file gone"
                // branch (code 4 and code 5 semantics are isolated)
                let bytes_now = paths
                    .iter()
                    .filter_map(|p| std::fs::metadata(p).ok())
                    .map(|m| m.len())
                    .sum();
                now.files.insert(
                    key.clone(),
                    FileObs {
                        records: 0,
                        tail: String::new(),
                        bytes: bytes_now,
                        verdict: "rejected".into(),
                    },
                );
            }
        }
    }

    // Sessions present in the baseline but gone from disk → code 4
    for key in old.files.keys() {
        if !now.files.contains_key(key) && !first_run {
            bump(EXIT_FILE_GONE, &mut worst);
            report_lines.push(format!(
                "{key}: FILE-GONE (in baseline but missing on disk — truncated or deleted)"
            ));
        }
    }

    finalize_and_report(
        cmd,
        &now,
        &mut worst,
        &report_lines,
        first_run,
        &baseline_path,
    );
    if worst == EXIT_OK {
        Ok(())
    } else {
        Err(CliError::SentinelExit(worst))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// The self-check must pass (golden vector = the metrology calibration of the ruler itself)
    #[test]
    fn self_test_passes_with_current_binaries() {
        assert!(
            self_test().is_ok(),
            "sentinel self-check failed: verifier drifted from the golden vectors"
        );
    }

    /// The built-in snapshot constants must match the kernel SSOT snapshot file line by line
    /// (compile-time + test-time double lock across repos)
    #[test]
    fn builtin_snapshot_matches_kernel_file() {
        let kernel_snapshot = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../evorule-audit-chain/hash_snapshot.txt");
        let content = std::fs::read_to_string(&kernel_snapshot)
            .expect("kernel SSOT snapshot file should exist (same workspace)");
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 7, "kernel snapshot should have 7 lines");
        for (i, line) in lines.iter().enumerate() {
            assert_eq!(
                *line,
                KERNEL_HASH_SNAPSHOT[i],
                "line {} does not match the built-in constant",
                i + 1
            );
        }
    }

    /// Session file-name merging (including rotation shards and exclusion of non-session files)
    #[test]
    fn session_wal_key_classification() {
        assert_eq!(
            session_wal_key("session_3.wal").as_deref(),
            Some("session:3")
        );
        assert_eq!(
            session_wal_key("session_12.wal.7").as_deref(),
            Some("session:12")
        );
        assert_eq!(session_wal_key("shared_facts.wal"), None);
        assert_eq!(session_wal_key("session_x.wal"), None);
        assert_eq!(session_wal_key("notes.txt"), None);
    }
}
