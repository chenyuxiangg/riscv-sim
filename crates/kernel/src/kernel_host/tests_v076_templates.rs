//! STAGE-52 · v0.76 批 —— **模板库协议**的宿主侧门禁（M 组）。
//!
//! ⚠️ **本文件钉的是「内核怎么报模板」**（裁决 M）：清单 = 内置板
//! **元数据**（id / 名称 / 优先级 / 器件数 · **priority 升序**）+ 按 `id`
//! 取**完整描述**（逐字等于内置 JSON 解析结果）⇒ ⇒ 未知 `id` 拒
//! **E_PLT-22**（与「无板装载」同一个码 ⇒ 引导用户从模板库选择）。
//!
//! ⚠️ **前端不抄模板数据** ⇒ ⇒ 清单**必须**由内核报 ⇒ ⇒ 前端那一条
//! 测试读的**就是**这条协议的回复（B-168 真机门禁同源）。
//!
//! ⚠️ **判别力**：
//! - 清单顺序写成内置存储顺序（default → qemu-virt → minimal）⇒ ⇒
//!   M-1 **变红**（不是 priority 升序）；
//! - device_count 数错（数成全部区域）⇒ ⇒ M-1 **变红**（必须只数
//!   peripheral）；
//! - 未知 id 回 E_PLT-09 ⇒ ⇒ M-3 **变红**（契约要的是 E_PLT-22）。

// ⚠️ 断言值一律写架构字面量（`"E_PLT-22"`），不引常量 —— 与
// tests_board_delta_equivalence 同款纪律。
use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, PlatformTemplateInfo, ProductTree, Project, ProjectVersion,
};
use riscv_sim_contract::platform::RegionSpec;
use riscv_sim_contract::protocol::{CommandPayload, EventPayload};

use super::tests_watchdog::dispatch;
use super::KernelHost;

/// 造一个**最小**的宿主（不装载任何工程）。
fn host() -> KernelHost {
    KernelHost::new()
}

/// 把事件里的模板清单抠出来。
fn templates_of(events: &[EventPayload]) -> &Vec<PlatformTemplateInfo> {
    for event in events {
        if let EventPayload::PlatformTemplates { templates } = event {
            return templates;
        }
    }
    panic!("回复里没有 PlatformTemplates：{events:?}");
}

/// M-1 —— 模板清单 = 3 块内置板 · **priority 升序** · 器件数只数
/// peripheral。
///
/// ⚠️ 数值**逐字来自内置 JSON**（default 10 / qemu-virt 20 / minimal
/// 30；器件数 = 描述里 `peripheral` 区域的数量 1 / 4 / 0）⇒ ⇒ 内置板
/// 改了数据，这条**当场变红**。
#[test]
fn the_template_list_reports_the_three_builtins_sorted_by_priority() {
    let mut host = host();
    let events = dispatch(&mut host, CommandPayload::ListPlatformTemplates);
    let templates = templates_of(&events);
    assert_eq!(templates.len(), 3, "内置板必须是 3 块");
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["default", "qemu-virt", "minimal"],
        "清单必须是 priority 升序：default(10) → qemu-virt(20) → minimal(30)",
    );
    // 优先级与器件数**逐字来自内置 JSON**。
    assert_eq!(templates[0].priority, 10);
    assert_eq!(
        templates[0].device_count, 1,
        "default 的器件数 = 1（只有 UART0）"
    );
    assert_eq!(templates[1].priority, 20);
    assert_eq!(templates[1].device_count, 4, "qemu-virt 的器件数 = 4");
    assert_eq!(templates[2].priority, 30);
    assert_eq!(templates[2].device_count, 0, "minimal 没有外设器件");
    // 名称不能是空的（模板库卡片要展示）。
    for t in templates {
        assert!(!t.name.is_empty(), "模板 {} 的名称不能是空的", t.id);
    }
}

