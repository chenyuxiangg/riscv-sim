//! v0.57 批（STAGE-42）的**内核侧**判别力用例 —— 验收 **K-1 ~ K-15**。
//!
//! ⚠️ **这些用例盯的是「宿主这条路真的接上了」**，不是「单元函数算对了」——
//!    单元层（[`crate::m08_breakpoint::cond`] / [`crate::m11_symbol`]）的
//!    判据在**各自模块**里。
//!
//! ⚠️ **三条能力**：符号查询（`ResolveSymbol`）· 整段转储（`DumpMemory`）·
//!    条件断点（`Breakpoint::condition`）。

use super::*;

use riscv_sim_contract::model::{Breakpoint, ProductTree, ProjectVersion, Symbol};

/// demo 固件里 **`sw x3, 256(x0)`** 那条指令的地址（v0.57 批的断点靶点）。
///
/// ⚠️ 它**不是** `tests.rs` 里那个 `DEMO_BREAKPOINT_ADDR`（0x20）⇒ ⇒
/// 这里要的是「一条**看得见副作用**的指令」⇒ ⇒ 停在它**之前** ⇒ ⇒
/// `0x100` 那个地址**必须还是 0**（K-13 的「该指令未执行」）。
const DEMO_STORE_INSN_ADDR: u32 = 0x0c;

/// demo 固件写进内存的目标地址（`sw x3, 256(x0)`）。
const DEMO_STORE_ADDR: u32 = 0x100;

/// demo 固件写进去的值（42 + 7）。
const DEMO_STORE_VALUE: u32 = 49;

