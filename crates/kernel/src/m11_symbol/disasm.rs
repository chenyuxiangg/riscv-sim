//! 反汇编序列 + 按地址缓存 + 自修改代码失效。
//!
//! 落地规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`（冻结）
//! §2 裁决 G · §5.2 验收 #1 / #2 / #4 / #5。
//!
//! ## 取指口径（LLD 11-1）
//!
//! **先读 2 字节判宽度**：低 2 位 ≠ `0b11` ⇒ 压缩指令（2 字节）；否则
//! **再补读 4 字节**。
//!
//! ⚠️ 之所以**不是**「一次读 4 字节」：映射区**末端**的压缩指令会因此
//! 越界 ⇒ 被误报成「未映射」。先读 2 字节就只碰**真正存在**的字节。
//!
//! ⚠️ 因此**宿主的 `read(addr, 2)` 必须真的只读 2 字节**（不能「读一个
//! 字再取低半」——那样越界又回来了）。
//!
//! ## 取指源是**回调**，不是 M-17 / M-14
//!
//! 依赖方向是 **M-11 ← M-17**（LLD §9.1）：M-11 **不得**反过来依赖
//! M-17 / M-14。所以取指走一个注入的闭包，由**宿主（Lane D）**把 M-17 的
//! 读内存接进来。
//!
//! 回调契约：`read(addr, size)` 读 `addr` 处的 `size` 字节（2 或 4），
//! 按**小端**拼成 `u32` 返回；读不到 ⇒ `Err`。
//!
//! ## 缓存（裁决 G）
//!
//! * 缓存**按地址索引**，条目含**机器码**（自修改代码失效后必须能重新
//!   取指）。
//! * **只失效被写的那个地址** —— [`Disassembler::invalidate`]。
//!   **不是整表清空**：整表清空会让自修改代码场景把整段反汇编缓存丢掉。
//! * 失效通道的**方向语义**用 M-18 的 `TraceDir`（契约层
//!   `riscv_sim_contract::model::TraceDir`，`Read` / `Write`）——
//!   [`Disassembler::invalidate_on_access`] 只在**写**时失效，**不**另造
//!   一套方向常量。
//!
//! ## 截断**不是**错误（LLD 11-3）
//!
//! 取指失败（未映射 / 宿主侧失败）⇒ **提前结束**，返回**部分序列**
//! （长度 < 请求条数），**不是**错误。宿主侧据此把「读不到」表达成
//! 「序列到此为止」，而不是一条错误事件。

use std::cell::Cell;
use std::collections::HashMap;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{DisasmEntryDto, TraceDir};

use crate::m13_iss::Decoder;

use super::render::render_entry;

/// 反汇编条数的**防御上限**（LLD 11-10）。
///
/// 超过它 ⇒ **拒绝**（`E_CTX-02`），不是截断后静默少给。
pub const MAX_DISASM_COUNT: u32 = 100_000;

/// 本模块的错误族：**Context**（请求本身不合法——不是内容解析失败）。
pub const DISASM_FAMILY: ErrorFamily = ErrorFamily::Context;

/// 场景码：请求的条数**超过内核上限**。
///
/// ⚠️ 用 **scenario 2**（「资源限制」）而不是 scenario 3 —— scenario 3
/// 已经是「请求参数 / 组合不合法」了（导出组合非法、执行轨迹拒绝 VCD
/// 都用它），再复用就是**同码不同义**。scenario 2 与 M-19 的内存预算
/// 同族同义。
pub const E_CTX_02_DISASM_COUNT_EXCEEDED: u16 = 2;

/// 反汇编器：按地址缓存已解码的指令条目。
#[derive(Debug, Default)]
pub struct Disassembler {
    /// 解码器（**M-13 的那一个**——裁决 A：零第二套解码）。
    decoder: Decoder,
    /// 地址 → 已渲染的条目。
    ///
    /// 条目里带着**机器码**（`code`）：自修改代码把某个地址改掉之后，
    /// 失效那个地址即可重新取指；缓存里的旧机器码**不会**被误当成新的。
    cache: HashMap<u32, DisasmEntryDto>,
    /// [`Disassembler::lookup`] 命中缓存的次数。
    ///
    /// ⚠️ **可观测性**：规格 §9.4 说「M-11 的 `disasmCache` 可以兜住
    /// （访存轨迹那次重复取指）」——那句话**必须可观测**，否则它只是一句
    /// 散文，谁也没法证明它成立。
    lookup_hits: Cell<u64>,
}

