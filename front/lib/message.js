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

/** Protocol schema version — mirrors `riscv_sim_contract::SCHEMA_VERSION`. */
export const SCHEMA_VERSION = 1;

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
    WaveformDelta: "waveform.delta",
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
    /** `EventPayload::Tick { elapsed_ps, pc }`. */
    tick: (elapsedPs, pc) => ({ Tick: { elapsed_ps: Number(elapsedPs), pc: Number(pc) } }),
    /** `EventPayload::Halted { reason, pc }`. */
    halted: (reason, pc) => ({ Halted: { reason, pc: Number(pc) } }),
    /** `EventPayload::Error { code, message }`. */
    error: (code, message) => ({ Error: { code: String(code), message: String(message) } }),
    /** `EventPayload::MemoryRead { addr, size, value }`. */
    memoryRead: (addr, size, value) => ({
        MemoryRead: { addr: Number(addr), size, value: Number(value) >>> 0 },
    }),
    /** `EventPayload::WaveformDelta(Waveform)`. */
    waveformDelta: (waveform) => ({ WaveformDelta: waveform }),
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
 * Build an edge-encoded waveform delta (the only contract-native carrier
 * for register / signal values — see the review doc, "contract gaps").
 *
 * @param {Array<{id: string, label: string, edges: Array<{t_ps: number, value: number}>}>} streams
 * @param {number} [schemaVersion]
 * @returns {{schema_version: number, streams: any[]}}
 */
export function waveform(streams, schemaVersion = 1) {
    return {
        schema_version: schemaVersion,
        streams: streams.map((s) => ({
            id: String(s.id),
            label: String(s.label ?? s.id),
            edges: (s.edges ?? []).map((e) => ({ t_ps: Number(e.t_ps), value: Number(e.value) })),
        })),
    };
}