/// 造一个挂着 demo 固件的工程。
///
/// ⚠️ **固件字节引用共享常量**（`kernel_host::DEMO_FIRMWARE_BYTES`）⇒ ⇒
/// 不抄一份。
fn demo_project() -> Project {
    Project {
        id: "proj-v057-demo".to_string(),
        name: "v057-demo".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(DEMO_FIRMWARE_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 取第一条 `Halted`。
fn first_halted(events: &[EventPayload]) -> (HaltReason, u32) {
    for e in events {
        if let EventPayload::Halted { reason, pc } = e {
            return (*reason, *pc);
        }
    }
    panic!("没有 Halted 事件：{events:?}");
}

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 取第一条 `Error`。
fn first_error(events: &[EventPayload]) -> (String, String) {
    for e in events {
        if let EventPayload::Error { code, message } = e {
            return (code.clone(), message.clone());
        }
    }
    panic!("没有 Error 事件：{events:?}");
}

/// 取第一条 `SymbolResolved`。
fn first_symbol_resolved(events: &[EventPayload]) -> (String, Option<Symbol>, Vec<Symbol>, u32) {
    for e in events {
        if let EventPayload::SymbolResolved {
            name,
            symbol,
            candidates,
            total,
        } = e
        {
            return (name.clone(), symbol.clone(), candidates.clone(), *total);
        }
    }
    panic!("没有 SymbolResolved 事件：{events:?}");
}

/// 取第一条 `MemoryDump`。
fn first_memory_dump(events: &[EventPayload]) -> (u32, Vec<u8>) {
    for e in events {
        if let EventPayload::MemoryDump { addr, bytes } = e {
            return (*addr, bytes.clone());
        }
    }
    panic!("没有 MemoryDump 事件：{events:?}");
}

// ---------------------------------------------------------------------------
// 夹具：带 `.symtab` / `.strtab` 的最小 ELF（**只拼字节** —— M-06 仍是唯一
// 的 ELF 解析者，M-11 仍是唯一的符号表解析者）
// ---------------------------------------------------------------------------

/// 造一个 `.symtab` + `.strtab`（`Elf32_Sym`，16 字节一条）。
///
/// ⚠️ 符号的**地址**是 `0x1000 + index * 4` ⇒ ⇒ 名字与地址的对应关系
/// **可预测** ⇒ ⇒ 用例可以断言具体的地址。
fn elf_symbol_table(names: &[&str]) -> (Vec<u8>, Vec<u8>) {
    let mut strtab = vec![0u8];
    let mut offsets = Vec::new();
    for name in names {
        offsets.push(strtab.len() as u32);
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
    }
    let mut symtab = Vec::new();
    symtab.extend_from_slice(&elf_sym(0, 0, 0, 0, 0));
    for (index, offset) in offsets.iter().enumerate() {
        symtab.extend_from_slice(&elf_sym(*offset, 0x1000 + index as u32 * 4, 4, 0x21, 1));
    }
    (symtab, strtab)
}

/// 一个 `Elf32_Sym` 条目（16 字节，小端）。
fn elf_sym(name: u32, value: u32, size: u32, info: u8, shndx: u16) -> [u8; 16] {
    let mut entry = [0u8; 16];
    entry[0..4].copy_from_slice(&name.to_le_bytes());
    entry[4..8].copy_from_slice(&value.to_le_bytes());
    entry[8..12].copy_from_slice(&size.to_le_bytes());
    entry[12] = info;
    entry[14..16].copy_from_slice(&shndx.to_le_bytes());
    entry
}

/// 造一个带**调试节区**的工程（ELF 镜像）。
fn elf_project(sections: &[(&str, Vec<u8>)]) -> Project {
    Project {
        id: "proj-v057-elf".to_string(),
        name: "v057-elf".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "firmware.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(build_elf_with_sections(sections)),
        }],
        version: ProjectVersion::default(),
    }
}

/// 造一个最小 ELF32（RV32 / ET_EXEC / LE），带给定的节区。
///
/// ⚠️ **只拼字节**：M-06 是唯一的 ELF 解析者。
fn build_elf_with_sections(sections: &[(&str, Vec<u8>)]) -> Vec<u8> {
    const EHDR_SIZE: usize = 52;
    const PHDR_SIZE: usize = 32;
    const SHDR_SIZE: usize = 40;
    const TEXT_OFF: u32 = (EHDR_SIZE + PHDR_SIZE) as u32;
    const TEXT_LEN: u32 = 4;
    const SHT_PROGBITS: u32 = 1;
    const SHT_STRTAB: u32 = 3;

    // `.shstrtab`：偏移 0 是空名（NULL 节区的 `sh_name`）。
    let mut shstr: Vec<u8> = vec![0u8];
    let mut name_offsets: Vec<u32> = Vec::with_capacity(sections.len());
    for (name, _) in sections {
        name_offsets.push(shstr.len() as u32);
        shstr.extend_from_slice(name.as_bytes());
        shstr.push(0);
    }
    let shstrtab_name = shstr.len() as u32;
    shstr.extend_from_slice(b".shstrtab");
    shstr.push(0);

    // 文件体。
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&[0u8; EHDR_SIZE]);
    bytes.extend_from_slice(&[0u8; PHDR_SIZE]);
    // `.text`：`addi x1, x0, 42`。
    bytes.extend_from_slice(&[0x93, 0x00, 0xA0, 0x02]);
    let mut section_offsets: Vec<u32> = Vec::with_capacity(sections.len());
    for (_, data) in sections {
        section_offsets.push(bytes.len() as u32);
        bytes.extend_from_slice(data);
    }
    let shstrtab_offset = bytes.len() as u32;
    bytes.extend_from_slice(&shstr);

    // 节区头表：[0] = NULL，[1] = `.shstrtab`，[2..] = 用户节区。
    let shnum = 2 + sections.len();
    let shoff = bytes.len() as u32;
    let mut shdrs: Vec<u8> = Vec::with_capacity(shnum * SHDR_SIZE);
    shdrs.extend_from_slice(&[0u8; SHDR_SIZE]);
    push_shdr(
        &mut shdrs,
        shstrtab_name,
        SHT_STRTAB,
        shstrtab_offset,
        shstr.len() as u32,
    );
    for (index, (_, data)) in sections.iter().enumerate() {
        push_shdr(
            &mut shdrs,
            name_offsets[index],
            SHT_PROGBITS,
            section_offsets[index],
            data.len() as u32,
        );
    }
    bytes.extend_from_slice(&shdrs);

    // ELF 头。
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = 1; // EI_CLASS = ELFCLASS32
    bytes[5] = 1; // EI_DATA  = ELFDATA2LSB
    bytes[6] = 1; // EI_VERSION
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
    bytes[18..20].copy_from_slice(&0xF3u16.to_le_bytes()); // e_machine = EM_RISCV
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    bytes[28..32].copy_from_slice(&(EHDR_SIZE as u32).to_le_bytes()); // e_phoff
    bytes[32..36].copy_from_slice(&shoff.to_le_bytes()); // e_shoff
    bytes[40..42].copy_from_slice(&(EHDR_SIZE as u16).to_le_bytes()); // e_ehsize
    bytes[42..44].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes()); // e_phentsize
    bytes[44..46].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
    bytes[46..48].copy_from_slice(&(SHDR_SIZE as u16).to_le_bytes()); // e_shentsize
    bytes[48..50].copy_from_slice(&(shnum as u16).to_le_bytes()); // e_shnum
    bytes[50..52].copy_from_slice(&1u16.to_le_bytes()); // e_shstrndx

    // 程序头：一个 PT_LOAD 覆盖 `.text`。
    bytes[52..56].copy_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
    bytes[56..60].copy_from_slice(&TEXT_OFF.to_le_bytes()); // p_offset
    bytes[68..72].copy_from_slice(&TEXT_LEN.to_le_bytes()); // p_filesz
    bytes[72..76].copy_from_slice(&TEXT_LEN.to_le_bytes()); // p_memsz
    bytes[76..80].copy_from_slice(&5u32.to_le_bytes()); // p_flags = R|X
    bytes[80..84].copy_from_slice(&4u32.to_le_bytes()); // p_align

    bytes
}