/// M-2 —— 按 id 取模板 ⇒ ⇒ **完整描述**逐字等于内置 JSON 解析结果。
///
/// ⚠️ 断言**落在输出上**：qemu-virt 的完整描述必须带那 4 个器件
/// （CLINT / PLIC / UART0 / GPIO0）⇒ ⇒ 前端拿它当草稿起点（
/// `draftFromDescription`）不会缺器件。
#[test]
fn getting_a_template_returns_its_full_description() {
    let mut host = host();
    let events = dispatch(
        &mut host,
        CommandPayload::GetPlatformTemplate {
            id: "qemu-virt".to_string(),
        },
    );
    let description = match events.as_slice() {
        [EventPayload::PlatformTemplate { description }] => description.as_ref().clone(),
        other => panic!("回复必须是单条 PlatformTemplate，实际 {other:?}"),
    };
    assert_eq!(description.id, "qemu-virt");
    assert_eq!(description.priority, 20);
    let peripheral_kinds: Vec<&str> = description
        .regions
        .iter()
        .filter_map(|region| match region {
            RegionSpec::Peripheral { device, .. } => Some(device.kind()),
            _ => None,
        })
        .collect();
    for kind in ["clint", "plic", "uart16550", "gpio"] {
        assert!(
            peripheral_kinds.contains(&kind),
            "qemu-virt 模板的描述必须含器件 {kind}（缺件 ⇒ 前端导入草稿会残），实际 {peripheral_kinds:?}"
        );
    }
}

