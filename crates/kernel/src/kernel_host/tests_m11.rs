//! Lane D —— **契约 v3 调试面（M-11）**的宿主接线用例。
//!
//! 冻结规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`
//! （§3.4 的命令 → 事件映射表 · §5.2 的验收 #10~#18 · §9 的接线细则）。
//!
//! ## 这些用例在防什么
//!
//! | 用例 | 防的是 |
//! | --- | --- |
//! | `disasm_command_produces_exactly_one_disasm_event` | 占位分支没被替换（还回 `E_CTX-03`） |
//! | `the_fetch_reads_exactly_two_bytes_…` | 取指回调「读一个字取低半」⇒ 映射区末端的压缩指令被误报成未映射（LLD 11-1） |
//! | `the_two_no_answer_states_are_distinguishable` | 「没有 DWARF」与「有 DWARF 但未命中」被混成一个（LLD 11-4） |
//! | `waveform_export_reads_the_m19_ring_…` | **D-50**：波形导出的数据源退回了 M-18 的边沿存储 |
//! | `the_m18_trace_ring_is_drained_into_the_m11_ring` | **D-51**：M-18 的环还是「只写不读」 |
//! | `illegal_export_combinations_are_rejected` | 非法组合被「换一个格式凑合」 |
//! | `a_write_invalidates_only_the_written_address` | 裁决 G：缓存被**整表清空**（而不是只失效一个地址） |
//! | `loading_a_new_project_rebuilds_the_m11_indexes` | 索引被缓存 ⇒ 新工程还查得到旧工程的符号 |

use super::*;

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, LineEntry, ProductTree, Project, ProjectVersion, TraceFormat,
    TraceSource,
};
use riscv_sim_contract::protocol::CommandPayload;
use riscv_sim_contract::session::{CheckpointMetaDto, SESSION_SNAPSHOT_SCHEMA_VERSION};

use crate::m11_symbol::disasm::E_CTX_02_DISASM_COUNT_EXCEEDED;
use crate::m11_symbol::MAX_DISASM_COUNT;

/// demo 固件 —— **引用共享常量**，不抄一份。
use super::DEMO_FIRMWARE_BYTES as DEMO_BYTES;

/// 低段 RAM 的**末端** —— **取值来源 = 平台描述**（v0.27 · D-123-B）。
///
/// ⚠️ 「压缩指令位于映射区**末端**」的用例要靠它：那里的最后 2 个字节
/// 之后就是未映射地址 ⇒ 取指**必须**只读 2 字节。
///
/// ⚠️ 末端 = 描述里那段**低段 RAM** 的 `base + size` ⇒ 不再抄一份宿主
/// 常量（宿主常量与描述是**同源**的 ⇒ 抄一份就多一个真相）。
fn ram_low_end() -> u32 {
    let description =
        platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置描述 default");
    let low = platform::regions_by_base(&description)
        .into_iter()
        .find_map(|region| match region {
            riscv_sim_contract::platform::RegionSpec::Ram { base, size } if *base == 0 => {
                Some(base + size)
            }
            _ => None,
        })
        .expect("default 描述里有一段 base 为 0 的低段 RAM");
    low
}

/// demo 固件的三条指令的机器码。
const DEMO_CODE_0: u32 = 0x02A0_0093; // addi x1, x0, 42
const DEMO_CODE_1: u32 = 0x0070_0113; // addi x2, x0, 7
const DEMO_CODE_2: u32 = 0x0020_81B3; // add x3, x1, x2

/// `c.addi sp, -16` 的 16 位原码。
const COMPRESSED_CODE: u32 = 0x1141;

/// 造一个挂着 demo 固件的工程（裸镜像 ⇒ 没有调试节区）。
fn demo_project() -> Project {
    Project {
        id: "proj-m11".to_string(),
        name: "m11".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(DEMO_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// DWARF 真产物的节区集合（`.debug_line` + `.debug_line_str`）。
///
/// ⚠️ 字节来自 **真工具链产物**（`riscv-none-elf-gcc` 15.2.0 编的真实
/// RISC-V OS，完全链接）⇒ 仓库自包含，测试**不依赖**外部工具链。
fn dwarf_sections() -> Vec<(String, Vec<u8>)> {
    use crate::m11_symbol::dwarf::fixtures::{OSZ_DL, OSZ_DLS};
    vec![
        (
            crate::m11_symbol::dwarf::DEBUG_LINE_SECTION.to_string(),
            OSZ_DL.to_vec(),
        ),
        (
            crate::m11_symbol::dwarf::DEBUG_LINE_STR_SECTION.to_string(),
            OSZ_DLS.to_vec(),
        ),
    ]
}

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 取第一条 `Disasm` 事件里的条目。
fn disasm_entries(events: &[EventPayload]) -> &[riscv_sim_contract::model::DisasmEntryDto] {
    for e in events {
        if let EventPayload::Disasm { entries } = e {
            return entries;
        }
    }
    panic!("没有 Disasm 事件：{events:?}");
}

/// 取第一条 `TraceExport` 事件的各个字段。
#[allow(clippy::type_complexity)]
fn trace_export(
    events: &[EventPayload],
) -> (TraceSource, TraceFormat, u64, u64, u64, String, bool) {
    for e in events {
        if let EventPayload::TraceExport {
            source,
            format,
            offset,
            next_offset,
            total,
            chunk,
            overflow_marked,
            ..
        } = e
        {
            return (
                *source,
                *format,
                *offset,
                *next_offset,
                *total,
                chunk.clone(),
                *overflow_marked,
            );
        }
    }
    panic!("没有 TraceExport 事件：{events:?}");
}

/// 取第一条 `Error` 事件的错误码。
fn error_code(events: &[EventPayload]) -> String {
    for e in events {
        if let EventPayload::Error { code, .. } = e {
            return code.clone();
        }
    }
    panic!("没有 Error 事件：{events:?}");
}

// ---------------------------------------------------------------------------
// 1. `Disasm` 命令 → 事件（验收 #10）
// ---------------------------------------------------------------------------

#[test]
fn disasm_command_produces_exactly_one_disasm_event() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 3 });
    assert_eq!(events.len(), 1, "恰好一条事件：{events:?}");
    let entries = disasm_entries(&events);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].addr, 0);
    assert_eq!(entries[0].code, DEMO_CODE_0);
    assert_eq!(entries[0].mnemonic, "addi ra,zero,42");
    assert_eq!(entries[0].width, 4);
    assert_eq!(entries[1].addr, 4);
    assert_eq!(entries[1].code, DEMO_CODE_1);
    assert_eq!(entries[2].addr, 8);
    assert_eq!(entries[2].code, DEMO_CODE_2);
}

