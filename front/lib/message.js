// JS mirror of the frozen `riscv_sim_contract::protocol::Message` envelope.
//
// The contract is **frozen** (`crates/contract/src/protocol.rs`), so every
// field name and every enum tag in this file is copied verbatim from the
// Rust source. This module is the single place that knows how the front
// end spells the wire format — no other front-end module is allowed to
// hand-roll an envelope.
//
// The Rust side serialises with serde's default externally-tagged enum
// representation:
//
//   Message { schema_version, direction, seq, payload }
//   Payload::Command(c)           -> { "Command": <c> }
//   Payload::Event(e)             -> { "Event": <e> }
//   CommandPayload::Nop           -> "Nop"              (unit variant)
//   CommandPayload::StepN { n }   -> { "StepN": { "n": 3 } }
//   EventPayload::Halted { .. }   -> { "Halted": { .. } }
//
// A unit variant therefore serialises as a bare string, and a struct /
// newtype variant as a single-key object. `decodeMessage()` inverts
// exactly that and rejects everything else with E_PROTO-08.
//
// Contract v2 (`schema_version = 2`) added three things this file mirrors:
//   - three new commands (`ReadRegisters` / `WriteRegister` / `Negotiate`)
//     and three new events (`Registers` / `Negotiated` / `SimStateDelta`);
//   - the D-34 rename `WaveformDelta` -> `WaveformHistory` (the *event*
//     variant follows the *content*);
//   - the §2 numeric policy: virtual time travels as a **decimal string**
//     (`ps()` / `psToNumber()`), everything else stays a JSON number.
//
// Contract v4 (`schema_version = 4`) adds:
//   - one new event: `Console { bytes }` — the guest's console (UART TX)
//     byte stream. The bytes travel as a **JSON array of numbers**
//     (serde's default for `Vec<u8>`), **not** as a string and **not** as
//     base64: the byte stream is the truth (the Spike comparison is
//     byte-wise), the front end decodes it for display;
//   - two **optional** DTO fields on `model::DisasmEntryDto` (`file` /
//     `line`) — the per-instruction source location (D-102). `null` means
//     "unknown"; it is **never** fabricated.
//
// Contract v6 (`schema_version = 6`) adds:
//   - one new command: `SetMemoryMap { description }` — the board the user
//     configured **in the front end**. It carries a **whole**
//     `PlatformDescription` (the contract layer **already has** it ⇒
//     **zero new DTO**), **not** a patch;
//   - one new event: `MemoryMap { description, pending }` — the
//     **currently effective** map (the front end's memory-map table's
//     data source ⇒ the mock table becomes real) plus a flag telling
//     whether the host holds a **pending** board configuration;
//   - ⚠️ nothing else changed in v6: no DTO change, no renumbering.
//
// Contract v7 (`schema_version = 7`) adds:
//   - one new command: `UnloadProject` — a **unit** variant (no payload)
//     ⇒ the wire form is the **bare string** `"UnloadProject"` (same
//     shape as `Nop` / `Continue`);
//   - one new command: `PatchMemoryMap { ops, expected_revision }` — the
//     **delta** update of the board memory map. `ops` is a list of
//     `MemoryMapOp`s, which use serde's **externally tagged** form ⇒ the
//     wire key **is** the Rust variant name (`UpsertRegion` /
//     `RemoveRegion` / `SetBudget`), **not** `op` / `upsert_region`;
//   - one new command field: `SetMemoryMap` gained `expected_revision`
//     (`None` ⇒ the host does **not** check the revision);
//   - one new event field pair: `MemoryMap` gained `loaded` (is an image
//     running?) and `revision` (the monotonically increasing map
//     revision ⇒ the front end sends it back as `expected_revision`).
//   - ⚠️ `PLATFORM_SCHEMA_VERSION` (the **description** format version)
//     is **unchanged** (still 1) — v7 only adds one optional field to
//     `PlatformDescription` ⇒ old descriptions still parse unchanged.
//
// Contract v5 (`schema_version = 5`) adds:
//   - one new command: `WriteConsole { bytes }` — the console **input**
//     direction (UART RX). It is the exact mirror of v4's `Console`
//     event (which is the TX direction): the bytes travel as a **JSON
//     array of numbers** as well, because the guest's byte stream is the
//     truth in **both** directions;
//   - ⚠️ nothing else changed in v5: no new event, no DTO change, no
//     renumbering.
//
// Contract v8 (`schema_version = 8`) adds:
//   - two **reply events** for the admin channel: `AdminLogs { entries }`
//     (the exported log entries) and `AdminDiag { diag }` (the health
//     management system's diagnostic kit) — contract v8 §2.2;
//   - ⚠️ **no new command**: `CommandPayload::Admin { op }` already
//     existed ⇒ ⇒ the channel only gained its **reply side**;
//   - two **scenario-code tables** on the front side (`FRONT_ERROR_CODES`
//     / `MIRRORED_ERROR_CODES` + `RUST_ERROR_CODE_REFS`) ⇒ ⇒ the front's
//     own codes finally have a **definition site** ⇒ ⇒ the "unique
//     definition" invariant can bite them too (STAGE-29 裁决 E / F).
//
// Contract v9 (`schema_version = 9`) adds:
//   - one new command: `ResolveSymbol { name }` — ask the kernel (M-11) for
//     a symbol **by name** (contract v9 §2.2). One command serves **both**
//     usages: an **exact** hit arrives in `SymbolResolved.symbol`, a
//     **prefix** hit arrives in `SymbolResolved.candidates`;
//   - one new command: `DumpMemory { addr, len }` — read a **run** of bytes
//     (contract v9 §2.2). The old `ReadMemory` can only read 1/2/4 bytes;
//   - two new reply events: `SymbolResolved { name, symbol, candidates,
//     total }` and `MemoryDump { addr, bytes }` (the bytes travel as a
//     **JSON array of numbers**, the same convention as `Console`);
//   - two **mirrored limits** (`DUMP_MAX_BYTES` / `SYMBOL_CANDIDATE_LIMIT`)
//     — they are **cross-layer** numbers ⇒ ⇒ the only truth is
//     `crates/contract/fixtures/debug-limits.json` ⇒ ⇒ the Rust side
//     asserts its constants against that file and the front side reads the
//     same file ⇒ ⇒ **two truths** are structurally impossible;
//   - ⚠️ `Breakpoint` gained **one optional field** (`condition`) on the
//     Rust side ⇒ ⇒ `Command.setBreakpoint` stays a **pass-through**
//     (it never rewrites the DTO) ⇒ ⇒ the caller is the one that must
//     put `condition` on the wire (and `null` means "no condition", **not**
//     the empty string).
//   - ⚠️ `SIM_STATE_SCHEMA_VERSION` / `PLATFORM_SCHEMA_VERSION` /
//     `PLATFORM_RULES_VERSION` are **unchanged** (still 2 / 1 / 3).
//
// ⚠️ The two admin DTOs (`LogEntryDto` / `AdminDiagDto`) have **one**
//   truth: `crates/contract/fixtures/admin-dto.json` ⇒ ⇒ the Rust side
//   asserts its serialisation against that file and the front side reads
//   the same file (`front/tests/admin-dto.test.mjs`) ⇒ ⇒ a shape change
//   on either side turns the other side red.

