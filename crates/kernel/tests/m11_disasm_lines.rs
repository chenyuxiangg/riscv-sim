//! Lane J —— M-11 的反汇编器给条目填**真的** `file` / `line`。
//!
//! 台账 **D-121**（契约 v4 的 `DisasmEntryDto` 加了 `file` / `line`，但
//! 内核侧一律填 `None`）· 验收 **J-4 / J-5**（冻结规格
//! `docs/detailed-design/STAGE-9_V024_Leftovers.md` §1 裁决 C · §2.3）。
//!
//! ## 这些用例在防什么
//!
//! - 反汇编条目**永远**是 `None`（契约形状定好了，内核侧不填 ⇒ 前端
//!   永远看不到源码行号 ⇒ E-6「源码高亮跳进 osz 源码」永远做不出来）；
//! - 查不到行号时**编一个**出来（`Some("")` / `Some(0)` / 拿别的地址的
//!   行号顶替）⇒ 那是**撒谎**，比「没有」更坏；
//! - 「查过没命中」与「压根没查」被**混成同一件事**（STAGE-7 裁决 J 的
//!   老规矩）；在 DTO 上两者确实都是 `None`（契约形状决定）⇒ **端口**
//!   [`SymbolService::resolve_line`] 上必须还能分出来；
//! - 「每条指令只查一次行表」这句**性能承诺**只是散文（⇒ 反复查 N 次
//!   DWARF）——它必须**可观测**。
//!
//! ## 断言口径
//!
//! **所有**期望值都是**写死的字面量**（`"src/main.c"` / `42` / 具体
//! 地址）——**不**从被测实现反推 ⇒ 用例不会恒绿。
//!
//! ## DWARF 夹具
//!
//! 本文件**自带**一个最小的 **DWARF 4** `.debug_line` 节区（手写字节，
//! 仓库自包含、不依赖外部工具链）：两行（`src/main.c` 的第 1 行与第 6
//! 行），覆盖地址 `0x1000` 与 `0x1004`，`0x1008` 之后**没有**行 ⇒
//! 「查过、没命中」。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{DisasmEntryDto, LineEntry};
use riscv_sim_kernel::m11_symbol::{Disassembler, LineLookup, SymbolIndex, SymbolService};

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 代码的基址。
const BASE: u32 = 0x1000;

/// 夹具代码（**写死的字节**）：
///
/// | 地址 | 字节 | 指令 |
/// | --- | --- | --- |
/// | 0x1000 | 13 85 a5 02 | `addi a0,a1,42` |
/// | 0x1004 | 41 11 | `c.addi sp,-16` |
/// | 0x1006 | 23 22 b5 00 | `sw a1,4(a0)` |
const CODE: [u8; 10] = [
    0x13, 0x85, 0xa5, 0x02, // addi a0,a1,42
    0x41, 0x11, // c.addi sp,-16
    0x23, 0x22, 0xb5, 0x00, // sw a1,4(a0)
];