#[test]
fn disasm_of_an_unmapped_address_is_an_empty_sequence_not_an_error() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 0x2000_0000 在默认内存图里**没有**映射（低段 RAM 0x0、高段 RAM
    // 0x8000_0000、GPIO 0x4000_0000）。
    let events = dispatch(
        &mut host,
        CommandPayload::Disasm {
            addr: 0x2000_0000,
            count: 4,
        },
    );
    assert_eq!(events.len(), 1, "读不到是**截断**，不是错误：{events:?}");
    assert!(disasm_entries(&events).is_empty(), "未映射 ⇒ 空序列");
}

#[test]
fn disasm_count_above_the_kernel_limit_is_rejected() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    let events = dispatch(
        &mut host,
        CommandPayload::Disasm {
            addr: 0,
            count: MAX_DISASM_COUNT + 1,
        },
    );
    assert_eq!(events.len(), 1);
    // ⚠️ 错误码是 **E_CTX-02**（资源限制），不是 E_CTX-03（组合不合法）
    // ——「一场景一码」。
    assert_eq!(error_code(&events), "E_CTX-02");
    assert_eq!(E_CTX_02_DISASM_COUNT_EXCEEDED, 2);
}

#[test]
fn the_fetch_reads_exactly_two_bytes_so_a_compressed_tail_is_not_a_false_miss() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 把一条**压缩指令**写到低段 RAM 的**最后 2 个字节**——它的后面
    // 就是未映射地址。
    //
    // ⚠️ 这是「取指必须真的只读 2 字节」的判别式：若宿主「读一个字取
    // 低半」（4 字节），这 2 字节就会**跨界** ⇒ 被误报成「未映射」⇒
    // 序列变成空的（LLD 11-1）。
    let tail = ram_low_end() - 2;
    dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: tail,
            size: WriteSize::Half,
            value: COMPRESSED_CODE,
        },
    );

    let events = dispatch(
        &mut host,
        CommandPayload::Disasm {
            addr: tail,
            count: 4,
        },
    );
    let entries = disasm_entries(&events);
    assert_eq!(
        entries.len(),
        1,
        "映射区末端的压缩指令必须被解出来：{entries:?}"
    );
    assert_eq!(entries[0].addr, tail);
    assert_eq!(entries[0].code, COMPRESSED_CODE, "16 位原码");
    assert_eq!(entries[0].mnemonic, "c.addi sp,-16");
    assert_eq!(entries[0].width, 2);
}

// ---------------------------------------------------------------------------
// 2. `ResolveLine` 命令 → 事件（验收 #11）
// ---------------------------------------------------------------------------

#[test]
fn resolve_line_reports_the_degraded_state_when_there_is_no_dwarf() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    let events = dispatch(&mut host, CommandPayload::ResolveLine { addr: 0x1000 });
    assert_eq!(events.len(), 1, "恰好一条事件：{events:?}");
    let EventPayload::SourceLocation { addr, file, line } = &events[0] else {
        panic!("没有 SourceLocation 事件：{events:?}");
    };
    assert_eq!(*addr, 0x1000);
    assert!(file.is_empty(), "没有 DWARF ⇒ 文件名是空串");
    assert_eq!(*line, 0, "没有 DWARF ⇒ 行号是 0");
}