/** Protocol schema version — mirrors `riscv_sim_contract::SCHEMA_VERSION`. */
export const SCHEMA_VERSION = 9;

/**
 * 转储长度上限（字节）—— mirrors `riscv_sim_contract::DUMP_MAX_BYTES`.
 *
 * ⚠️ **跨层的数** ⇒ ⇒ 它的**唯一真相**是
 *   `crates/contract/fixtures/debug-limits.json` ⇒ ⇒ 前端用例读**那一个
 *   文件**来断言本常量（契约 v9 §2.5 · 裁决 K）⇒ ⇒ 「两个真相」结构上
 *   不可能（前端抄一份字面量 ⇒ ⇒ 夹具断言当场红）。
 *
 * ⚠️ **前端拿它做预校验**（不发超限请求）⇒ ⇒ 但**内核仍然会拒**
 *   （`E_DBG-03`）⇒ ⇒ 前端**照样要把内核原文显示出来**（不吞、不改写）。
 */
export const DUMP_MAX_BYTES = 4096;

/**
 * 符号候选条数上限 —— mirrors `riscv_sim_contract::SYMBOL_CANDIDATE_LIMIT`.
 *
 * ⚠️ 同上：**跨层的数** ⇒ ⇒ 真相在
 *   `crates/contract/fixtures/debug-limits.json`（契约 v9 §2.5 · 裁决 K）。
 */
export const SYMBOL_CANDIDATE_LIMIT = 16;

/**
 * 描述格式版本 — mirrors `riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION`.
 *
 * ⚠️ **这与 `SCHEMA_VERSION`（契约信封的版本）是两个不同的东西**：
 * 前者管「**内存图描述**长什么样」（描述里那个 `schema_version` 字段），
 * 后者管「**消息信封**长什么样」。
 *
 * ⚠️ **它住在契约镜像层**（`front/lib/`）而不是视图层或服务层 ⇒
 * `front/views/components/memory-map.js` 与
 * `front/services/m23_boardstore.js` **都从这里 import** ⇒
 * **一个定义处**（STAGE-15 裁决 K：初版让服务层「另立一个同值常量」⇒
 * 那是**两个真相** ⇒ 不接受）。
 */
export const PLATFORM_SCHEMA_VERSION = 1;

/**
 * 合法性**规则集**版本 — mirrors `riscv_sim_contract::platform::PLATFORM_RULES_VERSION`.
 *
 * ⚠️ **它与 `PLATFORM_SCHEMA_VERSION`（形状）是两个不同的东西**：
 *   - `PLATFORM_SCHEMA_VERSION` = **形状**版本（内存图描述**长什么样**）；
 *   - `PLATFORM_RULES_VERSION` = **合法性规则集**版本（**哪些**描述合法）。
 *
 * ⚠️ **值 3 的含义**：版本 **1** = V-17 之前那套规则（**存量**按它判）⇒
 * 版本 **2** = 加上 V-17 之后那套 ⇒ 版本 **3** = 再加上 **V-18**（`wiring`
 * 里引用的器件必须存在于同一份描述里）与 **V-19**（`wiring` 的 `to.source`
 * 必须小于目标 PLIC 的 `sources`）之后那套 ⇒ ⇒ **老条目缺这个戳 ⇒ 隐含
 * 版本 1**（STAGE-18 裁决 G）⇒ ⇒ 「存量是不是老存量」**判得出来**。
 *
 * ⚠️ **每收紧一次规则集 ⇒ 这个数 +1** ⇒ 它就是「再收紧合法性规则时必须给
 * 迁移口径」那条规程的**可判依据**（台账 D-143）。
 *
 * ⚠️ **本批只升规则集版本**（STAGE-19 裁决 F）：规则集**收紧**了 ⇒ ⇒ 这个数
 * **2 → 3**；⚠️ **另外两个版本号一个都不升** ⇒ `SCHEMA_VERSION` **不因本批
 * 而变**（契约信封**零**变更）、`PLATFORM_SCHEMA_VERSION` **仍 1**（只加了一个
 * **带缺省**的**可选**字段 `wiring` ⇒ 按 STAGE-16 `max_region_bytes` 的
 * **同形先例** ⇒ **不是**破坏性格式变更 ⇒ ⇒ 存量板**不被新规则拒**）。
 *
 * ⚠️ **「不因本批而变」≠「永远是那个数」**：契约信封版本后来在 **v0.44 批**
 * （STAGE-29 · 管理员通道）升到了 **8** ⇒ ⇒ 那**不是**规则集版本 ⇒ ⇒ 本表
 * 的值**仍**是 3、`PLATFORM_SCHEMA_VERSION` **仍**是 1 ⇒ ⇒ 两者互不影响。
 *
 * ⚠️ **存量板走 v0.33 铺好的机制**（STAGE-19 裁决 F）：选用旧规则的板 ⇒
 * **明确提示**（**不静默**）⇒ 内核**接受** ⇒ **升级**落库条目（**按板 `id`**
 * 判）⇒ 内核**拒绝** ⇒ 显示**内核原文**（**不吞**）⇒ ⇒ **前端不写第二个
 * 校验器**（合法性**只**由内核那一处裁决）。
 *
 * ⚠️ **唯一落点**（STAGE-18 裁决 E）：住在这**一层**（`front/lib/`）⇒ 服务层
 * 与视图层**都从这里 import** ⇒ **一个定义处** ⇒ 依赖方向仍单向。
 */
export const PLATFORM_RULES_VERSION = 3;

// ---------------------------------------------------------------------------
// 场景码表（STAGE-29 · 裁决 E · v0.44 批）
// ---------------------------------------------------------------------------

/**
 * 前端**自有**的场景码 —— **定义处就在这张表里**（前端镜像层）。
 *
 * ⚠️ **形状是「键就是码本身」**（`"E_CTX-01": "E_CTX-01"`）⇒ ⇒ 工具
 * （`tools/error_code_check.py`）既能从表里**抽出码集合**，又能把
 * 「前端常量表里的条目」与「Rust 侧的定义」**分开记账** ⇒ ⇒ 「唯一定义」
 * 不变式（台账 D-120）对前端码**真正生效**。
 *
 * ⚠️ **只放前端自产的码** ⇒ ⇒ 契约层 / 内核 crate 的码**不许**出现在这里
 * ⇒ ⇒ 那是**镜像** ⇒ ⇒ 镜像表见下面 ⇒ ⇒ 两处定义会破「唯一定义」。
 *
 * ⚠️ **前端服务层不许再写裸字面量** ⇒ ⇒ 每个码都从这里取 ⇒ ⇒ 改一个码的
 * 拼写只改这一处。
 */
export const FRONT_ERROR_CODES = Object.freeze({
    "E_CTX-01": "E_CTX-01",
    "E_CTX-06": "E_CTX-06",
    "E_CTX-11": "E_CTX-11",
    "E_TRX-01": "E_TRX-01",
    "E_TRX-02": "E_TRX-02",
    "E_TRX-03": "E_TRX-03",
    "E_TRX-04": "E_TRX-04",
    "E_TRX-05": "E_TRX-05",
    "E_STORE-01": "E_STORE-01",
    "E_STORE-02": "E_STORE-02",
    "E_STORE-03": "E_STORE-03",
});