/// 一小段「内存」：基址 + 字节（读不到 ⇒ `Err` ⇒ 序列提前结束）。
fn memory(base: u32, bytes: Vec<u8>) -> impl FnMut(u32, u32) -> Result<u32, PlatformError> {
    move |addr: u32, size: u32| -> Result<u32, PlatformError> {
        let Some(offset) = addr.checked_sub(base) else {
            return Err(PlatformError::new(
                ErrorFamily::Context,
                1,
                "address below the base of the mapped region",
            ));
        };
        let start = offset as usize;
        let end = start + size as usize;
        if end > bytes.len() {
            return Err(PlatformError::new(
                ErrorFamily::Context,
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

/// 造一个最小的 **DWARF 4** `.debug_line` 节区。
///
/// 行号程序：
///
/// ```text
/// set_address 0x1000
/// copy                       ⇒ (0x1000, src/main.c, 1)
/// advance_pc 4               ⇒ 0x1004
/// advance_line +5            ⇒ 行 6
/// copy                       ⇒ (0x1004, src/main.c, 6)
/// advance_pc 4               ⇒ 0x1008
/// end_sequence               ⇒ 本段到 0x1008 为止（**不含** 0x1008）
/// ```
///
/// ⇒ `0x1000` 与 `0x1004` **命中**；`0x1008` 与 `0x9000` **没命中**
/// （「查过、没命中」）。
fn dwarf_v4_line_section() -> Vec<u8> {
    // v2~v4 头部（不含 `unit_length` / `version` / `header_length`）。
    let mut header: Vec<u8> = vec![
        1,    // minimum_instruction_length
        1,    // maximum_operations_per_instruction（v4+）
        1,    // default_is_stmt
        0xfb, // line_base = -5
        0x0e, // line_range = 14
        0x0d, // opcode_base = 13
    ];
    // standard_opcode_lengths（`opcode_base - 1` = 12 项）。
    header.extend_from_slice(&[0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1]);
    header.extend_from_slice(b"src\0"); // 目录 1
    header.push(0); // 目录表结束（空串）
    header.extend_from_slice(b"main.c\0"); // 文件 1
    header.extend_from_slice(&[1, 0, 0]); // dir=1, mtime=0, len=0
    header.push(0); // 文件表结束（空 name）

    let mut program: Vec<u8> = Vec::new();
    // DW_LNE_set_address（长度 = 1 + 4）
    program.push(0);
    program.push(5);
    program.push(2);
    program.extend_from_slice(&0x1000u32.to_le_bytes());
    program.push(1); // DW_LNS_copy
    program.push(2); // DW_LNS_advance_pc
    program.push(4); // 操作数 4
    program.push(3); // DW_LNS_advance_line
    program.push(0x05); // 操作数 +5（SLEB128）
    program.push(1); // DW_LNS_copy
    program.push(2); // DW_LNS_advance_pc
    program.push(4); // 操作数 4
                     // DW_LNE_end_sequence（长度 = 1）
    program.push(0);
    program.push(1);
    program.push(1);

    let mut unit: Vec<u8> = Vec::new();
    let body_len = 2 + 4 + header.len() + program.len();
    unit.extend_from_slice(&(body_len as u32).to_le_bytes());
    unit.extend_from_slice(&4u16.to_le_bytes());
    unit.extend_from_slice(&(header.len() as u32).to_le_bytes());
    unit.extend_from_slice(&header);
    unit.extend_from_slice(&program);
    unit
}

/// 带真行表的索引（`src/main.c` 的第 1 行与第 6 行）。
fn index_with_a_real_line_table() -> SymbolIndex {
    SymbolIndex::from_sections(&[(".debug_line".to_string(), dwarf_v4_line_section())])
        .expect("索引")
}

// ---------------------------------------------------------------------------
// a / b / c：填不填、填对不对、查不到时是不是 `None`
// ---------------------------------------------------------------------------

#[test]
fn disasm_without_a_resolver_leaves_the_file_and_the_line_none() {
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let entries = disasm.disasm(BASE, 3, &mut read).expect("disasm");

    assert_eq!(entries.len(), 3);
    for entry in &entries {
        assert_eq!(
            entry.file, None,
            "不传解析器 ⇒ `file` 必须是 `None`（**不是** `Some(\"\")`）"
        );
        assert_eq!(
            entry.line, None,
            "不传解析器 ⇒ `line` 必须是 `None`（**不是** `Some(0)`）"
        );
    }
    assert_eq!(
        disasm.line_resolutions(),
        0,
        "不带解析器 ⇒ 一次行表查询都不该发生"
    );
}

#[test]
fn a_resolver_fills_the_true_file_and_the_line() {
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let mut resolve = |_addr: u32| -> Option<LineEntry> {
        Some(LineEntry {
            addr: 0x1000,
            file: "src/main.c".to_string(),
            line: 42,
        })
    };
    let entries = disasm
        .disasm_with_lines(BASE, 3, &mut read, &mut resolve)
        .expect("disasm");

    assert_eq!(entries.len(), 3);
    for entry in &entries {
        assert_eq!(entry.file, Some("src/main.c".to_string()));
        assert_eq!(entry.line, Some(42));
    }
}

#[test]
fn a_resolver_that_finds_nothing_leaves_the_file_and_the_line_none() {
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let mut resolve = |_addr: u32| -> Option<LineEntry> { None };
    let entries = disasm
        .disasm_with_lines(BASE, 3, &mut read, &mut resolve)
        .expect("disasm");

    assert_eq!(entries.len(), 3);
    for entry in &entries {
        assert_eq!(entry.file, None, "查不到 ⇒ `file` 必须是 `None`");
        assert_eq!(entry.line, None, "查不到 ⇒ `line` 必须是 `None`");
    }
    // ⚠️ 「查了但没命中」**仍然算一次查询**（区别只在**结果**上）。
    assert_eq!(disasm.line_resolutions(), 3);
}

// ---------------------------------------------------------------------------
// d / e：缓存语义（「每条指令只查一次」是规格里的性能承诺）
// ---------------------------------------------------------------------------

#[test]
fn a_cached_address_resolves_the_line_only_once() {
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let mut resolve = |_addr: u32| -> Option<LineEntry> {
        Some(LineEntry {
            addr: 0x1000,
            file: "src/main.c".to_string(),
            line: 42,
        })
    };

    let first = disasm
        .disasm_with_lines(BASE, 3, &mut read, &mut resolve)
        .expect("disasm");
    assert_eq!(disasm.line_resolutions(), 3, "第一次：三条各查一次");

    let second = disasm
        .disasm_with_lines(BASE, 3, &mut read, &mut resolve)
        .expect("disasm");
    assert_eq!(
        disasm.line_resolutions(),
        3,
        "第二次全是缓存命中 ⇒ **不得**再查行表"
    );
    assert_eq!(first, second, "两次结果逐字段相同");
}

#[test]
fn each_distinct_address_resolves_its_own_line_once() {
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let mut resolve = |_addr: u32| -> Option<LineEntry> {
        Some(LineEntry {
            addr: 0x1000,
            file: "src/main.c".to_string(),
            line: 42,
        })
    };
    let entries = disasm
        .disasm_with_lines(BASE, 3, &mut read, &mut resolve)
        .expect("disasm");

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].addr, 0x1000);
    assert_eq!(entries[1].addr, 0x1004);
    assert_eq!(entries[2].addr, 0x1006);
    assert_eq!(
        disasm.line_resolutions(),
        3,
        "每条各查一次（不是查一次就复用）"
    );
}

// ---------------------------------------------------------------------------
// f / h：压缩指令、按地址查
// ---------------------------------------------------------------------------

#[test]
fn a_compressed_instruction_also_gets_a_line() {
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let mut resolve = |_addr: u32| -> Option<LineEntry> {
        Some(LineEntry {
            addr: 0x1004,
            file: "src/main.c".to_string(),
            line: 42,
        })
    };
    let entries = disasm
        .disasm_with_lines(BASE, 3, &mut read, &mut resolve)
        .expect("disasm");

    assert_eq!(entries[1].width, 2, "第二条是压缩指令");
    assert_eq!(entries[1].mnemonic, "c.addi sp,-16");
    assert_eq!(entries[1].file, Some("src/main.c".to_string()));
    assert_eq!(entries[1].line, Some(42));
}

#[test]
fn the_resolver_is_asked_for_the_address_of_the_entry_itself() {
    let queried = std::cell::RefCell::new(Vec::new());
    let mut resolve = |addr: u32| -> Option<LineEntry> {
        queried.borrow_mut().push(addr);
        match addr {
            0x1000 => Some(LineEntry {
                addr: 0x1000,
                file: "src/a.c".to_string(),
                line: 10,
            }),
            0x1004 => Some(LineEntry {
                addr: 0x1004,
                file: "src/b.c".to_string(),
                line: 20,
            }),
            0x1006 => Some(LineEntry {
                addr: 0x1006,
                file: "src/c.c".to_string(),
                line: 30,
            }),
            _ => None,
        }
    };
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let entries = disasm
        .disasm_with_lines(BASE, 3, &mut read, &mut resolve)
        .expect("disasm");

    assert_eq!(
        *queried.borrow(),
        vec![0x1000, 0x1004, 0x1006],
        "解析器必须按**每条自己**的地址被问"
    );
    assert_eq!(entries[0].file, Some("src/a.c".to_string()));
    assert_eq!(entries[0].line, Some(10));
    assert_eq!(entries[1].file, Some("src/b.c".to_string()));
    assert_eq!(entries[1].line, Some(20));
    assert_eq!(entries[2].file, Some("src/c.c".to_string()));
    assert_eq!(entries[2].line, Some(30));
}

// ---------------------------------------------------------------------------
// g：`SymbolIndex::resolve_line_entry` 的三态 ⇒ 两态
// ---------------------------------------------------------------------------

#[test]
fn resolve_line_entry_maps_found_to_some_and_the_two_no_answers_to_none() {
    let index = index_with_a_real_line_table();

    // ✅ 命中 ⇒ 真 `file` + 真 `line`。
    let hit = index.resolve_line_entry(0x1000).expect("命中");
    assert_eq!(hit.addr, 0x1000);
    assert_eq!(hit.file, "src/main.c");
    assert_eq!(hit.line, 1);
    let hit = index.resolve_line_entry(0x1004).expect("命中");
    assert_eq!(hit.addr, 0x1004);
    assert_eq!(hit.file, "src/main.c");
    assert_eq!(hit.line, 6);

    // ⚠️ 「查过、没命中」⇒ `None`（前提：这一状态真的是 `Missed`）。
    assert_eq!(
        index.resolve_line(0x9000).expect("端口"),
        LineLookup::Missed
    );
    assert_eq!(index.resolve_line_entry(0x9000), None);

    // ⚠️ 「压根没有 DWARF」⇒ `None`（前提：这一状态真的是 `NoDwarf`）。
    let empty = SymbolIndex::from_sections(&[]).expect("索引");
    assert_eq!(
        empty.resolve_line(0x1000).expect("端口"),
        LineLookup::NoDwarf
    );
    assert_eq!(empty.resolve_line_entry(0x1000), None);
}

// ---------------------------------------------------------------------------
// 端到端：真行表 ⇒ 反汇编条目带真 `file` / `line`
// ---------------------------------------------------------------------------

#[test]
fn a_real_line_table_fills_the_disassembly_with_the_true_file_and_line() {
    // 验收 **J-4**：DWARF 行表命中时，反汇编条目**填上真** `file` + `line`。
    let index = index_with_a_real_line_table();
    let mut disasm = Disassembler::new();
    let mut read = memory(BASE, CODE.to_vec());
    let entries = disasm
        .disasm_with_lines(BASE, 3, &mut read, |addr| index.resolve_line_entry(addr))
        .expect("disasm");

    assert_eq!(entries.len(), 3);
    // 0x1000 ⇒ 第 1 行。
    assert_eq!(entries[0].addr, 0x1000);
    assert_eq!(entries[0].file, Some("src/main.c".to_string()));
    assert_eq!(entries[0].line, Some(1));
    // 0x1004 ⇒ 第 6 行（压缩指令也一样）。
    assert_eq!(entries[1].addr, 0x1004);
    assert_eq!(entries[1].width, 2);
    assert_eq!(entries[1].file, Some("src/main.c".to_string()));
    assert_eq!(entries[1].line, Some(6));
    // 0x1006 ⇒ 行表只覆盖到 0x1008 之前 ⇒ 0x1006 **命中**第 6 行
    //（行号表按「最后一条 address ≤ 目标」查）。
    assert_eq!(entries[2].addr, 0x1006);
    assert_eq!(entries[2].file, Some("src/main.c".to_string()));
    assert_eq!(entries[2].line, Some(6));
    assert_eq!(disasm.line_resolutions(), 3);
}

#[test]
fn an_address_outside_the_line_table_gets_no_line_at_all() {
    // 验收 **J-5**：行表**没命中**时填 `None`（**不猜**）。
    let index = index_with_a_real_line_table();
    let mut disasm = Disassembler::new();
    // 代码放在 0x9000 —— 行表**不覆盖**那里。
    let mut read = memory(0x9000, CODE.to_vec());
    let entries = disasm
        .disasm_with_lines(0x9000, 2, &mut read, |addr| index.resolve_line_entry(addr))
        .expect("disasm");

    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert_eq!(entry.file, None, "没命中 ⇒ `file` 必须是 `None`");
        assert_eq!(entry.line, None, "没命中 ⇒ `line` 必须是 `None`");
    }
    // ⚠️ **查过了**（计数是 2）⇒ 「查过没命中」与「压根没查」在 DTO 上
    // 不可区分，但**查询确实发生过**。
    assert_eq!(disasm.line_resolutions(), 2);
}

// ---------------------------------------------------------------------------
// 宿主接线的**形状**（Lane H 要照着写的那段）
// ---------------------------------------------------------------------------

/// 仿宿主 `KernelHost::on_disasm` 的形状：同一个 `&mut self` 上同时要借
/// `self.disassembler`（**可变**）与 `self.index`（**不可变**）。
///
/// ⚠️ 这条用例**不**测行为，只测**能编译**：借用检查器按**字段**判
/// 不相交 ⇒ `self.disassembler` 与 `&self.index` 可以同时借 ⇒ 宿主
/// 可以直接把 `&self.index` 捕进解析器闭包里。
#[test]
fn the_host_can_borrow_the_index_and_the_disassembler_at_the_same_time() {
    struct HostLike {
        disassembler: Disassembler,
        index: SymbolIndex,
        code: Vec<u8>,
    }

    impl HostLike {
        fn on_disasm(&mut self, addr: u32, count: u32) -> Vec<DisasmEntryDto> {
            let base = 0x1000u32;
            let code = &self.code;
            let mut read = |addr: u32, size: u32| -> Result<u32, PlatformError> {
                let start = (addr - base) as usize;
                let end = start + size as usize;
                if end > code.len() {
                    return Err(PlatformError::new(ErrorFamily::Context, 1, "not mapped"));
                }
                let mut word = 0u32;
                for (index, byte) in code[start..end].iter().enumerate() {
                    word |= (*byte as u32) << (8 * index);
                }
                Ok(word)
            };
            // ⚠️ 三个借用分属**三个不同的字段** ⇒ 互不冲突。
            let index = &self.index;
            self.disassembler
                .disasm_with_lines(addr, count, &mut read, |a| index.resolve_line_entry(a))
                .expect("disasm")
        }
    }

    let mut host = HostLike {
        disassembler: Disassembler::new(),
        index: index_with_a_real_line_table(),
        code: CODE.to_vec(),
    };
    let entries = host.on_disasm(0x1000, 3);

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].file, Some("src/main.c".to_string()));
    assert_eq!(entries[0].line, Some(1));
    assert_eq!(entries[1].file, Some("src/main.c".to_string()));
    assert_eq!(entries[1].line, Some(6));
}