#[test]
fn resolve_line_returns_the_real_file_and_line_of_a_dwarf_image() {
    // ⚠️ 验收 A-14：宿主 `ResolveLine` 命令返回**真的** `file` + `line`
    // （契约形状**不变**：`SourceLocation { addr, file, line }`）。
    //
    // 夹具 = **真工具链产物**（`riscv-none-elf-gcc` 15.2.0 编的真实
    // RISC-V OS，三个编译单元，DWARF 5）。
    let mut host = KernelHost::new();
    host.rebuild_symbol_index(&dwarf_sections())
        .expect("索引重建不得报错");

    assert_eq!(
        host.dwarf_status(),
        crate::m11_symbol::DwarfStatus::Supported(5),
        "真产物的 DWARF 5 必须被**真的**解析"
    );

    // 命中 ⇒ 真的文件名 + 真的行号（真值来自 `addr2line` 与
    // `readelf --debug-dump=decodedline` 两个**独立** oracle）。
    let events = dispatch(&mut host, CommandPayload::ResolveLine { addr: 0x8000_0000 });
    assert_eq!(events.len(), 1, "恰好一条事件：{events:?}");
    let EventPayload::SourceLocation { addr, file, line } = &events[0] else {
        panic!("没有 SourceLocation 事件：{events:?}");
    };
    assert_eq!(*addr, 0x8000_0000);
    assert_eq!(
        file, "/tmp/osz-probe/kernel/base/hook/src/hook.c",
        "文件名必须来自 `.debug_line_str`（真实路径）"
    );
    assert_eq!(*line, 8, "行号必须是真的");

    // 另一个编译单元的地址 ⇒ 也能查到（多单元归并）。
    let events = dispatch(&mut host, CommandPayload::ResolveLine { addr: 0x8000_0060 });
    let EventPayload::SourceLocation { file, line, .. } = &events[0] else {
        panic!("没有 SourceLocation 事件：{events:?}");
    };
    assert_eq!(file, "/tmp/osz-probe/kernel/base/init/src/init.c");
    assert_eq!(*line, 12);

    // ⚠️ 「查过行表、没命中」⇒ `line = u32::MAX`（**不是** 0 —— 0 是
    // 「没有 DWARF」）。两态**不得混**（STAGE-7 裁决 J）。
    let events = dispatch(&mut host, CommandPayload::ResolveLine { addr: 0x1234_5678 });
    let EventPayload::SourceLocation { file, line, .. } = &events[0] else {
        panic!("没有 SourceLocation 事件：{events:?}");
    };
    assert!(file.is_empty());
    assert_eq!(
        *line,
        u32::MAX,
        "有 DWARF 但地址没命中 ⇒ `u32::MAX`（与「没有 DWARF」的 0 不同）"
    );
}

#[test]
fn the_two_no_answer_states_are_distinguishable() {
    // ⚠️ Wave 1 的 `DwarfLines::resolve` **恒**返回降级态 ⇒ 「有 DWARF
    // 但未命中」这一态在生产路径上**走不到**。⇒ 这里直接喂三种判定，
    // 把**映射点**钉住（否则「两态可区分」这条验收就没有判别力）。
    let no_dwarf = source_location_event(0x1000, LineLookup::NoDwarf);
    let missed = source_location_event(0x1000, LineLookup::Missed);
    let found = source_location_event(
        0x1000,
        LineLookup::Found(LineEntry {
            addr: 0x1000,
            file: "main.c".to_string(),
            line: 7,
        }),
    );

    let EventPayload::SourceLocation { file, line, .. } = no_dwarf else {
        panic!();
    };
    assert!(file.is_empty());
    assert_eq!(line, 0, "没有 DWARF ⇒ 0");

    let EventPayload::SourceLocation { file, line, .. } = missed else {
        panic!();
    };
    assert!(file.is_empty());
    assert_eq!(line, u32::MAX, "有 DWARF 但未命中 ⇒ u32::MAX");

    let EventPayload::SourceLocation { file, line, .. } = found else {
        panic!();
    };
    assert_eq!(file, "main.c");
    assert_eq!(line, 7);
}

// ---------------------------------------------------------------------------
// 3. `ExportTrace` —— 两条路由（验收 #12 / #13 / #18）
// ---------------------------------------------------------------------------

#[test]
fn waveform_export_reads_the_m19_ring_and_not_the_m18_edge_store() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 跑到 demo 固件里那条写 GPIO 的指令（第 8 条）⇒ 产生一条边沿。
    dispatch(&mut host, CommandPayload::StepN { n: 8 });

    // ⚠️ **先把 M-18 的边沿存储抽干**（订阅会推一次波形历史）。
    // 这一步之后：M-18 的边沿存储是**空的**，M-19 的环里**还有**边沿。
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventPayload::WaveformHistory(_))),
        "订阅必须推一次波形历史：{events:?}"
    );
    assert_eq!(
        host.probe_edge_count(),
        0,
        "M-18 的边沿存储已被抽干 ⇒ 之后的导出**只能**来自 M-19 的环"
    );

    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Waveform,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    assert_eq!(events.len(), 1);
    let (source, format, offset, next_offset, total, chunk, _overflow) = trace_export(&events);
    assert_eq!(source, TraceSource::Waveform, "回显数据源");
    assert_eq!(format, TraceFormat::Csv, "回显格式");
    assert_eq!(offset, 0);
    assert_eq!(total, 1, "M-19 的环里有一条边沿");
    assert_eq!(next_offset, 1);
    assert!(
        chunk.contains("GPIO0.OUT"),
        "导出内容必须来自 M-19 的环：{chunk}"
    );
}

#[test]
fn the_m19_ring_has_a_reader() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 8 });

    let buffered = host
        .waveform_pipeline()
        .expect("M-19 的管道")
        .pipeline()
        .stats()
        .buffered;
    assert!(buffered > 0, "M-19 的环里必须有边沿（D-50 的前提）");

    // 导出 = **读**那个环 ⇒ 环有读者了。
    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Waveform,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    let (_source, _format, _offset, _next, total, chunk, _overflow) = trace_export(&events);
    assert_eq!(total as usize, buffered, "导出读的就是环里的那些条目");
    assert!(!chunk.is_empty());
}