/**
 * **契约层镜像**的场景码 —— 本批采纳的那一个。
 *
 * ⚠️ **这里不是定义处** ⇒ ⇒ 定义处在 `crates/contract/src/error.rs`
 * （`E_CTX_05_ADMIN_CHANNEL_DISABLED`）⇒ ⇒ 这张表只给前端一个**可引用的
 * 名字** ⇒ ⇒ 「唯一定义」判据**不会**因为它多算一处。
 *
 * ⚠️ **不许**与 `FRONT_ERROR_CODES` 重叠（裁决 E / 验收 C-3）⇒ ⇒
 * `E_CTX-05` 只在这里、不在自有表里。
 */
export const MIRRORED_ERROR_CODES = Object.freeze({
    "E_CTX-05": "E_CTX-05",
});

/**
 * **Rust 侧**（契约层）已定义、前端**按码分派**时要用到的其余码 ——
 * **只是「引用用的名字」** ⇒ ⇒ **不是定义处**（定义处仍在 Rust 侧）。
 *
 * ⚠️ **为什么单列一张表**：`MIRRORED_ERROR_CODES` 装的是**本批**采纳的那个
 * 契约层镜像（`E_CTX-05`）；这张表装的是**其余**契约层镜像 ⇒ ⇒ 两张表
 * 合起来 = 前端用到的**全部** Rust 侧码 ⇒ ⇒ 而**前端自产**的码在
 * `FRONT_ERROR_CODES` 里 ⇒ ⇒ 三张表**互不重叠**。
 *
 * ⚠️ **内核 crate** 自有的码**不许**进这张表 ⇒ ⇒ 前端服务层一旦**用到**
 * 它，工具的「前端 × 内核撞号」判据就会判红（台账 D-157）⇒ ⇒ 那类码在
 * 前端只以**散文**出现。
 */
export const RUST_ERROR_CODE_REFS = Object.freeze({
    "E_DBG-01": "E_DBG-01",
    // ⚠️ 契约 v9 新增的三个码（`crates/contract/src/error.rs`）：
    //   `E_DBG-02` = 条件表达式解析失败；
    //   `E_DBG-03` = 转储长度越界；
    //   `E_DBG-04` = 符号名为空（⚠️ **整合批拆出**：规格原文让「符号名为
    //   空」也用 `E_DBG-02` ⇒ ⇒ 那是**一码两义** ⇒ ⇒ 「一场景一码」⇒ ⇒
    //   ⇒ 拆成独立的一个码）。
    // ⇒ ⇒ 前端**按码分派**（把内核原文显示到对应的那一位）⇒ ⇒ 这三个码
    // **前端要用到** ⇒ ⇒ 落在「引用用的名字」这张表里（不是定义处）。
    "E_DBG-02": "E_DBG-02",
    "E_DBG-03": "E_DBG-03",
    "E_DBG-04": "E_DBG-04",
    "E_PLT-13": "E_PLT-13",
    "E_PROTO-01": "E_PROTO-01",
    "E_PROTO-02": "E_PROTO-02",
    "E_PROTO-04": "E_PROTO-04",
    "E_PROTO-06": "E_PROTO-06",
    "E_PROTO-08": "E_PROTO-08",
});

/**
 * Largest virtual time (picoseconds) that survives a round trip through a
 * JS number without losing a single bit — 2^53.
 *
 * Beyond it an IEEE-754 double can no longer represent every integer, so
 * reading such a timestamp back would **silently** round it (contract v2
 * spec §2.2). `psToNumber()` refuses instead of rounding.
 */
export const MAX_EXACT_PS = 9007199254740992; // 2 ** 53

/** `protocol::Direction` — serde unit variants. */
export const DIRECTION = Object.freeze({
    Command: "Command",
    Event: "Event",
});

/** `model::WriteSize` — serde unit variants. */
export const WRITE_SIZE = Object.freeze({
    Byte: "Byte",
    Half: "Half",
    Word: "Word",
});

/** `protocol::HaltReason` — serde unit variants. */
export const HALT_REASON = Object.freeze({
    Breakpoint: "Breakpoint",
    Watchpoint: "Watchpoint",
    EndOfExecution: "EndOfExecution",
    Trap: "Trap",
    UserPause: "UserPause",
    Watchdog: "Watchdog",
});

/** `model::WatchAccess` — serde unit variants. */
export const WATCH_ACCESS = Object.freeze({
    Read: "Read",
    Write: "Write",
    Either: "Either",
});

/** `model::ImageKind` — serde unit variants. */
export const IMAGE_KIND = Object.freeze({
    Elf: "Elf",
    RawHex: "RawHex",
});

/**
 * `model::TraceSource` — which buffer an export reads from.
 *
 * `Waveform` reads M-19's edge ring; `Execution` reads M-11's own
 * instruction/memory trace ring. The **host** routes between them, so
 * neither module depends on the other.
 */
export const TRACE_SOURCE = Object.freeze({
    Waveform: "Waveform",
    Execution: "Execution",
});

/**
 * `model::TraceFormat` — export format.
 *
 * ⚠️ Not every combination is legal: `Vcd` is waveform-only (VCD models
 * *signals changing over time*), `Json` is execution-only. An illegal
 * combination is rejected by the kernel with `E_CTX-03`.
 */
export const TRACE_FORMAT = Object.freeze({
    Csv: "Csv",
    Vcd: "Vcd",
    Json: "Json",
});

/**
 * Event variant → topic name. The topic is a **front-end** concept (M-05
 * subscription key); the variant names stay byte-identical to the Rust
 * contract.
 */
export const EVENT_TOPIC = Object.freeze({
    Tick: "tick",
    Halted: "halted",
    Error: "error",
    MemoryRead: "memory.read",
    // 契约 v6 新增：当前生效的板级内存图（前端内存映射表的数据源）。
    MemoryMap: "memory.map",
    Registers: "sim.registers",
    Negotiated: "sim.negotiated",
    SimStateDelta: "sim.state.delta",
    WaveformHistory: "waveform.history",
    SnapshotCreated: "snapshot.created",
    LinkUp: "link.up",
    LinkDown: "link.down",
    Disasm: "debug.disasm",
    SourceLocation: "debug.source",
    TraceExport: "debug.export",
    TraceState: "debug.trace",
    // 契约 v4 新增：guest 的 console（UART TX）字节流。
    Console: "sim.console",
    // 契约 v8 新增：管理员通道的**回复侧**（D-129）。
    AdminLogs: "admin.logs",
    AdminDiag: "admin.diag",
    // 契约 v9 新增：调试器增强批（符号查询 + 整段转储）。
    SymbolResolved: "debug.symbol",
    MemoryDump: "debug.dump",
});

/**
 * 管理员通道的两个 topic —— **派生**自 `EVENT_TOPIC` ⇒ ⇒ **不是第二份真相**
 * ⇒ ⇒ 加一个 admin 事件只改 `EVENT_TOPIC` 一处。
 *
 * ⚠️ 订阅它们**不需要**改 M-05 的路由表（STAGE-29 裁决 K）：回包是**按
 * `reqId`** 匹配的 ⇒ ⇒ 新增事件变体只是多了一个 topic ⇒ ⇒ 通用分发路径
 * 已经覆盖。
 */