/// 追加一个 ELF32 节区头（40 字节）。
fn push_shdr(out: &mut Vec<u8>, sh_name: u32, sh_type: u32, sh_offset: u32, sh_size: u32) {
    out.extend_from_slice(&sh_name.to_le_bytes()); // sh_name
    out.extend_from_slice(&sh_type.to_le_bytes()); // sh_type
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
    out.extend_from_slice(&sh_offset.to_le_bytes()); // sh_offset
    out.extend_from_slice(&sh_size.to_le_bytes()); // sh_size
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_link
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_info
    out.extend_from_slice(&1u32.to_le_bytes()); // sh_addralign
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_entsize
}

// ---------------------------------------------------------------------------
// K-1 / K-2 —— 前缀匹配（**不是**子串匹配）
//
// ⚠️ 那两条判据的**单元层**用例住在 `m11_symbol::symbols` 的测试里 ⇒ ⇒
//    这里只钉「**宿主**这条路真的接上了」（前缀候选进了事件）。
// ---------------------------------------------------------------------------

/// **K-1 / K-2 的宿主侧**：前缀候选进了 `SymbolResolved` 事件。
#[test]
fn k1_the_host_reports_the_prefix_candidates() {
    let mut host = KernelHost::new();
    let (symtab, strtab) = elf_symbol_table(&["main", "malloc", "domain"]);
    dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(&[(".symtab", symtab), (".strtab", strtab)])),
    );
    let events = dispatch(
        &mut host,
        CommandPayload::ResolveSymbol {
            name: "ma".to_string(),
        },
    );
    let (_, symbol, candidates, total) = first_symbol_resolved(&events);
    assert!(symbol.is_none(), "`ma` 不是任何一个符号的全名 ⇒ 无精确命中");
    let names: Vec<&str> = candidates.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["main", "malloc"], "按名字升序");
    assert!(
        !names.contains(&"domain"),
        "`domain` 含 `ma` 但不以 `ma` 开头 ⇒ ⇒ 不该是候选"
    );
    assert_eq!(total, 3, "`total` 是该镜像的符号总数（**不是**候选数）");
}