#[test]
fn execution_export_reads_the_m11_trace_ring() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 三条指令 ⇒ 三次取指（**没有**数据访存）。
    dispatch(&mut host, CommandPayload::StepN { n: 3 });

    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Execution,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    assert_eq!(events.len(), 1);
    let (source, format, _offset, next_offset, total, chunk, overflow) = trace_export(&events);
    assert_eq!(source, TraceSource::Execution);
    assert_eq!(format, TraceFormat::Csv);
    assert_eq!(total, host.trace_len() as u64, "total == 环内条目数");
    assert_eq!(next_offset, total);
    assert!(!overflow, "没溢出 ⇒ overflow_marked 是 false");
    assert!(chunk.starts_with("time_ps,addr,kind,data,width"), "{chunk}");
    // 三条**取指** ⇒ kind = instruction（`is_fetch` 的映射）。
    assert_eq!(chunk.matches(",instruction,").count(), 3, "{chunk}");
}

#[test]
fn illegal_export_combinations_are_rejected() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 波形 + JSON：M-19 没有 JSON 波形导出。
    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Waveform,
            format: TraceFormat::Json,
            offset: 0,
            limit: 0,
        },
    );
    assert_eq!(events.len(), 1);
    assert_eq!(error_code(&events), "E_CTX-03", "非法组合 ⇒ E_CTX-03");

    // 执行轨迹 + VCD：执行轨迹不是信号波形。
    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Execution,
            format: TraceFormat::Vcd,
            offset: 0,
            limit: 0,
        },
    );
    assert_eq!(events.len(), 1);
    assert_eq!(error_code(&events), "E_CTX-03", "非法组合 ⇒ E_CTX-03");
}

#[test]
fn the_continuation_protocol_visits_every_entry_exactly_once() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 4 });

    let total = host.trace_len() as u64;
    assert!(total >= 4, "至少四条取指：{total}");

    let mut offset = 0u64;
    let mut chunks = 0u64;
    let mut rows = 0u64;
    let mut header_seen = false;
    loop {
        let events = dispatch(
            &mut host,
            CommandPayload::ExportTrace {
                source: TraceSource::Execution,
                format: TraceFormat::Csv,
                offset,
                limit: 1,
            },
        );
        let (_s, _f, echoed, next_offset, t, chunk, _o) = trace_export(&events);
        assert_eq!(echoed, offset, "回显本块起点");
        assert_eq!(t, total);
        chunks += 1;
        // 表头**只**出现在第一块 ⇒ 各块拼起来是一份 CSV。
        if chunk.starts_with("time_ps,") {
            assert!(!header_seen, "表头只能出现一次");
            header_seen = true;
        } else {
            assert!(header_seen, "后续块不该带表头");
        }
        rows += chunk.lines().filter(|l| !l.starts_with("time_ps,")).count() as u64;
        offset = next_offset;
        if offset >= t {
            break;
        }
        assert!(chunks <= total + 2, "续导必须推进，不能打转");
    }
    assert_eq!(offset, total, "续导必须正好走到末尾");
    assert_eq!(rows, total, "累计条目数必须 == total（不重不漏）");
    assert_eq!(chunks, total, "每块一条 ⇒ 块数 == 条目数");
}

#[test]
fn the_debugger_disassembly_does_not_show_up_as_guest_traffic() {
    // ⚠️ 调试器的读**不是**访客的访存（M-14 的 `read_only` 语义）：它不能
    // 进 M-18 的访存 trace 环，也不能进执行轨迹环 ⇒ 导出里的「访存流水」
    // 只讲访客的事。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 3 });
    assert_eq!(disasm_entries(&events).len(), 3);
    assert_eq!(
        host.probe_trace_len(),
        0,
        "反汇编的取指**不是**总线访问 ⇒ M-18 的环里不该有它"
    );
    assert_eq!(host.trace_len(), 0, "执行轨迹环里也不该有它");

    // 访客真的跑起来之后，环里只有**访客**的访存。
    dispatch(&mut host, CommandPayload::StepN { n: 3 });
    assert_eq!(host.trace_len(), 3, "只有三次访客取指");
}

// ---------------------------------------------------------------------------
// 4. D-51 / D-50 的闭环（验收 #17 / #18）
// ---------------------------------------------------------------------------
#[test]
fn the_m18_trace_ring_is_drained_into_the_m11_ring() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // ⚠️ 镜像装载本身会写总线（`load_bytes` 逐字节写）⇒ 那些写**不是**
    // 访客的执行历史 ⇒ **不能**进执行轨迹环，但必须**可见地**被丢弃。
    assert_eq!(host.trace_len(), 0, "装载的写不进执行轨迹环");
    assert!(
        host.trace_events_discarded() > 0,
        "装载的写必须被**可见地**丢弃"
    );
    assert_eq!(host.probe_trace_len(), 0, "M-18 的环在装载后也是空的");

    dispatch(&mut host, CommandPayload::StepN { n: 3 });

    assert_eq!(
        host.probe_trace_len(),
        0,
        "跑完一批 ⇒ M-18 的环必须被转移走（D-51：它不再是「只写不读」）"
    );
    assert_eq!(
        host.trace_len(),
        3,
        "M-11 的环拿到了访客的执行历史（3 次取指）"
    );
}