export const ADMIN_TOPICS = Object.freeze([
    EVENT_TOPIC.AdminLogs,
    EVENT_TOPIC.AdminDiag,
]);

/** Topic that receives every event. */
export const TOPIC_ALL = "*";

/**
 * Build a protocol error. `code` follows the `E_<FAMILY>-<SCENARIO>`
 * format of `riscv_sim_contract::error`.
 *
 * @param {string} code
 * @param {string} message
 * @returns {Error & {code: string}}
 */
export function protocolError(code, message) {
    const err = new Error(`${code}: ${message}`);
    /** @type {any} */ (err).code = code;
    return err;
}

// ---------------------------------------------------------------------------
// 虚拟时间的线上表示（契约 v2 · 规格 §2）
// ---------------------------------------------------------------------------

/**
 * 把一个虚拟时间量编码成线上表示（十进制字符串）。
 *
 * 凡是「随仿真时长增长」的量（虚拟时间）都走字符串：JSON 数字只能精确
 * 表示到 2^53，而 2^53 ps ≈ 2.5 小时的仿真时间——**可达**，不是理论值。
 *
 * ⚠️ **非安全整数走 BigInt 路径**：`String(1e21)` 给出的是指数记法
 * `"1e+21"`——那**不是**十进制整数字符串，`psToNumber` 会把它判成「形态
 * 非法」（E_PROTO-08）。但 1e21 > 2^53，按规格 §2.4 它应该是「输入超限」
 * （E_PROTO-04）——**错误的分类**会让「超限」与「畸形」两件事混在一起。
 * 所以这里的产出必须是**纯十进制**：让 `psToNumber` 走到超限分支。
 *
 * 常规路径（安全整数）**行为不变**。
 *
 * @param {number|bigint|string} value
 * @returns {string}
 */
export function ps(value) {
    if (typeof value === "bigint") return value.toString();
    if (typeof value === "number") {
        if (Number.isSafeInteger(value)) return String(value);
        // 整数但**不是**安全整数（|value| > 2^53）：BigInt 给出**纯十进制**
        // 的精确值（`BigInt` 接受任何整数值的 double）。
        if (Number.isInteger(value)) return BigInt(value).toString();
    }
    return String(value);
}

/**
 * 把线上表示的虚拟时间读回 JS number。
 * 超过 2^53 时**抛错**（E_PROTO-04）而不是静默丢精度。
 *
 * ⚠️ 这里**不能**用 `Number(text)` 了事：`Number("9007199254740993")` 会
 * 变成 `9007199254740992`——**静默**丢掉 1 ps，正是本规则要消灭的行为。
 * 所以字符串形态走 `BigInt` **精确**比较，只有确认 ≤ 2^53 才转成 number。
 *
 * 数字形态（旧的线上表示）**接受**，但精度可能早已丢失——文档口径是
 * 「数字形态只保证 ≤ 2^53 精确」。
 *
 * ⚠️ **错误码**：超限用 **`E_PROTO-04`**（输入超限：数值的大小超过了
 * 消费者能精确表示的上限）——**不是** `E_PROTO-02`。后者的原义是
 * 「未知字段」，在契约里恰恰是**不抛错**的（只增不改，仅记一条日志），
 * 拿一个「仅日志」的码去抛错语义是反的。形态非法（非有限数 / 非整数 /
 * 负数 / 不是十进制整数字符串）仍然是 `E_PROTO-08`（消息格式错误）。
 *
 * @param {unknown} text
 * @returns {number}
 * @throws {Error & {code: string}} E_PROTO-04（超过 2^53）/ E_PROTO-08（形态非法）
 */
export function psToNumber(text) {
    if (typeof text === "number") {
        if (!Number.isFinite(text)) {
            throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], `虚拟时间不是有限数：${String(text)}`);
        }
        if (!Number.isInteger(text)) {
            throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], `虚拟时间不是整数：${String(text)}`);
        }
        if (text < 0) {
            throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], `虚拟时间为负：${String(text)}`);
        }
        if (text > MAX_EXACT_PS) {
            throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-04"], `虚拟时间超过 2^53，无法精确表示：${String(text)}`);
        }
        return text;
    }
    if (typeof text === "string") {
        if (!/^\d+$/.test(text)) {
            throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], `虚拟时间不是十进制整数字符串：${text}`);
        }
        const exact = BigInt(text);
        if (exact > BigInt(MAX_EXACT_PS)) {
            throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-04"], `虚拟时间超过 2^53，无法精确表示：${text}`);
        }
        return Number(exact);
    }
    throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], `虚拟时间形态非法：${String(text)}`);
}

// ---------------------------------------------------------------------------
// Command payloads — `protocol::CommandPayload`
// ---------------------------------------------------------------------------

/**
 * Command payload builders. Each builder returns the **inner** payload
 * value (the value serde would put behind the `Command` tag), never the
 * full envelope — `commandMessage()` wraps it.
 */
