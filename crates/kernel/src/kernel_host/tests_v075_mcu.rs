//! STAGE-51 · v0.75 批 —— **MCU 概念**的宿主侧门禁（K 组 + A-3 + X-1）。
//!
//! ⚠️ **本文件钉的是「内核怎么判」**：板声明的 `mcu.isa` ⟷ 镜像的 ELF 头
//! ⇒ ⇒ 判不过就**不应用板、不建图、不写总线**（与「未知系统」门禁同款
//! 次序纪律 ⇒ ⇒ 机器/生效板/修订号**全都不动**）。
//!
//! ⚠️ **判别力**：
//! - c 位判据删掉（只认 class）⇒ ⇒ **K-3** 红（T-1）；
//! - 缺省（无 mcu）时也校验 ⇒ ⇒ **K-4** 红（T-2）；
//! - class 判据删掉 ⇒ ⇒ **K-2** 红（T-3）；
//! - 契约词法只判前缀（"rv32x" 放行）⇒ ⇒ **A-3 / X-1** 红（T-4）。
//!
//! ⚠️ 断言全部落在**输出**上：命令回的事件 ⇒ ⇒ 没有一条断言是「我调用了
//! 某个内部函数」（纯函数那部分 —— 词法 —— 在契约层 [`mcu_isa_info`]，
//! 这里只断**宿主行为**）。

use riscv_sim_contract::error::{E_PLT_21_MCU_INVALID, PLATFORM_FAMILY};
use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource, ProductTree, Project};
use riscv_sim_contract::platform::{
    mcu_isa_info, McuSpec, PlatformDescription, RegionSpec, VectorTableSpec,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload};

use super::tests_watchdog::{dispatch, errors_of};
use super::KernelHost;

/// 造一个最小的 ELF32（RV32 / 小端 / ET_EXEC），只有一个 PT_LOAD。
///
/// ⚠️ **为什么自己造**：真实固件的 ELF 在 `tests/golden/_out/`（仓内），但
/// 单测要**免 IO、免路径依赖** ⇒ 用与 tests_v024 **同一手法**的手造 ELF
/// 驱动门禁；K-5 的「存量 80 个 golden 固件照装」由**全量集成测试 + 变异
/// 脚本**覆盖（规格 §8）。
///
/// ⚠️ **`flags` 参数**：`0x1` = `EF_RISCV_RVC`（压缩指令位）⇒ ⇒ c 位
/// 门禁的判据输入（唯一带 RVC 的 golden 固件就是这一个位）。
fn minimal_elf(vaddr: u32, code: &[u8], flags: u32) -> Vec<u8> {
    const EHDR: usize = 52;
    const PHDR: usize = 32;
    let mut out = vec![0u8; EHDR + PHDR + code.len()];
    out[0] = 0x7f;
    out[1] = b'E';
    out[2] = b'L';
    out[3] = b'F';
    out[4] = 1; // EI_CLASS = ELFCLASS32
    out[5] = 1; // EI_DATA = ELFDATA2LSB
    out[6] = 1; // EI_VERSION
    out[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
    out[18..20].copy_from_slice(&243u16.to_le_bytes()); // e_machine = EM_RISCV
    out[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    out[24..28].copy_from_slice(&vaddr.to_le_bytes()); // e_entry
    out[28..32].copy_from_slice(&(EHDR as u32).to_le_bytes()); // e_phoff = 52
    out[32..36].copy_from_slice(&0u32.to_le_bytes()); // e_shoff（无节区表）
    out[36..40].copy_from_slice(&flags.to_le_bytes()); // e_flags（**本批新增落点**）
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
    out[p + 28..p + 32].copy_from_slice(&4u32.to_le_bytes()); // p_align
    out[EHDR + PHDR..].copy_from_slice(code);
    out
}

/// 一条 RAM 在 `base`、长 `size` 的描述（**用户的板**）+ 可选的 `mcu`。
///
/// ⚠️ **id 必须是参数**（STAGE-47 施工期订正）：`KernelHost::platform_id()`
/// 返回的是**当前生效那张板的 id** ⇒ ⇒ 两块板**同名** ⇒ ⇒ 「生效图有没有
/// 被动过」那条断言**分辨不出来** ⇒ ⇒ id 必须不同。
fn board(id: &str, base: u32, size: u32, mcu: Option<McuSpec>) -> PlatformDescription {
    PlatformDescription {
        schema_version: riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION,
        id: id.to_string(),
        name: format!("Board {id}"),
        priority: 100,
        // ⚠️ 用户配的板**不**参与自动选档（它是**点名**用的）。
        auto_selectable: false,
        description: None,
        wiring: None,
        region_budget_bytes: 16 * 1024 * 1024,
        max_region_bytes: None,
        regions: vec![RegionSpec::Ram {
            base: Some(base),
            size,
        }],
        roles: None,
        pins: None,
        mcu,
    }
}

/// 一块带 `mcu` 的板（ISA 由参数给）。
fn board_with_isa(id: &str, isa: &str) -> PlatformDescription {
    board(
        id,
        0x0000_0000,
        0x0001_0000,
        Some(McuSpec {
            isa: isa.to_string(),
            vendor: None,
            model: None,
            vector_table: None,
        }),
    )
}

/// 一条 32 字节的**可执行固件**（`nop` × 8 ⇒ ⇒ 合法指令 ⇒ ⇒ 访客真能跑）。
fn firmware_bytes() -> Vec<u8> {
    let mut buf = vec![0u8; 32];
    for i in 0..8 {
        buf[i * 4] = 0x93; // addi x0, x0, 0
    }
    buf
}

/// 只含一个 ELF 镜像的工程。
fn elf_project(elf: Vec<u8>) -> Project {
    Project {
        id: "proj-v075-mcu".to_string(),
        name: "v075-mcu".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "fw.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(elf),
        }],
        version: riscv_sim_contract::model::ProjectVersion::default(),
    }
}