impl Disassembler {
    /// 造一个空的反汇编器。
    pub fn new() -> Self {
        Disassembler {
            decoder: Decoder::default(),
            cache: HashMap::new(),
            lookup_hits: Cell::new(0),
        }
    }

    /// 从 `addr` 开始反汇编**至多** `count` 条指令。
    ///
    /// * `count == 0` ⇒ 空序列（**不是**错误）。
    /// * `count > [`MAX_DISASM_COUNT`] ⇒ **拒绝**（`E_CTX-02`）。
    /// * 取指失败 ⇒ **提前结束**，返回**部分序列**（**不是**错误）。
    /// * 命中缓存 ⇒ **不取指**（直接用缓存里的条目，包括它的机器码与
    ///   宽度）。
    ///
    /// `read` 是**注入的取指源**：读 `addr` 处的 `size` 字节（2 或 4），
    /// 按**小端**拼成 `u32` 返回；读不到 ⇒ `Err`（⇒ 序列提前结束）。
    pub fn disasm(
        &mut self,
        addr: u32,
        count: u32,
        read: &mut impl FnMut(u32, u32) -> Result<u32, PlatformError>,
    ) -> Result<Vec<DisasmEntryDto>, PlatformError> {
        if count > MAX_DISASM_COUNT {
            return Err(PlatformError::new(
                DISASM_FAMILY,
                E_CTX_02_DISASM_COUNT_EXCEEDED,
                format!("requested {count} entries, limit is {MAX_DISASM_COUNT}"),
            ));
        }

        let mut entries: Vec<DisasmEntryDto> = Vec::new();
        let mut cursor = addr;
        while entries.len() < count as usize {
            // 缓存命中 ⇒ 不取指（这是「同一地址两次结果逐字段相同」的
            // 原因，也是自修改代码**必须显式失效**的原因）。
            if let Some(cached) = self.cache.get(&cursor) {
                let cached = cached.clone();
                cursor = cursor.wrapping_add(cached.width as u32);
                entries.push(cached);
                continue;
            }

            // 先读 2 字节判宽度（见模块文档「取指口径」）。
            let half = match read(cursor, 2) {
                Ok(word) => word,
                Err(_) => break,
            };
            let raw = if half & 0b11 != 0b11 {
                // 压缩指令：16 位原码就够了。
                half & 0xFFFF
            } else {
                // 32 位指令：补读 4 字节。
                match read(cursor, 4) {
                    Ok(word) => word,
                    Err(_) => break,
                }
            };

            let decoded = self.decoder.decode(raw, cursor);
            let entry = render_entry(&decoded);
            cursor = cursor.wrapping_add(entry.width as u32);
            self.cache.insert(entry.addr, entry.clone());
            entries.push(entry);
        }
        Ok(entries)
    }

    /// **失效一个地址**的缓存条目（裁决 G）。
    ///
    /// **只**失效这一个地址——**不是**整表清空。
    pub fn invalidate(&mut self, addr: u32) {
        self.cache.remove(&addr);
    }

    /// 按**访存方向**失效缓存（裁决 G 的失效通道）。
    ///
    /// 只有**写**才让反汇编缓存失效（读不改代码）；方向用 M-18 的
    /// `TraceDir`（契约层枚举）——**不**另造一套方向常量。
    pub fn invalidate_on_access(&mut self, addr: u32, dir: TraceDir) {
        if dir == TraceDir::Write {
            self.invalidate(addr);
        }
    }

    /// 当前缓存里的条目数（可观测性 + 用例）。
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// 按**地址**查缓存里已有的反汇编条目。
    ///
    /// ⚠️ **用途**（STAGE-5 §9.4）：访存轨迹里的**取指**事件要机器码，而
    /// M-18 的 `TraceEvent` **不带数据** ⇒ 宿主要么**再取一次指**，要么
    /// **复用这里的缓存**。规格 §9.4 的原话是「M-11 的 `disasmCache` 可以
    /// 兜住它」——**这个方法就是那句话的落点**。
    ///
    /// ⚠️ **缓存里的 `code` 是原码**（压缩指令 = 16 位原码，不是展开后的
    /// 32 位形式），与宿主现场取指的口径**一致** ⇒ 两条路给出同一个值。
    ///
    /// ⚠️ **命中 ≠ 一定新鲜**：失效是**写**驱动的（裁决 G）⇒ 只要访客
    /// 写过那个地址，条目就**已经被移除**了 ⇒ 命中即新鲜。
    ///
    /// ⚠️ **命中会计数**（[`Disassembler::lookup_hits`]）：这是「缓存替我们
    /// 省了一次取指」的**可观测**证据。
    pub fn lookup(&self, addr: u32) -> Option<&DisasmEntryDto> {
        let hit = self.cache.get(&addr);
        if hit.is_some() {
            self.lookup_hits.set(self.lookup_hits.get() + 1);
        }
        hit
    }