// ---------------------------------------------------------------------------
// K-3 / K-4 —— 宿主的 `ResolveSymbol`
// ---------------------------------------------------------------------------

/// **K-3**：**精确命中** ⇒ `symbol` 有值、`candidates` **不含**它。
#[test]
fn k3_an_exact_hit_reports_the_symbol_and_keeps_it_out_of_the_candidates() {
    let mut host = KernelHost::new();
    let (symtab, strtab) = elf_symbol_table(&["main", "malloc"]);
    dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(&[(".symtab", symtab), (".strtab", strtab)])),
    );
    let events = dispatch(
        &mut host,
        CommandPayload::ResolveSymbol {
            name: "main".to_string(),
        },
    );
    let (name, symbol, candidates, total) = first_symbol_resolved(&events);
    assert_eq!(name, "main", "回显请求里的名字");
    let exact = symbol.expect("精确命中 ⇒ `symbol` 必须有值");
    assert_eq!(exact.name, "main");
    assert_eq!(exact.addr, Some(0x1000));
    assert!(
        !candidates.iter().any(|c| c.name == "main"),
        "精确命中**不**该同时出现在候选里：{candidates:?}"
    );
    assert_eq!(total, 2, "`total` 是该镜像的符号总数");
}

/// **K-3 的另一半**：**只有前缀命中** ⇒ `symbol == None` 且 `candidates`
/// 有值。
#[test]
fn k3_a_prefix_only_hit_reports_no_exact_symbol_but_candidates() {
    let mut host = KernelHost::new();
    let (symtab, strtab) = elf_symbol_table(&["main", "malloc"]);
    dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(&[(".symtab", symtab), (".strtab", strtab)])),
    );
    let events = dispatch(
        &mut host,
        CommandPayload::ResolveSymbol {
            name: "mal".to_string(),
        },
    );
    let (_, symbol, candidates, total) = first_symbol_resolved(&events);
    assert!(symbol.is_none(), "没有精确命中 ⇒ `symbol == None`");
    let names: Vec<&str> = candidates.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["malloc"], "候选按名字升序");
    assert_eq!(total, 2);
}

/// **K-4**：**空名字** ⇒ `E_DBG_02`，**不发** `SymbolResolved`。
#[test]
fn k4_an_empty_symbol_name_is_rejected_and_no_event_is_sent() {
    let mut host = KernelHost::new();
    let (symtab, strtab) = elf_symbol_table(&["main"]);
    dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(&[(".symtab", symtab), (".strtab", strtab)])),
    );
    for name in ["", "   ", "\t\n "] {
        let events = dispatch(
            &mut host,
            CommandPayload::ResolveSymbol {
                name: name.to_string(),
            },
        );
        let (code, message) = first_error(&events);
        assert_eq!(code, "E_DBG-02", "空名字必须被拒（名字 {name:?}）");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EventPayload::SymbolResolved { .. })),
            "被拒的查询**不**发 `SymbolResolved`：{events:?}"
        );
        assert!(!message.is_empty(), "错误消息不能是空的");
    }
}

/// **两态可区分**：没装载镜像 ⇒ `total == 0` ⇒ ⇒ 「这个镜像没有符号表」。
#[test]
fn an_image_without_a_symbol_table_reports_a_total_of_zero() {
    let mut host = KernelHost::new();
    let events = dispatch(
        &mut host,
        CommandPayload::ResolveSymbol {
            name: "main".to_string(),
        },
    );
    let (_, symbol, candidates, total) = first_symbol_resolved(&events);
    assert_eq!(total, 0, "没有符号表 ⇒ `total == 0`");
    assert!(symbol.is_none());
    assert!(candidates.is_empty());
}

// ---------------------------------------------------------------------------
// K-5 / K-6 / K-7 / K-8 —— 宿主的 `DumpMemory`
// ---------------------------------------------------------------------------