/// 配一张**用户的板**（⇒ ⇒ 之后 `LoadProject` 就**用**它）。
fn set_user_board(host: &mut KernelHost, description: PlatformDescription) {
    let events = dispatch(
        host,
        CommandPayload::SetMemoryMap {
            description,
            expected_revision: None,
        },
    );
    let errors = errors_of(&events);
    assert!(
        errors.is_empty(),
        "配用户的板就失败了（{:?}）⇒ ⇒ 后面那些判据没有意义",
        errors
    );
}

/// 装那个工程 ⇒ ⇒ 回的事件。
fn load(host: &mut KernelHost, project: Project) -> Vec<EventPayload> {
    dispatch(host, CommandPayload::LoadProject(project))
}

/// 事件里有没有 `E-PLT-20`（「固件 ISA 不匹配」那个码）。
///
/// ⚠️ **线上形态是 `E_PLT-20`**（族与场景码之间是**连字符**）⇒ ⇒ Rust
/// 常量名是 `E_PLT_20_FW_ISA_MISMATCH` ⇒ ⇒ 两者**不是一个字符串** ⇒ ⇒
/// 断言必须用**线上形态**（同「未知系统」那组的教训）。
fn is_isa_mismatch(events: &[EventPayload]) -> bool {
    errors_of(events).iter().any(|(code, _)| code == "E_PLT-20")
}

// ---------------------------------------------------------------------------

/// **A-3** —— isa 词法（契约层 [`mcu_isa_info`] 的**唯一落点**）。
///
/// ⚠️ 判据来自规格 §4.1 A-3：`"rv32ima"` ✓ · `"RV32IMAC"`（归一）✓ ·
/// `"rv64x"`（非法扩展）✗ · `"rv32"`（无扩展字母）✗ · `"arm"` ✗。
/// 且归一化后**语义相同**（`"RV32IMAC"` ⇒ `bits=32, has_c`）。
#[test]
fn a3_isa_lexicon_accepts_only_the_canonical_shape() {
    let ok32 = mcu_isa_info("rv32ima").expect("rv32ima 必须合法");
    assert_eq!((ok32.bits, ok32.has_c, ok32.has_e), (32, false, false));
    let ok32c = mcu_isa_info("rv32imac").expect("rv32imac 必须合法");
    assert_eq!((ok32c.bits, ok32c.has_c, ok32c.has_e), (32, true, false));
    let ok64 = mcu_isa_info("rv64ima").expect("rv64ima 必须合法");
    assert_eq!(ok64.bits, 64);

    // 大小写归一 ⇒ 语义相同。
    let upper = mcu_isa_info("RV32IMAC").expect("大写必须归一后合法");
    assert_eq!(upper, ok32c, "大小写归一后语义必须逐字相同");

    // 非法形态 ⇒ None（V-23 据此拒 E_PLT-21）。
    for bad in [
        "rv64x", "rv32", "arm", "rv32imaq", "rv32cima", "rv128ima", "rv32imaa",
    ] {
        assert_eq!(
            mcu_isa_info(bad),
            None,
            "{bad:?} 不该通过词法（去重/规范序/扩展字母集/位宽）"
        );
    }
}