export const Command = Object.freeze({
    /** `CommandPayload::Nop` — heartbeat. */
    nop: () => "Nop",
    /** `CommandPayload::LoadProject(Project)` — load + reset the engine. */
    loadProject: (project) => ({ LoadProject: project }),
    /** `CommandPayload::Continue` — run to the next stop condition. */
    continueRun: () => "Continue",
    /** `CommandPayload::StepN { n }` — run exactly `n` instructions. */
    stepN: (n) => ({ StepN: { n: Number(n) } }),
    /**
     * `CommandPayload::Pause` — stop the engine at the next **instruction**
     * boundary.
     *
     * ⚠️ v1 的措辞写的是「next **quantum** boundary」，契约层已订正掉
     * （`crates/contract/src/protocol.rs`）：宿主调用是**同步**的，
     * `pause()` 必然发生在批次**之外**；「量子边界」不是它能停的地方。
     * 实现落在 M-17 的 `pause_requested` 标志位上，在**下一条指令边界**
     * 停下（STAGE-4 裁决 C · 台账 D-59）。
     */
    pause: () => "Pause",
    /** `CommandPayload::SetBreakpoint(Breakpoint)`. */
    setBreakpoint: (breakpoint) => ({ SetBreakpoint: breakpoint }),
    /** `CommandPayload::ClearBreakpoint { id }`. */
    clearBreakpoint: (id) => ({ ClearBreakpoint: { id: Number(id) } }),
    /** `CommandPayload::SetWatchpoint(Watchpoint)`. */
    setWatchpoint: (watchpoint) => ({ SetWatchpoint: watchpoint }),
    /** `CommandPayload::ClearWatchpoint { id }`. */
    clearWatchpoint: (id) => ({ ClearWatchpoint: { id: Number(id) } }),
    /** `CommandPayload::ReadMemory { addr, size }`. */
    readMemory: (addr, size) => ({ ReadMemory: { addr: Number(addr), size } }),
    /** `CommandPayload::WriteMemory { addr, size, value }`. */
    writeMemory: (addr, size, value) => ({
        WriteMemory: { addr: Number(addr), size, value: Number(value) >>> 0 },
    }),
    /**
     * `CommandPayload::ReadRegisters { index, count }` — 读一段通用寄存器。
     *
     * 内核侧把 `index` / `count` **钳到** 32 个寄存器的范围内，回复带的是
     * **实际**起点（规格 §5）——所以调用方**不能**假设回复的起点等于
     * 请求的起点。
     */
    readRegisters: (index, count) => ({
        ReadRegisters: { index: Number(index), count: Number(count) },
    }),
    /**
     * `CommandPayload::WriteRegister { index, value }` — 写一个通用寄存器。
     *
     * 写 x0 被**丢弃**（D-04，但**仍回复**）；`index > 31` 是**拒绝**
     * （E_DBG-01）而不是钳位——写钳位会**静默**写到别的寄存器。
     */
    writeRegister: (index, value) => ({
        WriteRegister: { index: Number(index), value: Number(value) >>> 0 },
    }),
    /**
     * `CommandPayload::Negotiate { min_schema_version, cap_bits }` — 版本协商。
     *
     * 宿主报的是它**能说的最低**版本；内核回它**定下来**的版本。
     */
    negotiate: (minSchemaVersion, capBits) => ({
        Negotiate: {
            min_schema_version: Number(minSchemaVersion),
            cap_bits: Number(capBits),
        },
    }),
    /** `CommandPayload::Subscribe { topic }` — idempotent. */
    subscribe: (topic) => ({ Subscribe: { topic: String(topic) } }),
    /** `CommandPayload::Unsubscribe { topic }` — idempotent. */
    unsubscribe: (topic) => ({ Unsubscribe: { topic: String(topic) } }),
    /** `CommandPayload::Snapshot` — snapshot the current SimState. */
    snapshot: () => "Snapshot",
    /** `CommandPayload::Restore { state }`. */
    restore: (state) => ({ Restore: { state } }),
    /** `CommandPayload::Admin { op }` — admin channel only. */
    admin: (op) => ({ Admin: { op } }),
    /**
     * `CommandPayload::Disasm { addr, count }` — 反汇编一段指令（M-11）。
     *
     * 回复是 `Disasm` 事件。序列可能**短于** `count`（走到未映射地址 ⇒
     * 提前结束 = 截断，**不是**错误）。
     */
    disasm: (addr, count) => ({ Disasm: { addr: Number(addr), count: Number(count) } }),
    /**
     * `CommandPayload::ResolveLine { addr }` — 地址 → 源码行（M-11）。
     *
     * 回复有**两种「没有答案」**：`file` 空且 `line === 0` = 根本没有
     * DWARF；`file` 空且 `line === 0xFFFFFFFF` = 有 DWARF 但地址没命中
     * 行表。
     */
    resolveLine: (addr) => ({ ResolveLine: { addr: Number(addr) } }),
    /**
     * `CommandPayload::ExportTrace { source, format, offset, limit }` — 导出。
     *
     * `source` 选数据源、`format` 选序列化器；**非法组合会被拒绝**
     * （`Vcd` 只对 `Waveform` 合法，`Json` 只对 `Execution` 合法）。
     * `offset` 是**条目偏移**，续传时传上次回复的 `next_offset`。
     */
    exportTrace: (source, format, offset, limit) => ({
        ExportTrace: {
            source,
            format,
            offset: Number(offset),
            limit: Number(limit),
        },
    }),
    /**
     * `CommandPayload::SetTraceEnabled { enabled }` — 执行轨迹记录开关。
     *
     * 关闭时内核**零开销**（什么都不记）；开启且仿真在跑时前端显示降速提示。
     */
    setTraceEnabled: (enabled) => ({ SetTraceEnabled: { enabled: Boolean(enabled) } }),
    /**
     * `CommandPayload::WriteConsole { bytes }` — 往 console UART **写**
     * （RX 方向，契约 v5）。
     *
     * ⚠️ **字节，不是字符串**：与 `Event.console`（TX 方向）**同一个
     * 口径** —— guest 的字节流两个方向都是真相 ⇒ 线上形态是 **JSON
     * 数字数组**（`[104, 105]`），**不是** base64、**不是**字符串。
     *
     * ⚠️ **传字符串是错误用法，这里不默默接受**：字节必须是 **0..255
     * 的整数数组**。传进来一个字符串（`'hi'`）会**当场抛错**，而不是
     * 被逐字符拆成 `[104, 105]`（那是**另一个东西**，不是用户的意图）。
     *
     * @param {Array<number>} bytes
     * @returns {{WriteConsole: {bytes: number[]}}}
     * @throws {Error & {code: string}} E_PROTO-08（字节形态非法）
     */
    writeConsole: (bytes) => {
        if (!Array.isArray(bytes)) {
            throw protocolError(
                RUST_ERROR_CODE_REFS["E_PROTO-08"],
                `WriteConsole 的 bytes 必须是字节数组（0..255 的整数），收到：${typeof bytes}`,
            );
        }
        return {
            WriteConsole: {
                bytes: bytes.map((b) => {
                    const n = Number(b);
                    if (!Number.isInteger(n) || n < 0 || n > 255) {
                        throw protocolError(
                            RUST_ERROR_CODE_REFS["E_PROTO-08"],
                            `WriteConsole 的字节必须是 0..255 的整数：${String(b)}`,
                        );
                    }
                    return n;
                }),
            },
        };
    },
    /**
     * `CommandPayload::SetMemoryMap { description, expected_revision }` —
     * 设置板级内存图（用户在前端配的那张板，契约 v6 · **v7 多了
     * `expected_revision`**）。
     *
     * ⚠️ **携带的是一整份描述**，不是 patch ⇒ 没有「当前值 + 补丁」的
     * 第二个真相。
     *
     * ⚠️ **零新 DTO**：描述用的是契约层**已有**的 `PlatformDescription`
     * ⇒ 描述 DTO **一字未改**。本函数**原样透传**它，不重写字段名。
     *
     * ⚠️ **`expected_revision`（v7 · D-137）**：前端的**期望修订号** ⇒
     * `null` ⇒ **不检查**（旧调用方的行为）；给了数字 ⇒ 宿主发现它与
     * 当前的修订号不符 ⇒ **明确拒绝**（`E_PLT-13`）且**什么都不动**
     * ⇒ 并发丢更新**看得见**。
     *
     * 回复是 `MemoryMap` 事件（当前生效的图 + `pending` / `loaded` /
     * `revision`）。
     *
     * @param {object} description — `PlatformDescription`（契约层已有的 DTO）
     * @param {number|null} [expectedRevision] — 期望修订号（默认 `null` = 不检查）
     * @returns {{SetMemoryMap: {description: object, expected_revision: number|null}}}
     */
    setMemoryMap: (description, expectedRevision = null) => ({
        SetMemoryMap: {
            description,
            expected_revision: expectedRevision ?? null,
        },
    }),
    /**
     * `CommandPayload::PatchMemoryMap { ops, expected_revision }` — 板级
     * 内存图的**差量**更新（契约 v7 · D-137）。
     *
     * ⚠️ **一条命令一次合成一次校验一次应用** ⇒ 不会半张图。
     *
     * ⚠️ **`ops` 的线上形状**：`MemoryMapOp` 用 serde 的**外部标签**
     * ⇒ 键**就是 Rust 变体名**：
     *   `{"UpsertRegion": {"spec": {"kind": "ram", "base": 0, "size": 1048576}}}`
     *   `{"RemoveRegion": {"base": 0}}`
     *   `{"SetBudget": {"region_budget_bytes": 16777216, "max_region_bytes": null}}`
     * ⇒ ⚠️ **不是** `{"op": "upsert_region", ...}`。
     *
     * @param {Array<object>} ops — 差量操作（按顺序施加）
     * @param {number|null} [expectedRevision] — 期望修订号（默认 `null` = 不检查）
     * @returns {{PatchMemoryMap: {ops: Array<object>, expected_revision: number|null}}}
     */
    patchMemoryMap: (ops, expectedRevision = null) => ({
        PatchMemoryMap: {
            ops: ops ?? [],
            expected_revision: expectedRevision ?? null,
        },
    }),
    /**
     * `CommandPayload::UnloadProject` — **卸载工程**（契约 v7 · D-139）。
     *
     * ⚠️ **unit 变体** ⇒ 线上形态是**裸字符串** `"UnloadProject"`
     * （与 `nop` / `continueRun` 同形 ⇒ 不是 `{UnloadProject: {}}`）。
     *
     * ⚠️ 卸载 = **清运行态**（寄存器 / 事件队列 / 断点 / 观察点）⇒
     * **镜像字节与用户的板都留着** ⇒ 重新装载**不必重新选文件**。
     *
     * @returns {"UnloadProject"}
     */
    unloadProject: () => "UnloadProject",
    /**
     * `CommandPayload::ResolveSymbol { name }` — 按**名字**查符号（契约 v9）。
     *
     * ⚠️ **一条命令服务两种用法**（契约 v9 §3 裁决 B）：
     *   - **精确命中** ⇒ 回复的 `SymbolResolved.symbol` 里有它；
     *   - **只给前缀** ⇒ 回复的 `SymbolResolved.candidates` 里有候选。
     *
     * ⚠️ **键名逐字对齐 Rust**（`name`）⇒ ⇒ 跨层门禁会比。
     *
     * @param {string} name — 符号名（**空** ⇒ 内核拒 `E_DBG-04`）
     * @returns {{ResolveSymbol: {name: string}}}
     */
    resolveSymbol: (name) => ({ ResolveSymbol: { name: String(name) } }),
    /**
     * `CommandPayload::DumpMemory { addr, len }` — 转储一段内存（契约 v9）。
     *
     * ⚠️ **原样拒，不静默截断**（契约 v9 §3 裁决 D）：`len == 0` 或
     *   `len > DUMP_MAX_BYTES` ⇒ ⇒ 内核回 `E_DBG-03` ⇒ ⇒ 前端**照样要把
     *   内核原文显示出来**（不吞、不改写）。
     *
     * @param {number} addr — 起始**物理**地址
     * @param {number} len — 字节数（`1..=DUMP_MAX_BYTES`）
     * @returns {{DumpMemory: {addr: number, len: number}}}
     */
    dumpMemory: (addr, len) => ({ DumpMemory: { addr: Number(addr), len: Number(len) } }),
});