/// **K-5**：`len == 0` / `len > 4096` ⇒ `E_DBG_03`，**不发** `MemoryDump`。
#[test]
fn k5_an_out_of_range_dump_length_is_rejected_and_no_event_is_sent() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    for len in [0u32, 4097, 100_000] {
        let events = dispatch(&mut host, CommandPayload::DumpMemory { addr: 0, len });
        let (code, _) = first_error(&events);
        assert_eq!(code, "E_DBG-03", "len = {len} 必须被拒");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EventPayload::MemoryDump { .. })),
            "被拒的转储**不**发 `MemoryDump`：{events:?}"
        );
    }
}

/// **K-6**：读一段 ⇒ `bytes.len() == len` 且**逐字节**等于期望。
#[test]
fn k6_a_dump_returns_the_exact_bytes() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 先写一段已知字节进去（调试器写内存走的是**普通**总线写）。
    dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: 0x200,
            size: WriteSize::Word,
            value: 0x1122_3344,
        },
    );
    let events = dispatch(
        &mut host,
        CommandPayload::DumpMemory {
            addr: 0x200,
            len: 4,
        },
    );
    let (addr, bytes) = first_memory_dump(&events);
    assert_eq!(addr, 0x200, "回显起始地址");
    assert_eq!(bytes.len(), 4, "长度必须等于请求的 `len`");
    // RV32 是**小端** ⇒ ⇒ `0x11223344` 的四个字节是 44 33 22 11。
    assert_eq!(bytes, vec![0x44, 0x33, 0x22, 0x11], "逐字节必须正确");
}

/// **K-7**：转储**不计入**总线统计 ⇒ ⇒ **证明**走的是 `read_only`（裁决 E）。
#[test]
fn k7_a_dump_does_not_count_as_a_bus_access() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let before = host.backend.bus().stats().reads;
    let events = dispatch(
        &mut host,
        CommandPayload::DumpMemory {
            addr: 0x200,
            len: 16,
        },
    );
    assert_eq!(
        host.backend.bus().stats().reads,
        before,
        "调试转储不得计入总线统计"
    );
    // ⚠️ **前提断言**：转储**真的**读到了字节（否则上面那条是「空转绿」）。
    let (_, bytes) = first_memory_dump(&events);
    assert_eq!(bytes.len(), 16, "转储必须真的读到 16 个字节");
}

/// **K-8**：转储**不**触发读观察点 ⇒ ⇒ 下一条 guest 指令不得误报命中。
#[test]
fn k8_a_dump_does_not_fire_a_read_watchpoint() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetWatchpoint(riscv_sim_contract::model::Watchpoint {
            id: 1,
            addr: DEMO_STORE_ADDR,
            size: WriteSize::Word,
            access: riscv_sim_contract::model::WatchAccess::Read,
        }),
    );
    dispatch(
        &mut host,
        CommandPayload::DumpMemory {
            addr: DEMO_STORE_ADDR,
            len: 4,
        },
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    assert!(
        !events.iter().any(|e| matches!(
            e,
            EventPayload::Halted {
                reason: HaltReason::Watchpoint,
                ..
            }
        )),
        "调试转储不得让下一条 guest 指令误报观察点：{events:?}"
    );
}

/// **未映射 ⇒ 总线的错误原样透出**（不吞、不改写）。
#[test]
fn an_unmapped_dump_passes_the_bus_error_through_unchanged() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::DumpMemory {
            addr: 0x9000_0000,
            len: 4,
        },
    );
    let (code, message) = first_error(&events);
    assert_eq!(code, "E_BUS-01", "总线的错误码必须**原样**透出");
    assert!(
        message.contains("0x90000000"),
        "错误消息里要有那个地址：{message}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::MemoryDump { .. })),
        "失败的转储**不**发 `MemoryDump`"
    );
}

// ---------------------------------------------------------------------------
// K-11 ~ K-15 —— 条件断点
// ---------------------------------------------------------------------------