/// **A-3b** —— 描述带非法 `mcu.isa` ⇒ ⇒ 契约校验**就拒**（V-23 · `E_PLT-21`）。
///
/// ⚠️ 与装载门禁（`E_PLT-20`）**是两个对象** ⇒ ⇒ 各自的码（一码一义）。
#[test]
fn a3b_a_board_with_illegal_isa_is_rejected_at_validate() {
    let mut bad = board_with_isa("board-bad-isa", "rv32x");
    bad.schema_version = riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION;
    let err = bad.validate().expect_err("非法 isa 必须被 V-23 拒");
    assert_eq!(err.family, PLATFORM_FAMILY, "错误族必须是平台族");
    assert_eq!(err.scenario, E_PLT_21_MCU_INVALID, "场景码必须是 E_PLT-21");
}

/// **A-3c** —— 向量表表项数 0 ⇒ ⇒ 也拒（V-23 · `E_PLT-21`）。
#[test]
fn a3c_a_vector_table_with_zero_entries_is_rejected() {
    let mut board = board_with_isa("board-vt0", "rv32imac");
    board.mcu = Some(McuSpec {
        isa: "rv32imac".to_string(),
        vendor: None,
        model: None,
        vector_table: Some(VectorTableSpec {
            base: 0x8000_0000,
            entries: 0,
        }),
    });
    let err = board.validate().expect_err("表项数 0 必须被拒");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(err.scenario, E_PLT_21_MCU_INVALID);
}

// ---------------------------------------------------------------------------

/// **K-1** —— 匹配装：板声明 `rv32ima` + ELF32 固件 ⇒ ⇒ **装得上**。
#[test]
fn k1_matching_isa_loads() {
    let mut host = KernelHost::new();
    set_user_board(&mut host, board_with_isa("board-rv32ima", "rv32ima"));

    let events = load(
        &mut host,
        elf_project(minimal_elf(0x0000_0000, &firmware_bytes(), 0)),
    );

    assert!(
        errors_of(&events).is_empty(),
        "ISA 匹配却装不上（{:?}）⇒ ⇒ 门禁误伤",
        errors_of(&events)
    );
    assert!(
        !is_isa_mismatch(&events),
        "ISA 匹配却报了 E_PLT-20 ⇒ ⇒ 门禁误伤"
    );
}

/// **K-1 对照** —— 无 `mcu` 的板 + 同固件 ⇒ ⇒ 也装（缺省逐字不变）。
#[test]
fn k1b_without_mcu_the_same_firmware_loads() {
    let mut host = KernelHost::new();
    set_user_board(
        &mut host,
        board("board-nomcu", 0x0000_0000, 0x0001_0000, None),
    );

    let events = load(
        &mut host,
        elf_project(minimal_elf(0x0000_0000, &firmware_bytes(), 0)),
    );

    assert!(
        errors_of(&events).is_empty(),
        "无 mcu 却装不上（{:?}）⇒ ⇒ 缺省被破坏了",
        errors_of(&events)
    );
}

