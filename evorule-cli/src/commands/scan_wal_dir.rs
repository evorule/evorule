// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 审计链哨兵：定时扫描 WAL 目录，逐文件全链验证 + 跨扫描基线比对
//!
//! # 双层比对模型
//! - **尺子自检**（黄金向量）：扫描前断言内置 7 事实哈希快照与锚点黄金向量——
//!   证明验证器自身忠实（计量校准思想）。自检失败=工具红（退出码 2），
//!   不产出数据结论。
//! - **数据比对**（基线前缀锚定）：基线记录每文件 `链尾+记录数`。追加判定=
//!   重放本轮链至上轮记录数位置，断言链尾==上轮基线值——精确区分「纯追加」
//!   与「历史被改后整链重算」（后者前缀锚定必失配）。旧链尾同位变化/回退/
//!   文件消失分别对应退出码 3/3/4。
//!
//! # 退出码协议（调度器只认退出码）
//! `0` 全绿/正常追加/首跑建基线；`2` 工具自检失败（尺子坏）；`3` 链破坏
//! （验证失配/前缀锚定失配/同位链尾变化/记录回退）；`4` 基线有而磁盘无
//! （文件消失）；`5` 半行/损坏拒收（衔接 `truncate-partial-line` 受控恢复）。
//!
//! # 无哈希账本口径
//! `shared_facts.wal`（共享事实账本，暂无哈希链）做结构完整性记录
//! （行数/字节/内容 BLAKE3 摘要入基线），变化检测照常、链验证跳过并如实
//! 标注 `unhashed-ledger`。

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use clap::Args;
use evorule_reactor::{chain_step, fact_hash, read_wal_with_hash};

use crate::error::CliError;

/// 内核 SSOT 快照值（evorule-audit-chain/hash_snapshot.txt；有意变更须同批同步）
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
    /// WAL 目录（session_*.wal* / shared_facts.wal / governance_single_reactor.wal）
    pub wal_dir: PathBuf,

    /// 基线文件（跨扫描比对；首跑创建，其后每次更新）
    #[arg(long)]
    pub baseline: Option<PathBuf>,

    /// 哨兵日志（append-only，每次扫描每文件一行；建议独立目录）
    #[arg(long)]
    pub sentinel_log: Option<PathBuf>,

    /// 机器可读 JSON 报告输出文件
    #[arg(long)]
    pub json: Option<PathBuf>,
}

/// 单文件的本轮观测
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FileObs {
    records: u64,
    /// 链尾（unhashed 账本为内容 BLAKE3 摘要）
    tail: String,
    bytes: u64,
    verdict: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Baseline {
    files: BTreeMap<String, FileObs>,
}

/// 黄金向量自检：哈希快照 7 值 + genesis 语义 + 锚点黄金向量
fn self_test() -> Result<(), CliError> {
    use evorule_reactor::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

    // ① 7 事实哈希快照（与内核 SSOT 逐字节）
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

    // ② genesis 语义（空链链尾）
    // compute_chain_hash(&[]) == "genesis" 由内核测试守护；此处验证 chain_step 起点
    let first = chain_step("genesis", KERNEL_HASH_SNAPSHOT[0]);
    if first.len() != 64 || !first.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CliError::other("self-test FAILED: chain_step output shape"));
    }

    // ③ 锚点黄金向量（固定 seed 黄金锚点）：v2 载荷从锚点字段独立重构 + 本地
    // ed25519 验签——自检密码学元件与载荷格式忠实性（锚点链构造正确性由上游
    // governance 黄金向量测试守护，此处不重复）
    use crate::signing::{hex_decode, verify_signature, AuditSigner};
    const GOLDEN_PK: &str = "248acbdbaf9e050196de704bea2d68770e519150d103b587dae2d9cad53dd930";
    let signer = AuditSigner::from_bytes([0xabu8; 32]);
    let pk = signer.verifying_bytes();
    if crate::signing::hex_encode(&pk) != GOLDEN_PK {
        return Err(CliError::other(
            "self-test FAILED: anchor golden pubkey mismatch",
        ));
    }
    // 黄金锚点 0：seq=0, range [0,5), chain_head=a×64（与 governance 黄金向量同源）
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

/// 解析会话 WAL 文件名：session_{n}.wal 或轮换分片 session_{n}.wal.{seq}
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

/// 归并同会话的分片（基线以会话为键：分片集合整体观测）
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