/// **K-11**：`SetBreakpoint` 带**非法条件** ⇒ `E_DBG-02` 且**断点表不变**。
#[test]
fn k11_an_illegal_condition_is_rejected_and_the_table_is_unchanged() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 先放一个**合法**的断点 ⇒ ⇒ 表里有一条。
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 1,
            addr: 0x20,
            source_hint: None,
            enabled: true,
            condition: None,
        }),
    );
    let before = host.backend.breakpoints().len();
    let events = dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 2,
            addr: 0x24,
            source_hint: None,
            enabled: true,
            condition: Some("foo".to_string()),
        }),
    );
    let (code, message) = first_error(&events);
    assert_eq!(code, "E_DBG-02");
    assert!(
        message.contains("foo"),
        "错误消息要指出「哪一段不对」：{message}"
    );
    assert_eq!(
        host.backend.breakpoints().len(),
        before,
        "被拒的注册**不得**改变断点表"
    );
}

/// **K-12**：条件为**假** ⇒ 跑到断点地址**不停**（继续执行）。
#[test]
fn k12_a_false_condition_does_not_stop_at_the_breakpoint_address() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // x1 在 demo 固件里被写成 42（`addi x1, x0, 42`）⇒ ⇒ 条件 `x1 == 5`
    // 在断点地址处是**假**的。
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 1,
            addr: DEMO_STORE_INSN_ADDR,
            source_hint: None,
            enabled: true,
            condition: Some("x1 == 5".to_string()),
        }),
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 12 });
    assert!(
        !events.iter().any(|e| matches!(
            e,
            EventPayload::Halted {
                reason: HaltReason::Breakpoint,
                ..
            }
        )),
        "条件为假 ⇒ ⇒ 不得停在断点地址：{events:?}"
    );
    // ⚠️ **PC 最终越过**那个地址（K-12 的后半句）。
    let pc = host.backend.engine().pc();
    assert!(
        pc > DEMO_STORE_INSN_ADDR,
        "PC 必须越过断点地址（实际 {pc:#x}）"
    );
    // ⚠️ **前提断言**：那条指令**真的**跑了（否则「不停」是空转绿）。
    let stored = host
        .backend
        .read_memory(DEMO_STORE_ADDR, WriteSize::Word)
        .expect("read");
    assert_eq!(
        stored, DEMO_STORE_VALUE,
        "条件为假 ⇒ ⇒ 断点处那条指令**执行了**"
    );
}

/// **K-13**：条件为**真** ⇒ 停，`HaltReason::Breakpoint`，**PC == 断点地址**，
/// 且该指令**未执行**。
#[test]
fn k13_a_true_condition_stops_at_the_breakpoint_address() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 条件用 `pc` ⇒ ⇒ 它在**取指之前**就是断点地址 ⇒ ⇒ 恒真。
    //
    // ⚠️ 断点打在 `sw x3, 256(x0)`（0x0c）**之前** ⇒ ⇒ 停住时那次写
    //    **没有发生** ⇒ ⇒ `0x100` 处**必须还是 0** ⇒ ⇒ 这就是「该指令
    //    **未执行**」的**可观测**证据。
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 1,
            addr: DEMO_STORE_INSN_ADDR,
            source_hint: None,
            enabled: true,
            condition: Some("pc == 0x0c".to_string()),
        }),
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 12 });
    let (reason, pc) = first_halted(&events);
    assert_eq!(reason, HaltReason::Breakpoint);
    assert_eq!(pc, DEMO_STORE_INSN_ADDR, "PC 停在断点地址");
    // ⚠️ **该指令未执行** ⇒ ⇒ 那次写**没发生**。
    let value = host
        .backend
        .read_memory(DEMO_STORE_ADDR, WriteSize::Word)
        .expect("read");
    assert_eq!(value, 0, "命中的那条指令**未执行**");
}