/// **K-2** —— class 不匹配：板声明 `rv64ima` + ELF32 固件 ⇒ ⇒ **拒**
/// `E_PLT-20`（判据 family + scenario，不看 message）。
///
/// ⚠️ 必须断「**机器没动**」：拒了之后 PC 不变、生效板不变（门禁在
/// **应用板之前**判）⇒ ⇒ 与「未知系统」门禁同款次序纪律。
#[test]
fn k2_rv64_board_rejects_elf32_firmware() {
    let mut host = KernelHost::new();
    set_user_board(
        &mut host,
        board("board-fit", 0x0000_0000, 0x0001_0000, None),
    );
    let first = load(
        &mut host,
        elf_project(minimal_elf(0x0000_0000, &firmware_bytes(), 0)),
    );
    assert!(
        errors_of(&first).is_empty(),
        "第一装不上（{:?}）⇒ ⇒ 后面那些判据没有意义",
        errors_of(&first)
    );
    // 让访客跑起来 ⇒ ⇒ PC ≠ 0 ⇒ ⇒ 「PC 没动」才有判别力。
    let stepped = dispatch(&mut host, CommandPayload::StepN { n: 4 });
    assert!(
        errors_of(&stepped).is_empty(),
        "单步失败（{:?}）⇒ ⇒ 后面那些判据没有意义",
        errors_of(&stepped)
    );
    let pc_before = host.pc();
    assert_ne!(pc_before, 0, "访客跑了几步 PC 还是 0 ⇒ ⇒ 零判别力");

    // 配一块 rv64 的板（已装载 ⇒ ⇒ 待生效）⇒ ⇒ 再装 ⇒ ⇒ 必须拒。
    set_user_board(&mut host, board_with_isa("board-rv64", "rv64ima"));
    let platform_before = host.platform_id().to_string();

    let events = load(
        &mut host,
        elf_project(minimal_elf(0x0000_0000, &firmware_bytes(), 0)),
    );

    assert!(
        is_isa_mismatch(&events),
        "rv64 板 + ELF32 固件却没拒 ⇒ ⇒ class 判据零判别力（事件={:?}）",
        errors_of(&events)
    );
    assert_eq!(
        host.pc(),
        pc_before,
        "拒了之后 PC 变了（{pc_before:#x} ⇒ ⇒ {:x}）⇒ ⇒ 机器被复位了 ⇒ ⇒ 次序错了",
        host.pc()
    );
    assert_eq!(
        host.platform_id(),
        platform_before.as_str(),
        "拒了之后当前生效的板**变了** ⇒ ⇒ 门禁在应用板**之后**判 ⇒ ⇒ 次序错了"
    );
}

/// **K-3** —— c 位**单方向**严格：板声明 `rv32i`（无 c）+ **RVC 固件**
/// ⇒ ⇒ **拒** `E_PLT-20`；对照：声明 `rv32imac` + 同固件 ⇒ ⇒ **装得上**。
#[test]
fn k3_rvc_firmware_needs_a_board_that_declares_c() {
    let rvc_elf = minimal_elf(0x0000_0000, &firmware_bytes(), 0x1); // EF_RISCV_RVC

    // ① 无 c 的板 ⇒ 拒。
    let mut host = KernelHost::new();
    set_user_board(&mut host, board_with_isa("board-rv32i", "rv32i"));
    let events = load(&mut host, elf_project(rvc_elf.clone()));
    assert!(
        is_isa_mismatch(&events),
        "rv32i（无 c）+ RVC 固件却没拒 ⇒ ⇒ c 位判据零判别力（事件={:?}）",
        errors_of(&events)
    );

    // ② 对照：带 c 的板 ⇒ 装得上（c 是能力不是义务）。
    let mut host = KernelHost::new();
    set_user_board(&mut host, board_with_isa("board-rv32imac", "rv32imac"));
    let events = load(&mut host, elf_project(rvc_elf));
    assert!(
        errors_of(&events).is_empty(),
        "rv32imac + RVC 固件却装不上（{:?}）⇒ ⇒ c 位判据做成了**双向**严格",
        errors_of(&events)
    );
    assert!(
        !is_isa_mismatch(&events),
        "rv32imac + RVC 固件却报了 E_PLT-20 ⇒ ⇒ 双向严格误伤存量"
    );
}

/// **K-4** —— 缺省优先：无 `mcu` 段 ⇒ ⇒ **压缩固件也装**（不校验）⇒ ⇒
/// 存量零风险。
#[test]
fn k4_without_mcu_compressed_firmware_also_loads() {
    let mut host = KernelHost::new();
    set_user_board(
        &mut host,
        board("board-nomcu", 0x0000_0000, 0x0001_0000, None),
    );

    let events = load(
        &mut host,
        elf_project(minimal_elf(0x0000_0000, &firmware_bytes(), 0x1)),
    );

    assert!(
        errors_of(&events).is_empty(),
        "无 mcu 时压缩固件也装不上（{:?}）⇒ ⇒ 缺省被做成了「拿默认 isa 查」",
        errors_of(&events)
    );
}