// ---------------------------------------------------------------------------
// 5. trace 开关（验收 #15）
// ---------------------------------------------------------------------------

#[test]
fn the_trace_switch_stops_and_resumes_recording() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 关闭。
    let events = dispatch(
        &mut host,
        CommandPayload::SetTraceEnabled { enabled: false },
    );
    assert_eq!(events.len(), 1);
    let EventPayload::TraceState {
        enabled,
        len,
        dropped,
        ..
    } = &events[0]
    else {
        panic!("没有 TraceState 事件：{events:?}");
    };
    assert!(!*enabled, "回显开关状态");
    assert_eq!(*len, 0);
    assert_eq!(*dropped, 0);

    let before = host.trace_len();
    dispatch(&mut host, CommandPayload::StepN { n: 3 });
    assert_eq!(host.trace_len(), before, "关闭 ⇒ **零记录**");
    assert_eq!(
        host.probe_trace_len(),
        0,
        "关闭期间宿主**照常 drain**（M-18 的环是有界的，不 drain 会涨到上限）"
    );
    assert!(
        host.trace_events_discarded() > 0,
        "关闭期间的事件被**可见地**丢弃，不静默"
    );

    // 打开：把关闭期间攒的那批**丢掉**（不倒回来）。
    let events = dispatch(&mut host, CommandPayload::SetTraceEnabled { enabled: true });
    let EventPayload::TraceState { enabled, .. } = &events[0] else {
        panic!();
    };
    assert!(*enabled);
    assert_eq!(
        host.trace_len(),
        before,
        "重新打开**不**把关闭期间攒的那批倒回来"
    );

    dispatch(&mut host, CommandPayload::StepN { n: 3 });
    assert!(host.trace_len() > before, "开启 ⇒ 开始记录");
}

#[test]
fn overflow_marked_is_set_when_the_ring_dropped_entries() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 把轨迹环换成一个**小**的 ⇒ 跑几条指令就会溢出。
    host.trace = TraceBuffer::new(2);
    dispatch(&mut host, CommandPayload::StepN { n: 5 });
    assert!(host.trace_dropped() > 0, "小环必须溢出");

    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Execution,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    let (_s, _f, _o, _n, _t, _chunk, overflow) = trace_export(&events);
    assert!(overflow, "丢过条目的导出必须标成「窗口不完整」");
}

// ---------------------------------------------------------------------------
// 6. 自修改代码的缓存失效（验收 #5 / 裁决 G）
// ---------------------------------------------------------------------------

#[test]
fn a_write_invalidates_only_the_written_address() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 先反汇编三条 ⇒ 三个地址进缓存。
    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 3 });
    assert_eq!(disasm_entries(&events).len(), 3);
    let cached = host.disasm_cache_len();
    assert_eq!(cached, 3, "三条指令都该进缓存");

    // 写**第一个**地址（自修改代码）。
    dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: 0,
            size: WriteSize::Word,
            value: 0x02B0_0093,
        },
    );

    assert_eq!(
        host.disasm_cache_len(),
        cached - 1,
        "只失效**被写的那个地址**（不是整表清空 —— 清空的话这里是 0）"
    );

    // 再反汇编 ⇒ 第一条是新机器码，后两条**不变**。
    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 3 });
    let entries = disasm_entries(&events);
    assert_eq!(entries[0].code, 0x02B0_0093, "被写的地址必须重新取指");
    assert_eq!(entries[1].code, DEMO_CODE_1, "其它地址仍然命中缓存");
    assert_eq!(entries[2].code, DEMO_CODE_2, "其它地址仍然命中缓存");
}

// ---------------------------------------------------------------------------
// 7. 裁决 G 与裁决 H 的**解耦**（缓存失效 ≠ 记录开关）
// ---------------------------------------------------------------------------

#[test]
fn the_trace_switch_does_not_disable_the_cache_invalidation() {
    // ⚠️ **裁决 G（缓存失效）与裁决 H（记录开关）必须解耦**：
    // 缓存失效是**正确性**（「访客改了代码，反汇编必须跟着变」），记录开关
    // 是**性能**（「要不要把访存记进环里」）。
    //
    // 耦合 ⇒ 关闭 trace 期间访客自改代码 ⇒ 缓存**陈旧** ⇒ 用户看到的
    // 反汇编是**旧的** ⇒ 调试器给出**错误**的指令。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 先反汇编 ⇒ 地址 0 进缓存。
    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 1 });
    assert_eq!(disasm_entries(&events).len(), 1);
    let cached = host.disasm_cache_len();
    assert_eq!(cached, 1, "地址 0 已进缓存");

    // **关闭** trace。
    dispatch(
        &mut host,
        CommandPayload::SetTraceEnabled { enabled: false },
    );

    // 访客（此处用调试器的写命令触发同一条总线写通道）改了地址 0。
    dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: 0,
            size: WriteSize::Word,
            value: 0x02B0_0093,
        },
    );

    assert_eq!(
        host.disasm_cache_len(),
        cached - 1,
        "关闭 trace 期间访客写内存 ⇒ 缓存**照样**失效（裁决 G 的失效通道不受记录开关影响）"
    );

    // 重新打开 + 重新反汇编 ⇒ **新**机器码。
    dispatch(&mut host, CommandPayload::SetTraceEnabled { enabled: true });
    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 1 });
    let entries = disasm_entries(&events);
    assert_eq!(
        entries[0].code, 0x02B0_0093,
        "必须重新取指，不能给陈旧的机器码"
    );
}