// ---------------------------------------------------------------------------
// Event payloads — `protocol::EventPayload`
// ---------------------------------------------------------------------------

/**
 * `model::DeltaRegister { index, value }` — one changed register inside
 * `EventPayload::SimStateDelta` (contract v2 spec §4.4).
 *
 * The DTO is **nested** inside the event payload, so the contract gate
 * has to look at it explicitly: the variant-level field check only sees
 * the **outer** `registers` field and never descends into the elements
 * (F3).
 *
 * @param {number} index — register index (0..=31)
 * @param {number} value — the new value
 * @returns {{index: number, value: number}}
 */
export function deltaRegister(index, value) {
    return { index: Number(index), value: Number(value) >>> 0 };
}

/**
 * `model::DisasmEntryDto` — 一条反汇编条目（`addr` / `code` / `mnemonic` /
 * `width` / `file` / `line`）。
 *
 * The DTO is **nested** inside the `Disasm` event payload, so the contract
 * gate has to look at it explicitly: the variant-level field check only sees
 * the **outer** `entries` field and never descends into the elements.
 *
 * ⚠️ **v0.20 检视 B · D-3**：此前这条门禁**根本比不到**元素的字段名
 * （`Event.disasm([])` 传的是**空数组**）⇒ 把 Rust 侧的 `mnemonic` 改名成
 * `mnem`（同步改 Rust 自测），**前端门禁 228/228 全绿** ⇒ Rust 改而 JS
 * 忘改 ⇒ 线上字段名不匹配 ⇒ **静默**。
 * ⇒ 门禁现在喂**非空**数组，元素字段名被钉住。
 *
 * ⚠️ **契约 v4（D-102）新增两个可选字段**：`file`（源码文件）与 `line`
 * （1 起的行号）。它们是 `Option` ⇒ 线上形态是 **`null`**（serde 默认），
 * **不是**空串、**不是** 0。`null` 的含义是「**没有**行表 / 没命中」——
 * **绝不**伪造一个文件名或行号出来。
 *
 * @param {{addr?: number, code?: number, mnemonic?: string, width?: number, file?: string|null, line?: number|null}} e
 * @returns {{addr: number, code: number, mnemonic: string, width: number, file: string|null, line: number|null}}
 */
export function disasmEntry(e) {
    return {
        addr: Number(e?.addr),
        code: Number(e?.code) >>> 0,
        mnemonic: String(e?.mnemonic ?? ""),
        width: Number(e?.width),
        file: e?.file === null || e?.file === undefined ? null : String(e.file),
        line: e?.line === null || e?.line === undefined ? null : Number(e.line),
    };
}