/// 全链三查 + 链哈希轨迹（前缀锚定用）
fn verify_group(paths: &[PathBuf]) -> std::result::Result<(Vec<String>, u64, u64), String> {
    // paths 已排序：主文件 → .1 → .2（与写入时间序一致）
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

/// 无哈希账本（shared_facts）：结构完整性摘要
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
    // 严重度序：3 > 4 > 5 > 2 > 0（自检已在 run 前短路，此处不会有 2）
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

/// 基线前滚 + JSON 报告 + 人读摘要与处置指引
fn finalize_and_report(
    cmd: &ScanWalDir,
    now: &Baseline,
    worst: &mut u8,
    report_lines: &[String],
    first_run: bool,
    baseline_path: &Path,
) {
    // 写新基线（仅在无破坏时前滚；有破坏保留旧基线供取证比对；写出失败降级为
    // 告警——输出面故障不得改变扫描结论的退出码语义）
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
        println!("  处置指引：半行/损坏可用 `evorule truncate-partial-line <file> --confirm` 受控恢复（前缀链验证+备份）");
    }
}

/// 哨兵日志（append-only）：`ts | key | records | tail | verdict | delta`
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

/// 基线比对（前缀锚定）：obs=本轮观测，prev=上轮基线条目（None=新文件）
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
                    // 无哈希账本：内容变化如实报（追加与篡改不可分——账本无链可锚）
                    bump(EXIT_CHAIN_BROKEN, worst);
                    "UNHASHED-CHANGED（内容变化：正常追加或篡改不可分，人工确认）".to_string()
                }
            } else if obs.records < prev_obs.records {
                bump(EXIT_CHAIN_BROKEN, worst);
                format!(
                    "REGRESSED（记录回退：上轮 {} → 本轮 {}）",
                    prev_obs.records, obs.records
                )
            } else if obs.records == prev_obs.records {
                if obs.tail == prev_obs.tail {
                    "unchanged".to_string()
                } else {
                    bump(EXIT_CHAIN_BROKEN, worst);
                    "TAIL-CHANGED-SAME-LENGTH（同位链尾变化=篡改）".to_string()
                }
            } else {
                // 记录增加：前缀锚定——重放至上轮记录数处链尾必须==上轮 tail
                match verify_group(paths) {
                    Ok((chain, _, _)) => {
                        let anchor_idx = (prev_obs.records as usize).saturating_sub(1);
                        match chain.get(anchor_idx) {
                            Some(h) if h == &prev_obs.tail => {
                                "appended（前缀锚定通过）".to_string()
                            }
                            _ => {
                                bump(EXIT_CHAIN_BROKEN, worst);
                                "PREFIX-ANCHOR-MISMATCH（历史被改后重算/分片错位）".to_string()
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
    // ===== 层 A：尺子自检（失败=码 2，不产出数据结论）=====
    if let Err(e) = self_test() {
        eprintln!("[SENTINEL] SELF-TEST FAILED（验证器不可信，本轮不产出数据结论）: {e}");
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
        Err(_) => Baseline::default(), // 首跑
    };
    let first_run = old.files.is_empty();

    let mut now = Baseline::default();
    let mut worst: u8 = EXIT_OK;
    let mut report_lines: Vec<String> = Vec::new();

    for (key, paths) in &groups {
        let unhashed = key == "shared_facts";
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
                // ===== 层 B：基线比对（前缀锚定）=====
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
                // 解析/验证失败：区分「失配」（码 3）与「半行/损坏拒收」（码 5）
                let partial = e.contains("JSON parse error") || e.contains("EOF while parsing");
                let code = if partial {
                    EXIT_CORRUPT_REJECT
                } else {
                    EXIT_CHAIN_BROKEN
                };
                bump(code, &mut worst);
                report_lines.push(format!("{key}: REJECTED ({e})"));
                // 拒收文件仍入本轮观测（verdict=rejected 占位）：
                // 文件物理存在，不得被「文件消失」分支误报（码 4 与码 5 语义隔离）
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

    // 基线中存在而磁盘消失的会话 → 码 4
    for key in old.files.keys() {
        if !now.files.contains_key(key) && !first_run {
            bump(EXIT_FILE_GONE, &mut worst);
            report_lines.push(format!("{key}: FILE-GONE（基线有而磁盘无——截断/误删）"));
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

    /// 自检必须通过（黄金向量=尺子校准的本体测试）
    #[test]
    fn self_test_passes_with_current_binaries() {
        assert!(self_test().is_ok(), "哨兵自检失败：验证器与黄金向量漂移");
    }

    /// 内置快照常量必须与内核 SSOT 快照文件逐行一致（跨仓锁定的编译期+测试期双保险）
    #[test]
    fn builtin_snapshot_matches_kernel_file() {
        let kernel_snapshot = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../evorule-audit-chain/hash_snapshot.txt");
        let content = std::fs::read_to_string(&kernel_snapshot)
            .expect("内核 SSOT 快照文件应存在（同 workspace）");
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 7, "内核快照应为 7 行");
        for (i, line) in lines.iter().enumerate() {
            assert_eq!(
                *line,
                KERNEL_HASH_SNAPSHOT[i],
                "第 {} 行与内置常量不一致",
                i + 1
            );
        }
    }

    /// 会话文件名归并（含轮换分片与非会话文件排除）
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