/// M-3 —— 未知模板 id ⇒ ⇒ 拒 **E_PLT-22**（与「无板装载」同一个码 ⇒ ⇒
/// 引导用户从模板库选择）。
#[test]
fn an_unknown_template_id_is_rejected_with_e_plt_22() {
    let mut host = host();
    let events = dispatch(
        &mut host,
        CommandPayload::GetPlatformTemplate {
            id: "no-such-board".to_string(),
        },
    );
    match events.as_slice() {
        [EventPayload::Error { code, .. }] => {
            assert_eq!(*code, "E_PLT-22", "未知模板必须拒 E_PLT-22");
        }
        other => panic!("回复必须是单条 Error，实际 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 判别力（B 组 · 真机门禁同源断言 —— 无板时的引导文案）
// ---------------------------------------------------------------------------

/// B-169 —— 无生效板装载 ⇒ ⇒ E_PLT-22 + 引导文案含「模板导入」。
///
/// ⚠️ 文案**逐字等于**规格原文 ⇒ ⇒ 改了引导 ⇒ ⇒ 这条变红 ⇒ ⇒ 前端
/// 测试同源（真机门禁 B-169 看同一份文案）。
#[test]
fn loading_without_a_board_is_rejected_with_the_template_guidance() {
    let mut host = host();
    let events = dispatch(
        &mut host,
        CommandPayload::LoadProject(Project {
            id: "proj-no-board".to_string(),
            name: "no-board".to_string(),
            tree: ProductTree::default(),
            images: vec![ImageRef {
                name: "demo-firmware.bin".to_string(),
                kind: ImageKind::RawHex,
                source: ImageSource::Bytes(vec![0u8; 8]),
            }],
            version: ProjectVersion::default(),
        }),
    );
    match events.as_slice() {
        [EventPayload::Error { code, message }] => {
            assert_eq!(*code, "E_PLT-22", "无板装载必须拒 E_PLT-22");
            assert!(
                message.contains("模板导入"),
                "引导文案必须指向模板导入，实际：{message}"
            );
        }
        other => panic!("回复必须是单条 Error，实际 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// K-3 —— 未知系统门禁（B2 · 判据 = 即将生效板 · **不兜底**）
// ---------------------------------------------------------------------------

/// ⚠️ 手造 ELF（与 tests_v075_mcu 同手法 · 免 IO）：PT_LOAD 落在 `vaddr`。
///
/// ⚠️ **为什么 K-3 要 ELF**：裸镜像（RawHex bytes）落在 0 ⇒ ⇒ minimal
/// 有基址 0 的 RAM ⇒ ⇒ 落得进 ⇒ ⇒ 触不到「未知系统」⇒ ⇒ **必须**带地址
/// 的镜像，地址取 qemu-virt 才有的 GPIO 区（`0x4000_0000`）。
fn minimal_elf(vaddr: u32, code: &[u8]) -> Vec<u8> {
    const EHDR: usize = 52;
    const PHDR: usize = 32;
    let mut out = vec![0u8; EHDR + PHDR + code.len()];
    out[0..4].copy_from_slice(b"\x7fELF");
    out[4] = 1; // EI_CLASS = ELFCLASS32
    out[5] = 1; // EI_DATA = ELFDATA2LSB
    out[6] = 1; // EI_VERSION
    out[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
    out[18..20].copy_from_slice(&243u16.to_le_bytes()); // e_machine = EM_RISCV
    out[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    out[24..28].copy_from_slice(&vaddr.to_le_bytes()); // e_entry
    out[28..32].copy_from_slice(&(EHDR as u32).to_le_bytes()); // e_phoff
    out[40..42].copy_from_slice(&(EHDR as u16).to_le_bytes()); // e_ehsize
    out[42..44].copy_from_slice(&(PHDR as u16).to_le_bytes()); // e_phentsize
    out[44..46].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
    let p = EHDR;
    out[p..p + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    out[p + 4..p + 8].copy_from_slice(&((EHDR + PHDR) as u32).to_le_bytes()); // p_offset
    out[p + 8..p + 12].copy_from_slice(&vaddr.to_le_bytes()); // p_vaddr
    out[p + 12..p + 16].copy_from_slice(&vaddr.to_le_bytes()); // p_paddr
    out[p + 16..p + 20].copy_from_slice(&(code.len() as u32).to_le_bytes()); // p_filesz
    out[p + 20..p + 24].copy_from_slice(&(code.len() as u32).to_le_bytes()); // p_memsz
    out[p + 24..p + 28].copy_from_slice(&5u32.to_le_bytes()); // p_flags = R+X
    out[EHDR + PHDR..].copy_from_slice(code);
    out
}

fn elf_project(elf: Vec<u8>) -> Project {
    Project {
        id: "proj-elf".to_string(),
        name: "elf".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "fw.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(elf),
        }],
        version: ProjectVersion::default(),
    }
}

/// **K-3** —— 导入 minimal（RAM 小）⇒ 装 qemu-virt 才装得下的地址
/// （`0x4000_0000`）⇒ **E_PLT-17**（判据 = **即将生效的板** · **不再**有
/// 自动选档候选兜底 ⇒ 没有「换成 qemu-virt 再装」这件事）。
///
/// ⚠️ **判据为什么是「即将生效的板」**：`SetMemoryMap(minimal)` 之后
/// `self.platform` **已经**是 minimal（本批 B1 无板装载已拒、这里**先配
/// 板**）⇒ ⇒ 装载校验看的**就是** minimal 的区间 ⇒ ⇒ 「判据用哪一份」
/// 可观测（不是恒真）。
#[test]
fn k3_a_big_image_on_the_minimal_template_is_rejected_as_unknown_system() {
    let mut host = host();
    // ---- ① 从模板导入 minimal（真走模板协议）----------------------------
    let listed = dispatch(
        &mut host,
        CommandPayload::GetPlatformTemplate {
            id: "minimal".to_string(),
        },
    );
    let description = match listed.as_slice() {
        [EventPayload::PlatformTemplate { description }] => description.as_ref().clone(),
        other => panic!("GetPlatformTemplate 必须回描述，实际 {other:?}"),
    };
    let mapped = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description,
            expected_revision: None,
        },
    );
    assert!(
        !mapped
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "minimal 模板必须装得上：{mapped:?}"
    );
    // ---- ② 装 qemu-virt 才装得下的镜像 ⇒ 必须拒 E_PLT-17 ----------------
    let code = vec![0x13u8; 8]; // nop × 8
    let events = dispatch(
        &mut host,
        CommandPayload::LoadProject(elf_project(minimal_elf(0x4000_0000, &code))),
    );
    match events.as_slice() {
        [EventPayload::Error { code, message }] => {
            assert_eq!(*code, "E_PLT-17", "未知系统必须拒 E_PLT-17");
            assert!(
                message.contains("未知系统") && message.contains("minimal"),
                "详情必须点名即将生效的板（minimal）· 实际 {message}"
            );
        }
        other => panic!("回复必须是单条 Error，实际 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// K-7 —— 冲突校验（E_PLT-19 · 同引脚两路径源不一致 ⇒ 拒）
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// K-9 —— 必要对照：存量描述（无 pins / 无 pinmux）照装（B-173 同源）
// ---------------------------------------------------------------------------
