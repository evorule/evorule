// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Flash WAL——write-ahead 事实账本的纯逻辑层（介质抽象 + 环形页布局 + 断电恢复）
//!
//! # 层次
//! - [`FlashMedium`]：介质抽象（read / program 只 1→0 / erase_page）。
//!   真板介质（STM32F4 sector / CH32V3 页擦除）随实机批实现同一 trait；
//!   本批以 [`RamFlashMedium`]（`Vec<u8>` 模拟，全 0xFF 初始化）全测逻辑面
//! - [`FlashWalStore`]：环形页布局上的 JSONL 行账本，行格式与 RAM 后端
//!   / native `WalWriter` 逐字节一致（同一构造源 [`crate::wal`]）
//!
//! # 布局参数化（与板级 memory.x 口径同步）
//! 环 = `page_count` 个等长页；F407 = sector0-3（16KB × 4 @ 0x08000000）、
//! V307 = 尾部页环（4KB × 8 @ 0x08038000）。页是写入原子边界：**单行不跨页**
//! （行放不下当前页即换页；行超页容量 fail-visible）。
//!
//! # Flash 语义与断电窗口
//! - **编程只 1→0**：program 前全量校验 `(old & new) == new`，违例
//!   fail-visible 且介质不受影响（校验先于写入，无半途状态）
//! - **提交边界 = 行尾 `\n` 哨兵**：`append` 逐字节 program（行 + `\n`），
//!   断电窗口内最多残留一条半行记录——无 `\n` 的尾部字节段不构成记录
//! - **恢复 = 前缀扫描续链**：[`FlashWalStore::open`] 逐页扫描，只认完整行；
//!   链尾哈希自最后完整行的 `chain_hash` 恢复，续 append 原样接链
//! - **污染页**：断电半行残留区不可复写（1→0 不可逆），该页此后仅作只读
//!   残迹，新记录自动前进到下一空页（页容量损失 ≤ 1 行）
//! - **环满 fail-visible**（[`McuError::WalFull`]）：审计链不静默丢——
//!   不回绕覆盖最老记录，等待外部导出/重擦
//!
//! # 一致性不变式（违者 [`McuError::WalCorrupted`]，损坏面显式暴露）
//! - 页内：第一个 0xFF 之后必须全 0xFF（顺序 program）
//! - 环内：数据页之后不允许出现空页后再有数据页的空洞
//! - 换页：目标页必须为空（防误擦数据页）

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use evorule_audit_chain::fact::Fact;

use crate::error::McuError;
use crate::wal::{wal_line_json, McuWalStore};

/// Flash 介质抽象（偏移均相对 WAL 环基址，真板映射到绝对 Flash 地址）
pub trait FlashMedium {
    /// 页容量（字节）；环形布局的写入/擦除原子边界
    fn page_size(&self) -> usize;
    /// 页数
    fn page_count(&self) -> usize;
    /// 读 `buf.len()` 字节到 `buf`
    ///
    /// # Errors
    /// 偏移越界返回 [`McuError::FlashRange`]。
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<(), McuError>;
    /// 编程（只 1→0）
    ///
    /// # Errors
    /// 偏移越界返回 [`McuError::FlashRange`]；目标字节含 0→1 位返回
    /// [`McuError::FlashProgramViolated`]（fail-visible，校验先于写入，
    /// 介质不受影响）。
    fn program(&mut self, offset: usize, bytes: &[u8]) -> Result<(), McuError>;
    /// 整页擦除（置 0xFF）
    ///
    /// # Errors
    /// 页号越界返回 [`McuError::FlashRange`]。
    fn erase_page(&mut self, page: usize) -> Result<(), McuError>;
}

/// RAM 模拟 Flash 介质（`Vec<u8>`，0xFF 语义全真）
///
/// 断电截断注入的载体：测试直接经 [`FlashMedium::program`] 写半行字节
/// 模拟断电残留，再经 [`FlashWalStore::open`] 验证恢复语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RamFlashMedium {
    data: Vec<u8>,
    page_size: usize,
}