    /// [`Disassembler::lookup`] 命中缓存的次数。
    ///
    /// ⚠️ 这个数字**必须能被观测**，否则规格 §9.4 的「`disasmCache` 可以
    /// 兜住（重复取指）」只是一句散文。
    pub fn lookup_hits(&self) -> u64 {
        self.lookup_hits.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一小段「内存」：基址 + 字节。
    fn memory(base: u32, bytes: Vec<u8>) -> impl FnMut(u32, u32) -> Result<u32, PlatformError> {
        move |addr: u32, size: u32| -> Result<u32, PlatformError> {
            let Some(offset) = addr.checked_sub(base) else {
                return Err(PlatformError::new(
                    DISASM_FAMILY,
                    1,
                    "address below the base of the mapped region",
                ));
            };
            let start = offset as usize;
            let end = start + size as usize;
            if end > bytes.len() {
                return Err(PlatformError::new(
                    DISASM_FAMILY,
                    1,
                    "address is not mapped",
                ));
            }
            let mut word = 0u32;
            for (index, byte) in bytes[start..end].iter().enumerate() {
                word |= (*byte as u32) << (8 * index);
            }
            Ok(word)
        }
    }

    /// 夹具（基址 0x1000）：
    ///
    /// | 地址 | 字节 | 指令 |
    /// | --- | --- | --- |
    /// | 0x1000 | 13 85 a5 02 | `addi a0,a1,42` |
    /// | 0x1004 | 41 11 | `c.addi sp,-16` |
    /// | 0x1006 | 23 22 b5 00 | `sw a1,4(a0)` |
    const BASE: u32 = 0x1000;
    const CODE: [u8; 10] = [
        0x13, 0x85, 0xa5, 0x02, // addi a0,a1,42
        0x41, 0x11, // c.addi sp,-16
        0x23, 0x22, 0xb5, 0x00, // sw a1,4(a0)
    ];

    #[test]
    fn disassembly_yields_addr_code_mnemonic_and_width() {
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, CODE.to_vec());
        let entries = disasm.disasm(BASE, 3, &mut read).expect("disasm");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].addr, 0x1000);
        assert_eq!(entries[0].code, 0x02A5_8513);
        assert_eq!(entries[0].mnemonic, "addi a0,a1,42");
        assert_eq!(entries[0].width, 4);

        // 压缩指令：**16 位原码** + width = 2。
        assert_eq!(entries[1].addr, 0x1004);
        assert_eq!(entries[1].code, 0x1141);
        assert_eq!(entries[1].mnemonic, "c.addi sp,-16");
        assert_eq!(entries[1].width, 2);

        assert_eq!(entries[2].addr, 0x1006);
        assert_eq!(entries[2].code, 0x00B5_2223);
        assert_eq!(entries[2].mnemonic, "sw a1,4(a0)");
        assert_eq!(entries[2].width, 4);
    }

    #[test]
    fn width_is_decided_by_reading_two_bytes_first() {
        // 记录每次取指的 (地址, 字节数) ⇒ 钉住「先 2 字节、再补 4 字节」。
        let mut calls: Vec<(u32, u32)> = Vec::new();
        let mut bytes = CODE.to_vec();
        let mut read = |addr: u32, size: u32| -> Result<u32, PlatformError> {
            calls.push((addr, size));
            let start = (addr - BASE) as usize;
            let end = start + size as usize;
            if end > bytes.len() {
                return Err(PlatformError::new(DISASM_FAMILY, 1, "not mapped"));
            }
            let mut word = 0u32;
            for (index, byte) in bytes[start..end].iter().enumerate() {
                word |= (*byte as u32) << (8 * index);
            }
            Ok(word)
        };
        let mut disasm = Disassembler::new();
        let _ = disasm.disasm(BASE, 3, &mut read).expect("disasm");

        assert_eq!(
            calls,
            vec![
                (0x1000, 2), // 宽度探测
                (0x1000, 4), // 32 位 ⇒ 补读
                (0x1004, 2), // 压缩 ⇒ 不补读
                (0x1006, 2), // 宽度探测
                (0x1006, 4), // 32 位 ⇒ 补读
            ]
        );
        bytes.clear();
    }

    #[test]
    fn a_compressed_instruction_at_the_end_of_the_region_is_not_a_false_unmapped() {
        // 映射区**只有** 2 字节，且它是一条压缩指令。
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, vec![0x41, 0x11]);
        let entries = disasm.disasm(BASE, 4, &mut read).expect("disasm");
        assert_eq!(
            entries.len(),
            1,
            "the compressed instruction must be decoded"
        );
        assert_eq!(entries[0].addr, BASE);
        assert_eq!(entries[0].code, 0x1141);
        assert_eq!(entries[0].mnemonic, "c.addi sp,-16");
        assert_eq!(entries[0].width, 2);
    }

    #[test]
    fn a_failed_fetch_truncates_the_sequence_instead_of_erroring() {
        // 只映射前 4 字节 ⇒ 第二条指令读不到。
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, CODE[0..4].to_vec());
        let entries = disasm
            .disasm(BASE, 3, &mut read)
            .expect("truncation is not an error");
        assert_eq!(entries.len(), 1, "partial sequence");
        assert!(entries.len() < 3);
        assert_eq!(entries[0].addr, BASE);
    }

    #[test]
    fn a_failed_fetch_on_the_first_instruction_yields_an_empty_sequence() {
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, Vec::new());
        let entries = disasm
            .disasm(BASE, 3, &mut read)
            .expect("truncation is not an error");
        assert!(entries.is_empty());
    }

    #[test]
    fn the_same_address_disassembled_twice_gives_identical_entries() {
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, CODE.to_vec());
        let first = disasm.disasm(BASE, 3, &mut read).expect("disasm");
        let second = disasm.disasm(BASE, 3, &mut read).expect("disasm");
        assert_eq!(first, second, "cache hit must be field-for-field identical");
    }

    #[test]
    fn a_cache_hit_does_not_read_the_memory_again() {
        let calls = std::cell::RefCell::new(0usize);
        let bytes = CODE.to_vec();
        let mut read = |addr: u32, size: u32| -> Result<u32, PlatformError> {
            *calls.borrow_mut() += 1;
            let start = (addr - BASE) as usize;
            let end = start + size as usize;
            if end > bytes.len() {
                return Err(PlatformError::new(DISASM_FAMILY, 1, "not mapped"));
            }
            let mut word = 0u32;
            for (index, byte) in bytes[start..end].iter().enumerate() {
                word |= (*byte as u32) << (8 * index);
            }
            Ok(word)
        };
        let mut disasm = Disassembler::new();
        let first = disasm.disasm(BASE, 3, &mut read).expect("disasm");
        let after_first = *calls.borrow();
        let second = disasm.disasm(BASE, 3, &mut read).expect("disasm");
        assert_eq!(
            *calls.borrow(),
            after_first,
            "the second pass must not read memory"
        );
        assert_eq!(first, second);
    }

    #[test]
    fn a_cached_entry_survives_a_change_of_the_underlying_bytes() {
        // 先取一次（进缓存），再**改掉**底下的字节 ⇒ 缓存命中 ⇒ 仍是旧值。
        let bytes = std::cell::RefCell::new(CODE.to_vec());
        let mut read = |addr: u32, size: u32| -> Result<u32, PlatformError> {
            let bytes = bytes.borrow();
            let start = (addr - BASE) as usize;
            let end = start + size as usize;
            if end > bytes.len() {
                return Err(PlatformError::new(DISASM_FAMILY, 1, "not mapped"));
            }
            let mut word = 0u32;
            for (index, byte) in bytes[start..end].iter().enumerate() {
                word |= (*byte as u32) << (8 * index);
            }
            Ok(word)
        };
        let mut disasm = Disassembler::new();
        let first = disasm.disasm(BASE, 1, &mut read).expect("disasm");
        assert_eq!(first[0].mnemonic, "addi a0,a1,42");

        // 自修改代码：把 0x1000 处的指令改成 `addi a0,a1,43`。
        bytes.borrow_mut()[0..4].copy_from_slice(&0x02B5_8513u32.to_le_bytes());
        let second = disasm.disasm(BASE, 1, &mut read).expect("disasm");
        assert_eq!(
            second[0].mnemonic, "addi a0,a1,42",
            "cache hit must serve the old code"
        );
        assert_eq!(second[0].code, 0x02A5_8513);
    }

    #[test]
    fn invalidating_one_address_re_fetches_only_that_address() {
        // 裁决 G：**只**失效被写的那个地址。
        let bytes = std::cell::RefCell::new(CODE.to_vec());
        let mut read = |addr: u32, size: u32| -> Result<u32, PlatformError> {
            let bytes = bytes.borrow();
            let start = (addr - BASE) as usize;
            let end = start + size as usize;
            if end > bytes.len() {
                return Err(PlatformError::new(DISASM_FAMILY, 1, "not mapped"));
            }
            let mut word = 0u32;
            for (index, byte) in bytes[start..end].iter().enumerate() {
                word |= (*byte as u32) << (8 * index);
            }
            Ok(word)
        };
        let mut disasm = Disassembler::new();
        let first = disasm.disasm(BASE, 3, &mut read).expect("disasm");
        assert_eq!(first[0].mnemonic, "addi a0,a1,42");
        assert_eq!(first[2].mnemonic, "sw a1,4(a0)");
        let cached_before = disasm.cache_len();
        assert_eq!(cached_before, 3);

        // 改掉**两条**指令的字节，只失效其中一条的地址。
        bytes.borrow_mut()[0..4].copy_from_slice(&0x02B5_8513u32.to_le_bytes());
        bytes.borrow_mut()[6..10].copy_from_slice(&0x00B5_2423u32.to_le_bytes());
        disasm.invalidate(BASE); // 只失效 0x1000

        let after = disasm.disasm(BASE, 3, &mut read).expect("disasm");
        assert_eq!(
            after[0].mnemonic, "addi a0,a1,43",
            "the invalidated address must be fetched again"
        );
        assert_eq!(
            after[2].mnemonic, "sw a1,4(a0)",
            "the other addresses must still be served from the cache"
        );
        // 缓存条目数不变 ⇒ **不是**整表清空（清空的话这里会是 1）。
        assert_eq!(disasm.cache_len(), cached_before);
    }

    #[test]
    fn invalidating_on_a_read_does_not_drop_the_entry() {
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, CODE.to_vec());
        let _ = disasm.disasm(BASE, 1, &mut read).expect("disasm");
        assert_eq!(disasm.cache_len(), 1);
        // 读 ⇒ 不失效（读不改代码）。
        disasm.invalidate_on_access(BASE, TraceDir::Read);
        assert_eq!(disasm.cache_len(), 1);
        // 写 ⇒ 失效。
        disasm.invalidate_on_access(BASE, TraceDir::Write);
        assert_eq!(disasm.cache_len(), 0);
    }

    #[test]
    fn a_count_above_the_limit_is_rejected() {
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, CODE.to_vec());
        let err = disasm
            .disasm(BASE, MAX_DISASM_COUNT + 1, &mut read)
            .expect_err("must be rejected");
        assert_eq!(err.family, DISASM_FAMILY);
        assert_eq!(err.scenario, E_CTX_02_DISASM_COUNT_EXCEEDED);
        // 刚好等于上限 ⇒ 允许。
        let _ = disasm
            .disasm(BASE, MAX_DISASM_COUNT, &mut read)
            .expect("allowed");
    }

    #[test]
    fn a_count_of_zero_gives_an_empty_sequence() {
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, CODE.to_vec());
        let entries = disasm.disasm(BASE, 0, &mut read).expect("not an error");
        assert!(entries.is_empty());
        assert_eq!(disasm.cache_len(), 0, "nothing was fetched");
    }

    #[test]
    fn lookup_serves_the_cached_entry_and_counts_the_hits() {
        // 规格 §9.4：「M-11 的 `disasmCache` 可以兜住（重复取指）」——
        // 这个方法就是那句话的落点，**命中必须可计数**。
        let mut disasm = Disassembler::new();
        let mut read = memory(BASE, CODE.to_vec());
        let _ = disasm.disasm(BASE, 1, &mut read).expect("disasm");
        assert_eq!(disasm.cache_len(), 1);

        // 未命中 ⇒ `None`，**不**计数。
        assert!(disasm.lookup(BASE + 0x1000).is_none());
        assert_eq!(disasm.lookup_hits(), 0, "未命中不计");

        // 命中 ⇒ 给出**同一个**条目（含机器码）⇒ 计数 +1。
        let hit = disasm.lookup(BASE).expect("cached");
        assert_eq!(hit.code, 0x02A5_8513, "命中给的是缓存里的原码");
        assert_eq!(hit.width, 4);
        assert_eq!(disasm.lookup_hits(), 1);

        // 失效之后 ⇒ 又查不到了。
        disasm.invalidate(BASE);
        assert!(disasm.lookup(BASE).is_none());
        assert_eq!(disasm.lookup_hits(), 1, "失效后的查询不算命中");
    }
}