// ---------------------------------------------------------------------------
// 8. 访存轨迹的**时间轴**（M-17 ↔ M-14 的时间戳接线）
// ---------------------------------------------------------------------------

#[test]
fn the_execution_trace_carries_real_timestamps() {
    // ⚠️ **时间轴必须真的动**：M-14 的 `BusAccess::ts_ps` 来自总线的
    // `last_ts_ps`，它只由 `Bus::set_now_ps` 更新 ⇒ 没有 M-17 的那次接线，
    // 导出里的时间列**全是 0**（数据看着有，时间却是平的）。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 3 });

    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Execution,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    let (_s, _f, _o, _n, _t, chunk, _ov) = trace_export(&events);

    let rows: Vec<&str> = chunk
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(rows.len(), 3, "三次取指 ⇒ 三行：{chunk}");

    // 时间列是**带引号的十进制字符串**（契约的虚拟时间口径）。
    let stamps: Vec<u64> = rows
        .iter()
        .map(|row| {
            let cell = row.split(',').next().expect("时间列");
            cell.trim_matches('"')
                .parse()
                .unwrap_or_else(|e| panic!("时间列不是十进制数：{row}（{e}）"))
        })
        .collect();
    // ⚠️ 第一条指令跑在虚拟时刻 **0**（时钟从 0 起）⇒ 第一个戳是 0；
    // **后面**的必须**递增** ⇒ 恒 0（没有接线时的样子）会在这里变红。
    assert_eq!(stamps[0], 0, "第一条指令跑在虚拟时刻 0");
    assert!(
        stamps[0] < stamps[1] && stamps[1] < stamps[2],
        "时间戳必须随指令递增：{stamps:?}"
    );
    assert!(stamps[2] > 0, "时间轴不能是平的：{stamps:?}");
}

// ---------------------------------------------------------------------------
// 9. 缓存**兜住**访存轨迹那次重复取指（规格 §9.4）
// ---------------------------------------------------------------------------

#[test]
fn the_disasm_cache_absorbs_the_repeated_fetch() {
    // 规格 §9.4 的原话：「M-18 的 `TraceEvent` **不带数据** ⇒ 指令条目需要
    // 宿主**额外取一次指**……但 M-11 的 `disasmCache` 可以兜住它。」
    //
    // ⚠️ 那句话**必须可观测** ⇒ 用 `lookup_hits` 计数钉住它。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 先反汇编 ⇒ 三个地址进缓存。
    dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 3 });
    assert_eq!(host.disasm_cache_len(), 3);
    let hits_before = host.disasm_cache_hits();

    // 跑三条指令 ⇒ 三次取指事件要机器码 ⇒ 应当**命中缓存**（少三次总线读）。
    dispatch(&mut host, CommandPayload::StepN { n: 3 });

    assert_eq!(
        host.disasm_cache_hits() - hits_before,
        3,
        "访存轨迹的取指**必须**复用反汇编缓存（规格 §9.4）"
    );
}

// ---------------------------------------------------------------------------
// 10. 装载工程 ⇒ 重建 M-11 的索引（验收 #8 / STAGE-5 §9.2）
// ---------------------------------------------------------------------------

#[test]
fn loading_a_new_project_rebuilds_the_m11_indexes() {
    let mut host = KernelHost::new();

    // 第一个工程：带一个符号表（符号 `alpha`）。
    let (symtab, strtab) = elf_symbol_table(&["alpha"]);
    dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(&[(".symtab", symtab), (".strtab", strtab)])),
    );
    assert_eq!(host.symbol_count(), 1, "第一个工程的符号必须被解析");
    let alpha = host.lookup_symbol("alpha").expect("alpha 必须查得到");
    assert_eq!(alpha.addr, Some(0x1000));

    // 第二个工程：符号表里**没有** `alpha`。
    let (symtab, strtab) = elf_symbol_table(&["beta"]);
    dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(&[(".symtab", symtab), (".strtab", strtab)])),
    );
    assert_eq!(host.symbol_count(), 1);
    assert!(
        host.lookup_symbol("alpha").is_none(),
        "装载新工程 ⇒ 旧工程的符号必须**查不到**（索引被重建了）"
    );
    assert!(
        host.lookup_symbol("beta").is_some(),
        "新工程的符号必须查得到"
    );
}

#[test]
fn a_project_without_debug_sections_still_loads() {
    let mut host = KernelHost::new();
    // 裸机镜像（没有节区表）⇒ 空的符号表 + 降级的 DWARF。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(host.symbol_count(), 0);
    assert_eq!(
        host.dwarf_status(),
        crate::m11_symbol::DwarfStatus::Absent,
        "装载过了，但这个镜像没有 DWARF"
    );

    // 反汇编**照常**可用（验收 #3：无 DWARF 独立可用）。
    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 1 });
    assert_eq!(disasm_entries(&events).len(), 1);
}

// ---------------------------------------------------------------------------
// 11. 检视 A 的三条修复的判别式
// ---------------------------------------------------------------------------