/** Event payload builders. Same shape rule as `Command`. */
export const Event = Object.freeze({
    /** `EventPayload::Tick { elapsed_ps, pc }` — `elapsed_ps` 走十进制字符串。 */
    tick: (elapsedPs, pc) => ({ Tick: { elapsed_ps: ps(elapsedPs), pc: Number(pc) } }),
    /** `EventPayload::Halted { reason, pc }`. */
    halted: (reason, pc) => ({ Halted: { reason, pc: Number(pc) } }),
    /** `EventPayload::Error { code, message }`. */
    error: (code, message) => ({ Error: { code: String(code), message: String(message) } }),
    /** `EventPayload::MemoryRead { addr, size, value }`. */
    memoryRead: (addr, size, value) => ({
        MemoryRead: { addr: Number(addr), size, value: Number(value) >>> 0 },
    }),
    /**
     * `EventPayload::Registers { index, values }` — 一批通用寄存器值。
     *
     * v2 之前契约**没有**寄存器载体，前端只能从边沿编码的波形流里「读」
     * 寄存器——那把「信号历史」和「寄存器快照」混成了一件事。
     */
    registers: (index, values) => ({
        Registers: {
            index: Number(index),
            values: (values ?? []).map((v) => Number(v) >>> 0),
        },
    }),
    /** `EventPayload::Negotiated { schema_version, cap_bits, kernel }`. */
    negotiated: (schemaVersion, capBits, kernel) => ({
        Negotiated: {
            schema_version: Number(schemaVersion),
            cap_bits: Number(capBits),
            kernel: String(kernel),
        },
    }),
    /**
     * `EventPayload::SimStateDelta { registers, pc, t_ps }` — 增量状态更新。
     *
     * `pc` 是 `Option<u32>`：没有变化时是 `null`。`t_ps` 走十进制字符串。
     */
    simStateDelta: (registers, pc, tPs) => ({
        SimStateDelta: {
            registers: (registers ?? []).map((r) => deltaRegister(r?.index, r?.value)),
            pc: pc === null || pc === undefined ? null : Number(pc),
            t_ps: ps(tPs),
        },
    }),
    /** `EventPayload::WaveformHistory(WaveformHistory)`. */
    waveformHistory: (waveform) => ({ WaveformHistory: waveform }),
    /** `EventPayload::SnapshotCreated { id }`. */
    snapshotCreated: (id) => ({ SnapshotCreated: { id: Number(id) } }),
    /** `EventPayload::LinkUp { link_id }`. */
    linkUp: (linkId) => ({ LinkUp: { link_id: String(linkId) } }),
    /** `EventPayload::LinkDown { link_id }`. */
    linkDown: (linkId) => ({ LinkDown: { link_id: String(linkId) } }),
    /**
     * `EventPayload::Disasm { entries }` — 反汇编结果（M-11）。
     *
     * `entries` 可能**短于**请求条数 ⇒ 那是**截断**（走到了未映射地址），
     * 不是错误。
     */
    disasm: (entries) => ({
        Disasm: {
            entries: (entries ?? []).map((e) => disasmEntry(e)),
        },
    }),
    /**
     * `EventPayload::SourceLocation { addr, file, line }` — 地址 → 源码行。
     *
     * ⚠️ **两种「没有答案」要能区分**：`line === 0` = 根本没有 DWARF；
     * `line === 0xFFFFFFFF` = 有 DWARF 但地址没命中行表。
     */
    sourceLocation: (addr, file, line) => ({
        SourceLocation: {
            addr: Number(addr),
            file: String(file ?? ""),
            line: Number(line),
        },
    }),
    /**
     * `EventPayload::TraceExport { source, format, offset, next_offset, total, bytes, chunk, overflow_marked }`.
     *
     * 续传：把 `next_offset` 作为下次 `exportTrace` 的 `offset` 参数传回。
     *
     * ⚠️ `overflowMarked` = **本次导出的窗口不是完整历史**（源环溢出过、
     * 丢过最旧条目）。前端**必须**把它显示出来——否则会把一份被截断的
     * 导出当成完整会话呈现。
     */
    traceExport: (source, format, offset, nextOffset, total, bytes, chunk, overflowMarked) => ({
        TraceExport: {
            source,
            format,
            offset: Number(offset),
            next_offset: Number(nextOffset),
            total: Number(total),
            bytes: Number(bytes),
            chunk: String(chunk ?? ""),
            overflow_marked: Boolean(overflowMarked),
        },
    }),
    /** `EventPayload::TraceState { enabled, len, dropped }` — 轨迹记录状态。 */
    traceState: (enabled, len, dropped) => ({
        TraceState: {
            enabled: Boolean(enabled),
            len: Number(len),
            dropped: Number(dropped),
        },
    }),
    /**
     * `EventPayload::Console { bytes }` — guest 的 console（UART TX）字节流。
     *
     * ⚠️ **字节，不是字符串**：serde 把 `Vec<u8>` 默认序列化成 **JSON 数组**
     * （`[104, 105]`）——**不是** base64、**不是**字符串。guest 可能吐出
     * 任意字节（ANSI 转义、`\r`、半个 UTF-8 序列），**字节流才是真相**
     * （与 Spike 的逐字节比对就是比它），显示层自己去解码。
     *
     * ⚠️ 每个字节被钳到 `0..=255`：线上形态必须是**无符号**字节值。
     */
    console: (bytes) => ({
        Console: {
            bytes: (bytes ?? []).map((b) => Number(b) & 0xff),
        },
    }),
    /**
     * `EventPayload::MemoryMap { description, pending, loaded, revision }` —
     * 当前生效的板级内存图（契约 v6 · **v7 多了 `loaded` 与 `revision`**）。
     *
     * ⚠️ `description` 是**当前生效**的图（**不是**「用户配的那份」）
     * ⇒ 前端内存映射表显示的**就是它**。
     *
     * ⚠️ `pending` = 宿主是否存着一份**待生效**的板级配置 ⇒ 前端要
     * **显式**显示「待生效 —— 重新装载工程后应用」。
     *
     * ⚠️ `loaded` = 有没有装载镜像 ⇒ 决定「改板」是**立即生效**还是
     * **待生效**（裁决 M：它由**内核**说 ⇒ 前端**不复制**任何内核状态）。
     *
     * ⚠️ `revision` = 内存图的**修订号**（单调递增）⇒ 前端下次提交时
     * 把它作为 `expected_revision` 带上 ⇒ 并发丢更新**看得见**
     * （`E_PLT-13`）。
     *
     * @param {object} description — `PlatformDescription`（契约层已有的 DTO）
     * @param {boolean} pending — 有无待生效的板级配置
     * @param {boolean} loaded — 有没有装载镜像
     * @param {number} revision — 当前的修订号（缺省 0）
     * @returns {{MemoryMap: {description: object, pending: boolean, loaded: boolean, revision: number}}}
     */
    memoryMap: (description, pending, loaded, revision) => ({
        MemoryMap: {
            description,
            pending: Boolean(pending),
            loaded: Boolean(loaded),
            revision: Number(revision ?? 0),
        },
    }),
    /**
     * `EventPayload::AdminLogs { entries }` — 管理员通道导出的**日志条目**
     * （契约 v8 · D-129）。
     *
     * ⚠️ **条目原样透传** ⇒ ⇒ 前端**不复制**契约层的 DTO 形状 ⇒ ⇒ 形状的
     * **唯一真相**是 `crates/contract/fixtures/admin-dto.json` ⇒ ⇒ Rust 侧
     * 断言自己的序列化与那份文件逐字相符，前端侧读**同一份**文件
     * （`front/tests/admin-dto.test.mjs`）⇒ ⇒ 任何一侧改了形状，另一侧就红。
     *
     * ⚠️ **条目的两个时间字段是十进制字符串**（契约 v2 §2.2 的 `u64_decimal`
     * 策略）⇒ ⇒ 前端**不**把它们当数字用。
     *
     * @param {Array<object>} entries — `LogEntryDto` 列表（契约层已有的 DTO）
     * @returns {{AdminLogs: {entries: Array<object>}}}
     */
    adminLogs: (entries) => ({ AdminLogs: { entries: entries ?? [] } }),
    /**
     * `EventPayload::AdminDiag { diag }` — 管理员**诊断包**（契约 v8 · D-129）。
     *
     * ⚠️ 挂在 `AdminOp::Custom("diag")` 上 ⇒ ⇒ 契约层**零**新命令。
     *
     * ⚠️ **原样透传**（理由同 `adminLogs`）⇒ ⇒ `AdminDiagDto` 的**四个**字段
     * 由那份 fixture 钉住。
     *
     * @param {object} diag — `AdminDiagDto`（契约层已有的 DTO）
     * @returns {{AdminDiag: {diag: object}}}
     */
    adminDiag: (diag) => ({ AdminDiag: { diag } }),
    /**
     * `EventPayload::SymbolResolved { name, symbol, candidates, total }` —
     * 符号查询的**答复**（契约 v9）。
     *
     * ⚠️ **这是一次查询的答复，不是内核的持久状态** ⇒ ⇒ 前端把它存进
     *   **视图状态**即可 ⇒ ⇒ **不持久化**（否则就成了第二份真相）。
     *
     * ⚠️ **`symbol` 缺席 ≠ 错误**：`null` 的意思是「**没有精确命中**」
     *   （契约 v9 §2.3）⇒ ⇒ 界面要说「没有这个符号」，不是「出错」。
     *
     * ⚠️ **`total == 0`** ⇒ ⇒ 这个镜像**没有符号表**（可能被 strip 了）
     *   ⇒ ⇒ 与「有符号表但没匹配上」**必须能区分**（两态）。
     *
     * @param {string} name — 回显请求里的名字（前端按它对上号）
     * @param {object|null} symbol — 精确命中的那一条（`model::Symbol`）
     * @param {object[]} candidates — 前缀候选（`model::Symbol` 列表）
     * @param {number} total — 该镜像的符号总数
     * @returns {{SymbolResolved: {name: string, symbol: object|null, candidates: object[], total: number}}}
     */
    symbolResolved: (name, symbol, candidates, total) => ({
        SymbolResolved: {
            name: String(name ?? ""),
            symbol: symbol ?? null,
            candidates: candidates ?? [],
            total: Number(total ?? 0),
        },
    }),
    /**
     * `EventPayload::MemoryDump { addr, bytes }` — 一段内存的**字节**
     * （契约 v9）。
     *
     * ⚠️ **字节，不是字符串**：serde 把 `Vec<u8>` 默认序列化成 **JSON 数组**
     *   （`[104, 105]`）——**不是** base64、**不是**字符串 ⇒ ⇒ 与 `Console`
     *   事件**同一个口径**。
     *
     * @param {number} addr — 回显起始地址
     * @param {number[]} bytes — 读到的字节（长度 == 请求的 `len`）
     * @returns {{MemoryDump: {addr: number, bytes: number[]}}}
     */
    memoryDump: (addr, bytes) => ({
        MemoryDump: {
            addr: Number(addr),
            bytes: (bytes ?? []).map((b) => Number(b) & 0xff),
        },
    }),
});

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/**
 * Wrap a command payload into the frozen `Message` envelope.
 *
 * @param {number} seq
 * @param {object|string} payload — inner value from `Command.*`
 * @returns {{schema_version: number, direction: string, seq: number, payload: object}}
 */