impl RamFlashMedium {
    /// 创建全 0xFF 的模拟介质
    ///
    /// # Panics（host 测试便利路径，非 no_std 运行面）
    /// `page_size == 0`（除零防护交给调用方；板级真介质不经此构造）。
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
        // 先全量校验再写入（无半途状态：违例时介质保持原样）
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

/// 页扫描产物：完整行区间（相对页内起止）+ 断电半行残留标记
struct PageScan {
    /// 完整行区间（\n 终止）
    rows: Vec<(usize, usize)>,
    /// 页内是否有断电半行残留（\n 之后的非 0xFF 字节段）
    polluted: bool,
    /// 页内第一个 0xFF 位置（None = 页满）
    first_free: Option<usize>,
}

/// 单页扫描：产出页内完整行 + 断电半行残留标记
///
/// 页内形态约束（顺序 program 不变式）：第一个 0xFF 之后必须全 0xFF，
/// 违者 `WalCorrupted`（损坏面显式暴露）。
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
            // 页满：\n 之后的残余段（若有）不构成记录，但语义上同断电半行；
            // 页满本身即触发换页，残留标记不影响写指针（页满已强制换页）
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

/// Flash WAL 后端（环形页布局上的 JSONL 行账本）
///
/// 行内容全量缓存于 RAM（对拍/审计读取便利面）；板级实例化的行缓存
/// 预算（环容量 vs 内核堆池）随实机批复核。介质经 [`FlashMedium`] 注入。
pub struct FlashWalStore<M: FlashMedium> {
    medium: M,
    /// 完整行内容（无 `\n`，顺序 = 落账顺序）
    lines: Vec<String>,
    /// 每行在介质上的起始偏移（审计定位/导出面）
    line_offsets: Vec<usize>,
    /// 链尾哈希缓存（open 恢复自最后完整行；append 原样接链）
    tail_hash: String,
    /// 活跃页号（写入目标页）
    active_page: usize,
    /// 页内写指针（字节）；`page_size` 表示页满待换页
    write_offset: usize,
    /// 活跃页是否含断电半行残留（污染页：不可复写，append 强制换页）
    polluted: bool,
    /// 页容量缓存（open 时自介质取定）
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

/// 单行 `chain_hash` 解析（恢复扫描用：链尾续链的锚点）
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
    /// 打开（恢复）WAL 环：全环前缀扫描，只认完整行，续链就绪
    ///
    /// # Errors
    /// - [`McuError::WalCorrupted`]：页内非法形态（0xFF 后有字节）/ 环内
    ///   空洞（空页后再有数据页）/ 行非 JSON / 行缺 chain_hash
    /// - [`McuError::FlashRange`]：介质读取越界（理论不可达，页遍历内生）
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
            write_offset = scan.first_free.unwrap_or(page_size); // None = 页满
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

    /// 介质引用（观测/导出面）
    pub fn medium(&self) -> &M {
        &self.medium
    }

    /// 每行在介质上的起始偏移（审计定位面）
    pub fn line_offsets(&self) -> &[usize] {
        &self.line_offsets
    }

    /// 活跃页是否含断电半行残留（审计观测面）
    pub fn is_polluted(&self) -> bool {
        self.polluted
    }

    /// 介质可变引用（测试断电注入专用；生产面不暴露）
    #[cfg(test)]
    pub(crate) fn medium_mut_for_test(&mut self) -> &mut M {
        &mut self.medium
    }
}

impl<M: FlashMedium> McuWalStore for FlashWalStore<M> {
    /// 追加一行（write-ahead：program 成功即承诺已落）
    ///
    /// 行放不下当前页（或活跃页污染）→ 校验并擦除下一页后写入；
    /// 行超页容量 / 环满 / 编程违例 → fail-visible。
    /// program 失败时 store 指针已前进（新页为空可续写）；行账本
    /// `lines` 与介质实际行保持一致（未落行不记账）。
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