/// 一条**自环**指令（`beq x0, x0, 0`）—— PC 永远回到自己 ⇒ 访客可以
/// 一直跑下去（不会跑完、不会撞到未映射地址）。
///
/// ⚠️ **「跳回自己」的 B 型偏移是 `0`，不是 `-4`**：分支目标是 `pc + imm`
/// （**字节**偏移），`imm = 0` ⇒ 目标就是**本条指令自己** ⇒ 真正的自环。
/// `imm = -4` 会跳到**上一条**指令（不在本条）。
///
/// ⚠️ **历史坑（本轮实测）**：这里原先手算成 `0xFE00_0FE3`，注释说是
/// `beq x0, x0, -4`，**实际解出来是 `beq x0, x0, -2`** ⇒ 目标 `pc − 2`
/// （未对齐 + 未映射）⇒ **第 2 条指令就陷入** ⇒ `StepN(250_000)` 实际只跑了
/// **2 条** ⇒ 访存环永远填不满 ⇒ 「访存环不许溢出」那条断言**恒成立**
/// ⇒ **该用例是假绿**（反向变异 `V-14` 存活当场暴露）。
/// ⇒ 现在改成 `beq x0, x0, 0`（`0x0000_0063`，B 型立即数全零 ⇒ 手算无歧义）。
const SELF_LOOP: u32 = 0x0000_0063;

/// 造一个跑着**自环**固件的工程。
fn loop_project() -> Project {
    Project {
        id: "proj-m11-loop".to_string(),
        name: "m11-loop".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "loop.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(SELF_LOOP.to_le_bytes().to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

#[test]
fn a_long_step_batch_keeps_the_invalidation_channel_lossless() {
    // ⚠️ **失效通道必须永远无损**（v0.20 检视 A · 重要 1）：
    // M-18 的访存环是**有界**的（`TRACE_CAP = 200_000`，溢出丢最旧）⇒ 它
    // 同时是**反汇编缓存的失效通道**（裁决 G：访存写事件 ⇒ 失效对应地址）
    // ⇒ 环一溢出，写事件就被丢掉 ⇒ 缓存条目不失效 ⇒ **反汇编给出旧机器码**
    // （调试器给出**错误**的指令）。
    //
    // ⇒ `StepN` 按指令预算**分块** ⇒ 单块事件数 ≪ 环的容量 ⇒ 环**不溢出**。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(loop_project()));

    // 25 万条指令 ⇒ 不分块的话，访存环（20 万条）**必然溢出**。
    dispatch(&mut host, CommandPayload::StepN { n: 250_000 });

    assert_eq!(
        host.probe_trace_dropped(),
        0,
        "访存环**不许溢出**（否则缓存的失效通道有损 ⇒ 反汇编给出旧机器码）"
    );
}

#[test]
fn restoring_a_session_invalidates_the_m11_session_scoped_state() {
    // ⚠️ **还原会话 = 新会话** ⇒ M-11 的**会话级**缓存与历史必须作废
    // （STAGE-5 §9.2 的同一条规矩：「新会话 ⇒ 旧的索引全部作废」）。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 先反汇编（缓存里有条目）+ 跑几条（轨迹环里有历史）。
    dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 3 });
    assert_eq!(host.disasm_cache_len(), 3, "三条指令都该进缓存");
    dispatch(&mut host, CommandPayload::StepN { n: 3 });
    assert!(host.trace_len() > 0, "轨迹环里该有历史");

    // 采集 + 还原。
    let payload = host.capture_session(CheckpointMetaDto {
        id: 1,
        t_virt: 0,
        t_host: 0,
        base_seq: 0,
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
    });
    host.restore_session(&payload).expect("还原必须成功");

    assert_eq!(
        host.disasm_cache_len(),
        0,
        "还原后反汇编缓存必须作废（否则 `Disasm` 给出**上个会话**的机器码）"
    );
    assert_eq!(
        host.trace_len(),
        0,
        "还原后轨迹环必须清空（否则两个会话的轨迹混在一起，而 `overflow_marked` 仍说「没溢出」）"
    );
}

#[test]
fn the_trace_instruction_code_matches_the_disassembly_entry() {
    // ⚠️ **取指口径只有一份实现**（M-11 的 `disasm::fetch_instruction_code`）：
    // 访存轨迹里的机器码必须与反汇编条目的机器码**逐字段相同**。
    // 压缩指令尤其要验：**16 位原码**，不是展开后的 32 位形式。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 把一条**压缩指令**写到地址 0 ⇒ 访客第一条取指就是它。
    dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: 0,
            size: WriteSize::Half,
            value: COMPRESSED_CODE,
        },
    );
    dispatch(&mut host, CommandPayload::StepN { n: 1 });

    // 反汇编条目。
    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 1 });
    let entries = disasm_entries(&events);
    assert_eq!(entries[0].code, COMPRESSED_CODE, "16 位原码");
    assert_eq!(entries[0].width, 2);

    // 访存轨迹里的**指令**条目。
    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Execution,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    let (_s, _f, _o, _n, _t, chunk, _ov) = trace_export(&events);
    let row = chunk
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .find(|l| l.split(',').nth(2) == Some("instruction"))
        .unwrap_or_else(|| panic!("轨迹里没有指令条目：{chunk}"));
    let cols: Vec<&str> = row.split(',').collect();
    assert_eq!(
        cols[3].parse::<u32>().expect("机器码列"),
        entries[0].code,
        "轨迹里的机器码必须与反汇编条目**逐字段相同**（同一条取指口径）：{row}"
    );
}