export function commandMessage(seq, payload) {
    return {
        schema_version: SCHEMA_VERSION,
        direction: DIRECTION.Command,
        seq: Number(seq),
        payload: { Command: payload },
    };
}

/**
 * Wrap an event payload into the frozen `Message` envelope.
 *
 * @param {number} seq
 * @param {object|string} payload — inner value from `Event.*`
 * @returns {{schema_version: number, direction: string, seq: number, payload: object}}
 */
export function eventMessage(seq, payload) {
    return {
        schema_version: SCHEMA_VERSION,
        direction: DIRECTION.Event,
        seq: Number(seq),
        payload: { Event: payload },
    };
}

/**
 * Decode + validate a `Message` envelope.
 *
 * Unknown fields are **ignored** (the contract's "only add, never change"
 * rule — E_PROTO-02 is a log-only event). A `schema_version` *below* the
 * supported one is rejected (E_PROTO-01, incompatible); a higher one is
 * accepted for forward compatibility.
 *
 * @param {unknown} raw
 * @returns {{schema_version: number, direction: string, seq: number, variant: string, value: any}}
 * @throws {Error & {code: string}} on a malformed envelope
 */
export function decodeMessage(raw) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], "消息不是对象");
    }
    const msg = /** @type {any} */ (raw);
    if (typeof msg.schema_version !== "number" || !Number.isFinite(msg.schema_version)) {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], "schema_version 缺失或非法");
    }
    if (msg.schema_version < SCHEMA_VERSION) {
        throw protocolError(
            RUST_ERROR_CODE_REFS["E_PROTO-01"],
            `协议版本不兼容：${msg.schema_version} < ${SCHEMA_VERSION}`,
        );
    }
    if (msg.direction !== DIRECTION.Command && msg.direction !== DIRECTION.Event) {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], `未知 direction：${String(msg.direction)}`);
    }
    if (typeof msg.seq !== "number" || !Number.isFinite(msg.seq)) {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], "seq 缺失或非法");
    }
    if (!msg.payload || typeof msg.payload !== "object" || Array.isArray(msg.payload)) {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], "payload 缺失或非法");
    }
    const tag = msg.direction === DIRECTION.Command ? "Command" : "Event";
    const other = tag === "Command" ? "Event" : "Command";
    if (!(tag in msg.payload)) {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], `payload 缺少 ${tag} 判别式`);
    }
    if (other in msg.payload) {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], "payload 同时带 Command 与 Event 判别式");
    }
    const inner = msg.payload[tag];
    let variant;
    let value = null;
    if (typeof inner === "string") {
        // Unit variant (`Nop`, `Continue`, `Pause`, `Snapshot`).
        variant = inner;
    } else if (inner && typeof inner === "object" && !Array.isArray(inner)) {
        const keys = Object.keys(inner);
        if (keys.length !== 1) {
            throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], "payload 必须恰好带一个变体判别式");
        }
        variant = keys[0];
        value = inner[variant];
    } else {
        throw protocolError(RUST_ERROR_CODE_REFS["E_PROTO-08"], "payload 变体格式非法");
    }
    return {
        schema_version: msg.schema_version,
        direction: msg.direction,
        seq: msg.seq,
        variant,
        value,
    };
}

/**
 * Topic name of an event variant. Unknown variants map to `"unknown"` —
 * a forward-compatible kernel must not break the dispatcher.
 *
 * @param {string} variant
 * @returns {string}
 */
export function eventTopic(variant) {
    return EVENT_TOPIC[variant] ?? "unknown";
}

/**
 * Build an edge-encoded waveform history (the carrier for probe signals).
 *
 * Mirrors `model::WaveformHistory` — the struct was renamed in contract v2
 * (D-34): the **history** shape is `schema_version + streams[]`, while the
 * live-batch shape kept the name `model::WaveformDelta`.
 *
 * ⚠️ `t_ps` travels as a **decimal string** (contract v2 §2.2) — a virtual
 * time that grows with the run length must not be a JSON number.
 *
 * @param {Array<{id: string, label: string, edges: Array<{t_ps: number, value: number}>}>} streams
 * @param {number} [schemaVersion] — the **edge format** version (`WAVEFORM_SCHEMA_VERSION`）
 * @returns {{schema_version: number, streams: any[]}}
 */
export function waveform(streams, schemaVersion = 1) {
    return {
        schema_version: schemaVersion,
        streams: streams.map((s) => ({
            id: String(s.id),
            label: String(s.label ?? s.id),
            edges: (s.edges ?? []).map((e) => ({ t_ps: ps(e.t_ps), value: Number(e.value) })),
        })),
    };
}