/// **K-13 的另一半**：条件看到的是**该指令执行前**的值。
#[test]
fn k13_the_condition_sees_the_values_from_before_the_instruction() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // demo 固件：0x00 = `addi x1, x0, 42`；0x04 = `addi x2, x0, 7`。
    // ⇒ ⇒ 断点打在 0x04 上，条件 `x1 == 42`：
    //   - 若条件看到的是**执行前**的值 ⇒ x1 = 42（0x00 已经跑过了）⇒ 真；
    //   - 若看到的是**执行后**的值 ⇒ x1 仍然是 42（这条写的是 x2）⇒ 也真。
    // ⇒ ⇒ 换成条件 `x2 == 7`：
    //   - 执行**前** ⇒ x2 = 0 ⇒ 假 ⇒ 不停；
    //   - 执行**后** ⇒ x2 = 7 ⇒ 真 ⇒ 停。
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 1,
            addr: 0x04,
            source_hint: None,
            enabled: true,
            condition: Some("x2 == 7".to_string()),
        }),
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 12 });
    assert!(
        !events.iter().any(|e| matches!(
            e,
            EventPayload::Halted {
                reason: HaltReason::Breakpoint,
                ..
            }
        )),
        "条件看到的是**执行前**的值 ⇒ x2 还是 0 ⇒ 条件为假 ⇒ 不该停：{events:?}"
    );
}

/// **K-14**：**无条件断点**（`condition == None`）⇒ 行为与既有**逐字相同**。
#[test]
fn k14_an_unconditional_breakpoint_stops_exactly_as_before() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 1,
            addr: DEMO_STORE_INSN_ADDR,
            source_hint: None,
            enabled: true,
            condition: None,
        }),
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 12 });
    let (reason, pc) = first_halted(&events);
    assert_eq!(reason, HaltReason::Breakpoint);
    assert_eq!(pc, DEMO_STORE_INSN_ADDR, "无条件断点停的位置一字不变");
    // ⚠️ **前提断言**：无条件断点**真的**挡住了那条指令。
    let value = host
        .backend
        .read_memory(DEMO_STORE_ADDR, WriteSize::Word)
        .expect("read");
    assert_eq!(value, 0, "无条件断点的语义一字不变");
}

/// **K-15**：条件断点在**时间驱动路径**（`Continue`）上**同样**生效。
#[test]
fn k15_a_conditional_breakpoint_also_works_on_the_time_driven_path() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 条件恒真（PC 就是断点地址）⇒ ⇒ 时间驱动路径**必须**停下来。
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 1,
            addr: DEMO_STORE_INSN_ADDR,
            source_hint: None,
            enabled: true,
            condition: Some("pc == 0x0c".to_string()),
        }),
    );
    let events = dispatch(&mut host, CommandPayload::Continue);
    let (reason, pc) = first_halted(&events);
    assert_eq!(
        reason,
        HaltReason::Breakpoint,
        "时间驱动路径上条件断点**同样**要生效：{events:?}"
    );
    assert_eq!(pc, DEMO_STORE_INSN_ADDR);
}

/// **K-15 的另一半**：时间驱动路径上，条件为**假** ⇒ ⇒ **不停**。
#[test]
fn k15_a_false_condition_does_not_stop_on_the_time_driven_path() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(Breakpoint {
            id: 1,
            addr: DEMO_STORE_INSN_ADDR,
            source_hint: None,
            enabled: true,
            condition: Some("x1 == 5".to_string()),
        }),
    );
    let events = dispatch(&mut host, CommandPayload::Continue);
    assert!(
        !events.iter().any(|e| matches!(
            e,
            EventPayload::Halted {
                reason: HaltReason::Breakpoint,
                ..
            }
        )),
        "条件为假 ⇒ ⇒ 时间驱动路径上也**不该**停：{events:?}"
    );
    // ⚠️ **判别力**：如果时间驱动路径**不判**条件，它会**停**在断点地址
    //    ⇒ ⇒ 那次写**不会发生** ⇒ ⇒ 现在写**发生了** ⇒ ⇒ 证明条件**真的**
    //    被判了。
    let stored = host
        .backend
        .read_memory(DEMO_STORE_ADDR, WriteSize::Word)
        .expect("read");
    assert_eq!(
        stored, DEMO_STORE_VALUE,
        "时间驱动路径上条件为假 ⇒ ⇒ 断点处那条指令**执行了**"
    );
}
