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

/** Protocol schema version — mirrors `riscv_sim_contract::SCHEMA_VERSION`. */
export const SCHEMA_VERSION = 2;

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
 * Event variant → topic name. The topic is a **front-end** concept (M-05
 * subscription key); the variant names stay byte-identical to the Rust
 * contract.
 */
export const EVENT_TOPIC = Object.freeze({
    Tick: "tick",
    Halted: "halted",
    Error: "error",
    MemoryRead: "memory.read",
    Registers: "sim.registers",
    Negotiated: "sim.negotiated",
    SimStateDelta: "sim.state.delta",
    WaveformHistory: "waveform.history",
    SnapshotCreated: "snapshot.created",
    LinkUp: "link.up",
    LinkDown: "link.down",
});

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
 * @param {number|bigint|string} value
 * @returns {string}
 */
export function ps(value) {
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
            throw protocolError("E_PROTO-08", `虚拟时间不是有限数：${String(text)}`);
        }
        if (!Number.isInteger(text)) {
            throw protocolError("E_PROTO-08", `虚拟时间不是整数：${String(text)}`);
        }
        if (text < 0) {
            throw protocolError("E_PROTO-08", `虚拟时间为负：${String(text)}`);
        }
        if (text > MAX_EXACT_PS) {
            throw protocolError("E_PROTO-04", `虚拟时间超过 2^53，无法精确表示：${String(text)}`);
        }
        return text;
    }
    if (typeof text === "string") {
        if (!/^\d+$/.test(text)) {
            throw protocolError("E_PROTO-08", `虚拟时间不是十进制整数字符串：${text}`);
        }
        const exact = BigInt(text);
        if (exact > BigInt(MAX_EXACT_PS)) {
            throw protocolError("E_PROTO-04", `虚拟时间超过 2^53，无法精确表示：${text}`);
        }
        return Number(exact);
    }
    throw protocolError("E_PROTO-08", `虚拟时间形态非法：${String(text)}`);
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
    /** `CommandPayload::Pause` — pause at the next quantum boundary. */
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
});

// ---------------------------------------------------------------------------
// Event payloads — `protocol::EventPayload`
// ---------------------------------------------------------------------------

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
            registers: (registers ?? []).map((r) => ({
                index: Number(r?.index),
                value: Number(r?.value) >>> 0,
            })),
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
        throw protocolError("E_PROTO-08", "消息不是对象");
    }
    const msg = /** @type {any} */ (raw);
    if (typeof msg.schema_version !== "number" || !Number.isFinite(msg.schema_version)) {
        throw protocolError("E_PROTO-08", "schema_version 缺失或非法");
    }
    if (msg.schema_version < SCHEMA_VERSION) {
        throw protocolError(
            "E_PROTO-01",
            `协议版本不兼容：${msg.schema_version} < ${SCHEMA_VERSION}`,
        );
    }
    if (msg.direction !== DIRECTION.Command && msg.direction !== DIRECTION.Event) {
        throw protocolError("E_PROTO-08", `未知 direction：${String(msg.direction)}`);
    }
    if (typeof msg.seq !== "number" || !Number.isFinite(msg.seq)) {
        throw protocolError("E_PROTO-08", "seq 缺失或非法");
    }
    if (!msg.payload || typeof msg.payload !== "object" || Array.isArray(msg.payload)) {
        throw protocolError("E_PROTO-08", "payload 缺失或非法");
    }
    const tag = msg.direction === DIRECTION.Command ? "Command" : "Event";
    const other = tag === "Command" ? "Event" : "Command";
    if (!(tag in msg.payload)) {
        throw protocolError("E_PROTO-08", `payload 缺少 ${tag} 判别式`);
    }
    if (other in msg.payload) {
        throw protocolError("E_PROTO-08", "payload 同时带 Command 与 Event 判别式");
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
            throw protocolError("E_PROTO-08", "payload 必须恰好带一个变体判别式");
        }
        variant = keys[0];
        value = inner[variant];
    } else {
        throw protocolError("E_PROTO-08", "payload 变体格式非法");
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