        // 换页判定：页满 / 剩余空间不足 / 活跃页污染（半行残迹不可复写）
        let need_new_page = self.polluted
            || self.write_offset >= self.page_size
            || self.page_size - self.write_offset < bytes.len();
        if need_new_page {
            let next = self.active_page + 1;
            if next >= self.medium.page_count() {
                return Err(McuError::WalFull);
            }
            // 换页防线：目标页必须为空（顺序写入不变式；防误擦数据页）
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

    /// 当前链尾哈希（空环为 `"genesis"`）
    fn chain_tail_hash(&self) -> &str {
        &self.tail_hash
    }

    /// 已落账的完整行数
    fn len(&self) -> usize {
        self.lines.len()
    }

    /// 第 `index` 行的 JSONL 行内容（无 `\n`，与 RAM 后端逐字节一致）
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

    /// 确定性 LCG（可复现随机断电注入；no_std 面不引入 rand 依赖）
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

    /// 干净空介质（页长 256 / 8 页，行 ~170B → 每页恰一行，天然练换页）
    fn medium() -> RamFlashMedium {
        RamFlashMedium::new(256, 8)
    }

    /// 构造第 `version_before` 版记录的完整行字节（行 + \n，借 RAM 后端算行）
    fn full_line_bytes(version_before: u64, prev_hash: &str, chain_hash: &str) -> String {
        let fact = command_fact(version_before + 1, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        let mut tmp = crate::wal::RamWalStore::new();
        tmp.append_record(version_before, &fact, &content, prev_hash, chain_hash)
            .unwrap();
        format!("{}\n", tmp.line_json(0).unwrap())
    }

    /// 经 trait 追加一条带 tag 的记录（chain_hash = "{tag}-{version}"，逐条唯一）
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
        // 先写 0b1010 再写 0b1000：新位全落在旧 1 位上（合法，结果 0b1000）
        m.program(0, &[0b1010]).unwrap();
        m.program(0, &[0b1000]).unwrap();
        let mut out = [0u8; 1];
        m.read(0, &mut out).unwrap();
        assert_eq!(out[0], 0b1000);
        // 写含 0→1 位的字节（bit2 旧 0 新 1）：fail-visible 且介质保持原样
        let err = m.program(0, &[0b0100]).unwrap_err();
        assert!(matches!(err, McuError::FlashProgramViolated { offset: 0 }));
        m.read(0, &mut out).unwrap();
        assert_eq!(out[0], 0b1000, "违例 program 不得改动介质");
    }

    #[test]
    fn test_erase_page_resets_to_ff() {
        let mut m = medium();
        m.program(0, &[0x00, 0x00]).unwrap();
        m.erase_page(0).unwrap();
        let mut out = [0u8; 2];
        m.read(0, &mut out).unwrap();
        assert_eq!(out, [0xFF, 0xFF]);
        // 页号越界 fail-visible
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

        // 行内容逐字节一致（同构造源 + 同输入）
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
        // 行 ~200B（含 \n）> 页 256B 只容一行 → 6 行应跨 6 页
        for (i, off) in store.line_offsets().iter().enumerate() {
            assert_eq!(off / 256, i, "行 {i} 应落在页 {i}");
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
        // 第 9 行 → 环满 fail-visible（不回绕覆盖）
        let fact = command_fact(9, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        let prev = String::from(store.chain_tail_hash());
        let err = store
            .append_record(8, &fact, &content, &prev, "chain-9")
            .unwrap_err();
        assert!(matches!(err, McuError::WalFull), "got: {err}");
        // 原链面不受影响
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

        // drop 后重开（进程重启模拟）
        let m = store.medium().clone();
        drop(store);
        let mut reopened = FlashWalStore::open(m).unwrap();
        assert_eq!(reopened.len(), 3);
        assert_eq!(reopened.chain_tail_hash(), tail_before);
        for (i, line) in lines_before.iter().enumerate() {
            assert_eq!(&reopened.line_json(i).unwrap(), line);
        }

        // 续 append 原样接链
        append_tagged(&mut reopened, 3, "t");
        assert_eq!(reopened.len(), 4);
        assert_eq!(reopened.chain_tail_hash(), "t-3");
    }

    #[test]
    fn test_power_cut_truncated_line_is_not_a_record() {
        // 断电注入：最后一条只写入前缀字节（无 \n）→ 恢复不认、活跃页污染
        let mut store = FlashWalStore::open(medium()).unwrap();
        for v in 0..2u64 {
            append_tagged(&mut store, v, "t");
        }
        let full = full_line_bytes(2, store.chain_tail_hash(), "t-2");
        let offset = store.medium().page_size() * 2; // 第 3 行应落在页 2
        store
            .medium_mut_for_test()
            .program(offset, &full.as_bytes()[..40])
            .unwrap();
        // 注入后取介质状态（进程重启模拟：介质持久，store 弃置）
        let m = store.medium().clone();
        drop(store);

        let reopened = FlashWalStore::open(m).unwrap();
        assert_eq!(reopened.len(), 2, "半行不构成记录");
        assert_eq!(reopened.chain_tail_hash(), "t-1");
        assert!(reopened.is_polluted(), "活跃页应标记污染");
    }

    #[test]
    fn test_power_cut_then_continue_appends_to_next_page() {
        // 断电半行 → 重开 → 续 append 自动前进下一空页，链面完整
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
        // 新行落在污染页（页 2）之后的空页（页 3）
        assert_eq!(
            reopened.line_offsets()[2] / reopened.medium().page_size(),
            3
        );
    }

    #[test]
    fn test_random_power_cut_injection_matrix() {
        // 随机断电截断矩阵：LCG 驱动截断点（含零残留/半行/完整行三态），
        // 逐点验证恢复行数 / 链尾 / 污染标记 / 续写能力
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
                "case {case}: k={k}/{} 截断后恢复行数不符",
                full.len()
            );
            let expected_tail = if k == full.len() { "t-2" } else { "t-1" };
            assert_eq!(
                reopened.chain_tail_hash(),
                expected_tail,
                "case {case}: k={k} 链尾不符"
            );
            assert_eq!(
                reopened.is_polluted(),
                k > 0 && k < full.len(),
                "case {case}: k={k} 污染标记不符"
            );
            // 恢复后必可继续落账（本用例环内必有余页）
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
        // 0xFF 区后又出现非 0xFF 字节：顺序写入不变式破坏 → fail-visible
        let mut m = medium();
        m.program(10, &[0xAA]).unwrap(); // 页 0 内 0xFF 区后再写
        m.program(100, &[0xBB]).unwrap();
        let err = FlashWalStore::open(m).unwrap_err();
        assert!(matches!(err, McuError::WalCorrupted(_)), "got: {err}");
    }

    #[test]
    fn test_ring_hole_fails_visible() {
        // 空页后再有数据页：环内空洞 → fail-visible
        let mut m = medium();
        m.program(0, &[0x11]).unwrap(); // 页 0 有数据
        m.program(256 * 2, &[0x22]).unwrap(); // 页 1 空、页 2 有数据
        let err = FlashWalStore::open(m).unwrap_err();
        assert!(matches!(err, McuError::WalCorrupted(_)), "got: {err}");
    }
}