#[test]
fn the_waveform_vcd_export_produces_a_vcd_product() {
    // ⚠️ **合法组合必须真的产出那种格式**（v0.20 检视 B · D-1）：
    // 规格 §5.2 #12 要求「`format=Vcd` ⇒ 合法」——但「**合法**」不等于
    // 「**返回的是 VCD**」⇒ 把 `export_vcd` 换成 `export_csv`，**795 + 5 条
    // 用例全绿** ⇒ 那条分支**没有判别力**。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 8 });

    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Waveform,
            format: TraceFormat::Vcd,
            offset: 0,
            limit: 0,
        },
    );
    assert_eq!(events.len(), 1);
    let (source, format, _o, _n, _t, chunk, _ov) = trace_export(&events);
    assert_eq!(source, TraceSource::Waveform);
    assert_eq!(format, TraceFormat::Vcd);
    // ⚠️ **VCD 的形状**：时间标度 + 变量声明 + 定义结束。
    assert!(chunk.contains("$timescale"), "VCD 必须有时间标度：{chunk}");
    assert!(chunk.contains("$var"), "VCD 必须有变量声明：{chunk}");
    assert!(
        chunk.contains("$enddefinitions $end"),
        "VCD 必须有定义结束：{chunk}"
    );
    assert!(
        !chunk.contains("time_ps,addr,kind,data,width"),
        "波形导出走 VCD 时**不许**是执行轨迹的 CSV 表头：{chunk}"
    );
}

#[test]
fn the_waveform_export_event_propagates_the_overflow_marker() {
    // ⚠️ **「窗口不完整」的标记必须原样传出去**（v0.20 检视 B · D-2）：
    // 把 `overflow_marked` 改成 `false`，**795 + 5 条用例全绿** ⇒
    // `Waveform` 这一侧的标记**没有判别力**（只有 `Execution` 侧有断言）。
    //
    // ⇒ 这里直接喂一个**人造**的 `ExportHandle`（`overflow_marked = true`）
    //    把那条映射**单独**钉住。
    let handle = crate::m19_waveform::export::ExportHandle {
        chunk: "x".to_string(),
        next_offset: 3,
        total: 9,
        done: false,
        progress_pct: 33,
        overflow_marked: true,
        dropped_ranges: Vec::new(),
    };
    let EventPayload::TraceExport {
        overflow_marked, ..
    } = waveform_export_event(handle, TraceFormat::Csv, 0)
    else {
        panic!("必须是 TraceExport 事件");
    };
    assert!(overflow_marked, "窗口不完整的标记必须传出去");
}

#[test]
fn the_host_can_look_up_a_symbol_by_address() {
    // ⚠️ **验收 #6 的生产路径**（v0.20 检视 B · D-4）：`SymbolService` 端口
    // 此前**全仓 0 处实现** ⇒ 「按地址查符号」这条能力**只在 M-11 单元层**
    // 被覆盖，**宿主根本没有它** ⇒ 与 D-50 / D-51 同形的「实现了 ≠ 接上了」。
    let mut host = KernelHost::new();
    let (symtab, strtab) = elf_symbol_table(&["alpha"]);
    dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(&[(".symtab", symtab), (".strtab", strtab)])),
    );

    let hit = host.symbol_at(0x1000).expect("按地址必须查得到");
    assert_eq!(hit.name, "alpha");
    assert_eq!(hit.addr, Some(0x1000));
    // 比所有符号的起始地址都小 ⇒ `None`（**不是错误**）。
    assert!(host.symbol_at(0x0).is_none());
    // 按名字查（同一个端口）也要通。
    assert_eq!(
        host.lookup_symbol("alpha").map(|s| s.name),
        Some("alpha".to_string())
    );
    assert!(host.lookup_symbol("nope").is_none());
}

// ---------------------------------------------------------------------------
// 12. 主题常量（STAGE-5 §3.5 / §9.6）
// ---------------------------------------------------------------------------

#[test]
fn the_topic_constants_are_the_registered_ones() {
    assert_eq!(TOPIC_DISASM, "debug.disasm");
    assert_eq!(TOPIC_SOURCE, "debug.source");
    assert_eq!(TOPIC_EXPORT, "debug.export");
    assert_eq!(TOPIC_TRACE, "debug.trace");
}

// ---------------------------------------------------------------------------
// 夹具：带调试节区的最小 ELF
// ---------------------------------------------------------------------------

/// 造一个 `.symtab` + `.strtab`（Elf32_Sym，16 字节一条）。
///
/// ⚠️ 这是**测试夹具**：只拼字节，M-06 仍然是唯一的 ELF 解析者、M-11
/// 仍然是唯一的符号表解析者。
fn elf_symbol_table(names: &[&str]) -> (Vec<u8>, Vec<u8>) {
    // `.strtab`：偏移 0 是空串，之后每个名字一个 NUL 结尾的串。
    let mut strtab = vec![0u8];
    let mut offsets = Vec::new();
    for name in names {
        offsets.push(strtab.len() as u32);
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
    }

    // `.symtab`：第 0 号是空符号（名字偏移 0 ⇒ 空名 ⇒ M-11 跳过它）。
    let mut symtab = Vec::new();
    symtab.extend_from_slice(&elf_sym(0, 0, 0, 0, 0));
    for (index, offset) in offsets.iter().enumerate() {
        // st_info = (STT_FUNC << 4) | STB_GLOBAL = 0x21；shndx = 1（已定义）。
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
        id: "proj-m11-elf".to_string(),
        name: "m11-elf".to_string(),
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
