// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 判定面：导出四步 / WAL 三步 + 链折叠
//!
//! 报告式判定采用**首红即停**语义（与哨兵/独立验证器短路语义一致）：
//! 断链后折叠链头失去锚，后续条目标记为未判定（`stopped_at`）。
//! 全绿 = 无停止点 且 链尾与声明尾哈希对账成功。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::record::{ExportDoc, WalLine, GENESIS};
use evorule_audit_chain::hash::{chain_step, fact_hash, fact_to_stable_json};

/// 哈希串短展示（前 16 字符）
fn short(h: &str) -> String {
    if h.len() <= 16 {
        String::from(h)
    } else {
        format!("{}…", &h[..16])
    }
}

/// 导出条目判定步骤
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportStep {
    /// ① prev 链接
    Link,
    /// ② 内容哈希（fact_hash 公式重算）
    Content,
    /// ③ stable 深比（重算 stable 与原文对账 + 条目投影一致性）
    Stable,
    /// 链尾对账（last_hash）
    Tail,
}

/// 导出条目否决详情
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryReject {
    pub step: ExportStep,
    pub reason: String,
}

/// 导出条目判定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryVerdict {
    pub index: usize,
    pub ok: bool,
    pub reject: Option<EntryReject>,
}

/// 导出面报告
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// 逐条判定（至首红为止：全绿条 + 首红条）
    pub verdicts: Vec<EntryVerdict>,
    /// 首红条下标（全绿为 None）
    pub stopped_at: Option<usize>,
    /// 折叠链尾（至停止点）
    pub tail_hash: String,
    /// 链尾与 `last_hash` 对账结果
    pub last_hash_matches: bool,
}

impl ExportReport {
    /// 全绿判定
    pub fn all_ok(&self) -> bool {
        self.stopped_at.is_none() && self.last_hash_matches
    }
}

/// 导出面判定（四步/条 + 链尾对账）
pub fn verify_export(doc: &ExportDoc) -> ExportReport {
    let mut prev = String::from(GENESIS);
    let mut verdicts = Vec::with_capacity(doc.entries.len());
    for (index, e) in doc.entries.iter().enumerate() {
        // ① 链接完整性：声明 prev 必须等于折叠链头
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
        // ② 内容哈希：fact_hash 公式重算（蕴含 fact_json_stable 字节原文对账）
        let recomputed = match fact_hash(&e.fact) {
            Ok(h) => h,
            Err(err) => {
                verdicts.push(EntryVerdict {
                    index,
                    ok: false,
                    reject: Some(EntryReject {
                        step: ExportStep::Content,
                        reason: format!("fact_hash 重算失败: {err}"),
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
        // ③ stable 深比：重算 stable 与 fact_json_stable 原文 Value 级对账，
        //    外加条目投影（fact_id / fact_type）一致性
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
                        "fact_json_stable 原文与 fact 字段重算结果不一致（或原文非法 JSON）",
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
                        "条目投影不符: fact_id({})/fact_type({}) 与 fact 字段不一致",
                        e.fact_id, e.fact_type
                    ),
                }),
            });
            return finish(verdicts, Some(index), prev, doc);
        }
        // ④ 链折叠
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

/// WAL 行否决详情
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalReject {
    /// 步骤序（1=链接 2=内容哈希 3=链折叠）
    pub step: u8,
    pub reason: String,
}

/// WAL 行判定
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalLineVerdict {
    pub index: usize,
    pub ok: bool,
    pub reject: Option<WalReject>,
}

/// WAL 面报告
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalReport {
    pub verdicts: Vec<WalLineVerdict>,
    pub stopped_at: Option<usize>,
    pub tail_hash: String,
}

impl WalReport {
    /// 全绿判定
    pub fn all_ok(&self) -> bool {
        self.stopped_at.is_none()
    }
}

/// WAL 面判定（三步/行，首红即停）
pub fn verify_wal_lines(lines: &[WalLine]) -> WalReport {
    let mut prev = String::from(GENESIS);
    let mut verdicts = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        // ① 链接完整性
        if line.prev_hash != prev {
            verdicts.push(WalLineVerdict {
                index,
                ok: false,
                reject: Some(WalReject {
                    step: 1,
                    reason: format!(
                        "prev_hash={} 与链尾={} 不符（链接断裂）",
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
        // ② 内容哈希：fact_hash 公式重算
        match fact_hash(&line.fact) {
            Ok(h) if h == line.content_hash => {}
            Ok(h) => {
                verdicts.push(WalLineVerdict {
                    index,
                    ok: false,
                    reject: Some(WalReject {
                        step: 2,
                        reason: format!(
                            "content_hash 失配: 声明={} 重算={}",
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
                        reason: format!("fact_hash 重算失败: {err}"),
                    }),
                });
                return WalReport {
                    verdicts,
                    stopped_at: Some(index),
                    tail_hash: prev,
                };
            }
        }
        // ③ 链折叠完整性
        let folded = chain_step(&line.prev_hash, &line.content_hash);
        if folded != line.chain_hash {
            verdicts.push(WalLineVerdict {
                index,
                ok: false,
                reject: Some(WalReject {
                    step: 3,
                    reason: format!(
                        "chain_hash 失配: 声明={} 重算={}",
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