/// **K-5** —— 自动选档路径（**用户没配板** ⇒ ⇒ 走内置 qemu-virt，`mcu` =
/// `rv32imac`）⇒ ⇒ ELF32 固件照装；RVC 固件**也**装（声明含 c）。
///
/// ⚠️ 这是**存量链路零影响**的那条护栏：平台的真机门禁（脚本口、demo 固件、
/// osz）全都走这条路 ⇒ ⇒ 它们**必须**照旧装得上（80 个 golden 固件全部
/// class=32 / 不带 RVC ⇒ ⇒ 本判据覆盖它们的公共形状）。
#[test]
fn k5_auto_selected_qemu_virt_accepts_legacy_firmware() {
    // ① ELF32（不带 RVC）⇒ 装。
    let mut host = KernelHost::new();
    let events = load(
        &mut host,
        elf_project(minimal_elf(0x8400_0000, &firmware_bytes(), 0)),
    );
    assert!(
        errors_of(&events).is_empty(),
        "自动选 qemu-virt + ELF32 却装不上（{:?}）⇒ ⇒ 存量链路被破坏",
        errors_of(&events)
    );

    // ② RVC 固件 ⇒ 也装（qemu-virt 声明 rv32imac，含 c）。
    let mut host = KernelHost::new();
    let events = load(
        &mut host,
        elf_project(minimal_elf(0x8400_0000, &firmware_bytes(), 0x1)),
    );
    assert!(
        errors_of(&events).is_empty(),
        "自动选 qemu-virt（含 c）+ RVC 固件却装不上（{:?}）⇒ ⇒ c 位做成了双向严格",
        errors_of(&events)
    );
}

/// **K-5b** —— 自动选档路径**也会**被 ISA 门禁拦：把内置板换成带 rv64 mcu
/// 的假想板 ⇒ 不可能（内置只有 qemu-virt）⇒ 这里用**用户板 + 自动选档的
/// 边角**：用户配了 rv64 板但**没装载**（SetMemoryMap 立即生效 ⇒
/// `self.platform` 已是 rv64 板）⇒ 再装 ELF32 ⇒ 拒且生效板不变。
///
/// ⚠️ 与 K-2 的区别：K-2 是**已装载**场景（板待生效）；这条是**未装载**
/// 场景（板立即生效）⇒ ⇒ 两条路都覆盖 ⇒ ⇒ 门禁在两个分支都生效。
#[test]
fn k5c_the_gate_also_holds_when_the_board_is_applied_immediately() {
    let mut host = KernelHost::new();
    // 未装载 ⇒ SetMemoryMap 立即生效 ⇒ self.platform = rv64 板。
    set_user_board(&mut host, board_with_isa("board-rv64", "rv64ima"));
    let platform_before = host.platform_id().to_string();

    let events = load(
        &mut host,
        elf_project(minimal_elf(0x0000_0000, &firmware_bytes(), 0)),
    );

    assert!(
        is_isa_mismatch(&events),
        "立即生效的 rv64 板 + ELF32 固件却没拒（事件={:?}）",
        errors_of(&events)
    );
    assert_eq!(
        host.platform_id(),
        platform_before.as_str(),
        "拒了之后生效板变了 ⇒ ⇒ 门禁晚了一步"
    );
}

/// **X-1** —— 契约 `mcu_isa_info()` 与内核门禁**同一个落点**。
///
/// ⚠️ 内核门禁**不重抄词法**：这里断的是「内核侧**没有**自己的词法」——
/// 具体做法：词法行为**只**由契约函数提供，变异 T-4（把词法改成只判前缀）
/// 会同时红掉 A-3 与**本文件所有依赖词法的用例**（K-3 的 `rv32i` 依赖
/// `has_c` 语义）⇒ ⇒ 词法只有一处。
///
/// 这条把「**判据形状**」钉死：`has_c` 语义 = 声明含 `c`；`bits` 语义 =
/// 位宽。任何一侧擅自改语义都会在这里露出。
#[test]
fn x1_lexicon_is_the_single_source_for_the_gate() {
    // rv32i：bits=32、无 c ⇒ K-3 的「拒」依赖它。
    let i = mcu_isa_info("rv32i").expect("rv32i 合法");
    assert_eq!((i.bits, i.has_c), (32, false));
    // rv32imac：bits=32、有 c ⇒ K-3 的「装」依赖它。
    let imac = mcu_isa_info("rv32imac").expect("rv32imac 合法");
    assert_eq!((imac.bits, imac.has_c), (32, true));
    // rv64ima：bits=64 ⇒ K-2 的「拒」依赖它。
    let ima64 = mcu_isa_info("rv64ima").expect("rv64ima 合法");
    assert_eq!((ima64.bits, ima64.has_c), (64, false));
}
