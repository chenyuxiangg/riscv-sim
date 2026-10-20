// Contract-alignment gate (M-07 / M-05 wire format).
//
// The `riscv_sim_contract` crate is **frozen**: the front end's JS
// mirror (`front/lib/message.js`) has to spell the envelope, the
// variant tags and the enum tags exactly like the Rust source. A
// renamed variant would desynchronise the mock kernel from the real
// one — and nothing else in the gate would notice.
//
// So this file parses the Rust contract **out of the repository** and
// compares it with the JS builders. If somebody edits one side only,
// this test goes red.
//
// The comparison covers three levels, because the wire format has three
// levels: the variant **name**, the variant **shape** (unit / newtype /
// struct) and the struct variant's **field names**. Checking only the
// variant name left a blind spot: renaming `ClearBreakpoint { id }` to
// `{ ident }` on the JS side alone desynchronised the wire format
// without turning any test red (F6).

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
    SCHEMA_VERSION,
    DIRECTION,
    WRITE_SIZE,
    HALT_REASON,
    WATCH_ACCESS,
    Command,
    Event,
    commandMessage,
    eventMessage,
    decodeMessage,
    eventTopic,
    waveform,
} from "../lib/message.js";
import { encodeFrame, decodeFrame, FRAME_TYPE } from "../lib/frame.js";
import { buildMockProject } from "../dev/mock-project.js";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const protocolRs = resolve(repoRoot, "crates/contract/src/protocol.rs");
const modelRs = resolve(repoRoot, "crates/contract/src/model.rs");
const hasRust = existsSync(protocolRs) && existsSync(modelRs);

/**
 * Extract the variant names of a Rust `pub enum` from the contract
 * source.
 *
 * @param {string} source
 * @param {string} enumName
 * @returns {string[]}
 */
function rustVariants(source, enumName) {
    const start = source.indexOf(`pub enum ${enumName} {`);
    assert.notEqual(start, -1, `契约里找不到 enum ${enumName}`);
    const body = source.slice(start);
    const end = body.indexOf("\n}");
    assert.notEqual(end, -1, `enum ${enumName} 没有闭合`);
    const variants = [];
    for (const line of body.slice(0, end).split("\n")) {
        // A variant line is indented by exactly four spaces and starts
        // with an upper-case name — doc comments (`///`) and struct
        // fields (`    n: u32,`) do not match.
        const m = /^ {4}([A-Z][A-Za-z0-9]*)\s*(\{|\(|,)/.exec(line);
        if (m) variants.push(m[1]);
    }
    return variants;
}

/**
 * The variant name a JS payload builder produces.
 *
 * @param {object|string} value
 * @returns {string}
 */
function variantOf(value) {
    if (typeof value === "string") return value;
    return Object.keys(value)[0];
}

/**
 * Shape of one Rust enum variant — `unit`, `newtype` or `struct` — plus
 * the **field names** of a struct variant.
 *
 * The variant *name* alone is not enough: renaming a struct field on one
 * side only (e.g. `ClearBreakpoint { id }` → `{ ident }`) desynchronises
 * the wire format just as badly as renaming the variant, and the variant
 * set check cannot see it (F6).
 *
 * The contract crate has no `#[serde(rename = ...)]` attributes, so the
 * Rust field name *is* the wire name.
 *
 * @param {string} source
 * @param {string} enumName
 * @param {string} variantName
 * @returns {{kind: "unit"|"newtype"|"struct", fields: string[]}}
 */
function rustVariantShape(source, enumName, variantName) {
    const start = source.indexOf(`pub enum ${enumName} {`);
    assert.notEqual(start, -1, `契约里找不到 enum ${enumName}`);
    const body = source.slice(start);
    const end = body.indexOf("\n}");
    assert.notEqual(end, -1, `enum ${enumName} 没有闭合`);
    const lines = body.slice(0, end).split("\n");
    // A variant line is indented by exactly four spaces; the negative
    // look-ahead keeps `StepN` from matching a hypothetical `StepNext`.
    const variantRe = new RegExp(`^ {4}${variantName}(?![A-Za-z0-9_])`);
    const index = lines.findIndex((line) => variantRe.test(line));
    assert.notEqual(index, -1, `enum ${enumName} 里找不到变体 ${variantName}`);
    const line = lines[index];
    if (line.includes("{")) {
        // Struct variant — the fields are indented by eight spaces and
        // the block ends with a four-space `}`.
        const fields = [];
        for (const l of lines.slice(index + 1)) {
            if (/^ {4}\}/.test(l)) break;
            const m = /^ {8}(?:pub\s+)?([a-z_][A-Za-z0-9_]*)\s*:/.exec(l);
            if (m) fields.push(m[1]);
        }
        return { kind: "struct", fields };
    }
    if (line.includes("(")) return { kind: "newtype", fields: [] };
    return { kind: "unit", fields: [] };
}

/**
 * Field names of a top-level Rust `struct` (the DTOs the newtype
 * variants carry).
 *
 * @param {string} source
 * @param {string} structName
 * @returns {string[]}
 */
function rustStructFields(source, structName) {
    const start = source.indexOf(`pub struct ${structName} {`);
    assert.notEqual(start, -1, `契约里找不到 struct ${structName}`);
    const body = source.slice(start);
    const end = body.indexOf("\n}");
    assert.notEqual(end, -1, `struct ${structName} 没有闭合`);
    const fields = [];
    for (const line of body.slice(0, end).split("\n")) {
        const m = /^ {4}(?:pub\s+)?([a-z_][A-Za-z0-9_]*)\s*:/.exec(line);
        if (m) fields.push(m[1]);
    }
    return fields;
}

describe("Message 信封（与 crates/contract 逐字对齐）", () => {
    test("信封字段名与 Rust Message 完全一致", () => {
        const msg = commandMessage(7, Command.nop());
        assert.deepEqual(Object.keys(msg), ["schema_version", "direction", "seq", "payload"]);
        assert.equal(msg.schema_version, SCHEMA_VERSION);
        assert.equal(msg.direction, "Command");
        assert.equal(msg.seq, 7);
        assert.deepEqual(Object.keys(msg.payload), ["Command"]);
    });

    test("命令方向的判别式是 Command，事件方向是 Event", () => {
        assert.deepEqual(Object.keys(commandMessage(0, Command.nop()).payload), ["Command"]);
        assert.deepEqual(Object.keys(eventMessage(0, Event.tick(0, 0)).payload), ["Event"]);
    });

    test("unit 变体序列化为裸字符串，struct 变体为单键对象", () => {
        // serde 的 externally-tagged 表示：unit 变体没有花括号。
        assert.equal(commandMessage(0, Command.nop()).payload.Command, "Nop");
        assert.deepEqual(commandMessage(0, Command.stepN(3)).payload.Command, { StepN: { n: 3 } });
        assert.deepEqual(eventMessage(0, Event.tick(11, 22)).payload.Event, { Tick: { elapsed_ps: 11, pc: 22 } });
    });

    test("decodeMessage 能还原 variant 与载荷", () => {
        const decoded = decodeMessage(eventMessage(4, Event.memoryRead(0x100, "Word", 7)));
        assert.equal(decoded.direction, "Event");
        assert.equal(decoded.variant, "MemoryRead");
        assert.deepEqual(decoded.value, { addr: 0x100, size: "Word", value: 7 });
        assert.equal(decoded.seq, 4);
    });

    test("decodeMessage 拒绝畸形信封（E_PROTO-08）", () => {
        assert.throws(() => decodeMessage(null), /E_PROTO-08/);
        assert.throws(() => decodeMessage({}), /E_PROTO-08/);
        assert.throws(() => decodeMessage({ ...commandMessage(0, Command.nop()), direction: "Nope" }), /E_PROTO-08/);
        assert.throws(() => decodeMessage({ ...commandMessage(0, Command.nop()), payload: { Event: "Tick" } }), /E_PROTO-08/);
        assert.throws(
            () => decodeMessage({ ...commandMessage(0, Command.nop()), payload: { Command: { A: 1, B: 2 } } }),
            /E_PROTO-08/,
        );
    });

    test("版本过低被拒（E_PROTO-01），版本更高按前向兼容放行", () => {
        const old = { ...commandMessage(0, Command.nop()), schema_version: 0 };
        assert.throws(() => decodeMessage(old), /E_PROTO-01/);
        const newer = { ...commandMessage(0, Command.nop()), schema_version: SCHEMA_VERSION + 1 };
        assert.equal(decodeMessage(newer).schema_version, SCHEMA_VERSION + 1);
    });

    test("未知字段被忽略（只增不改，E_PROTO-02 仅日志）", () => {
        const raw = { ...commandMessage(0, Command.nop()), future_field: 42 };
        assert.equal(decodeMessage(raw).variant, "Nop");
    });

    test("事件 topic 映射覆盖全部事件变体", () => {
        assert.equal(eventTopic("Tick"), "tick");
        assert.equal(eventTopic("Halted"), "halted");
        assert.equal(eventTopic("MemoryRead"), "memory.read");
        assert.equal(eventTopic("WaveformDelta"), "waveform.delta");
        // 前向兼容：未知变体不抛异常。
        assert.equal(eventTopic("SomethingNew"), "unknown");
    });

    test("waveform 构造器对齐 model::Waveform 字段名", () => {
        const w = waveform([{ id: "cpu.gpr.x1", label: "x1", edges: [{ t_ps: 5, value: 42 }] }]);
        assert.deepEqual(Object.keys(w), ["schema_version", "streams"]);
        assert.deepEqual(Object.keys(w.streams[0]), ["id", "label", "edges"]);
        assert.deepEqual(Object.keys(w.streams[0].edges[0]), ["t_ps", "value"]);
    });
});

describe("契约一致性（JS 镜像 vs Rust 源码）", () => {
    test("命令变体集合与 Rust CommandPayload 一致", { skip: !hasRust }, () => {
        const rust = rustVariants(readFileSync(protocolRs, "utf8"), "CommandPayload").sort();
        const js = [
            variantOf(Command.nop()),
            variantOf(Command.loadProject({})),
            variantOf(Command.continueRun()),
            variantOf(Command.stepN(1)),
            variantOf(Command.pause()),
            variantOf(Command.setBreakpoint({})),
            variantOf(Command.clearBreakpoint(1)),
            variantOf(Command.setWatchpoint({})),
            variantOf(Command.clearWatchpoint(1)),
            variantOf(Command.readMemory(0, "Word")),
            variantOf(Command.writeMemory(0, "Word", 0)),
            variantOf(Command.subscribe("t")),
            variantOf(Command.unsubscribe("t")),
            variantOf(Command.snapshot()),
            variantOf(Command.restore({})),
            variantOf(Command.admin({})),
        ].sort();
        assert.deepEqual(js, rust, "JS 命令变体与 Rust CommandPayload 不一致");
    });

    test("事件变体集合与 Rust EventPayload 一致", { skip: !hasRust }, () => {
        const rust = rustVariants(readFileSync(protocolRs, "utf8"), "EventPayload").sort();
        const js = [
            variantOf(Event.tick(0, 0)),
            variantOf(Event.halted("Trap", 0)),
            variantOf(Event.error("E_X-01", "m")),
            variantOf(Event.memoryRead(0, "Word", 0)),
            variantOf(Event.waveformDelta({})),
            variantOf(Event.snapshotCreated(1)),
            variantOf(Event.linkUp("l")),
            variantOf(Event.linkDown("l")),
        ].sort();
        assert.deepEqual(js, rust, "JS 事件变体与 Rust EventPayload 不一致");
    });

    test("Direction / WriteSize / HaltReason / WatchAccess 标签逐字一致", { skip: !hasRust }, () => {
        const protocol = readFileSync(protocolRs, "utf8");
        const model = readFileSync(modelRs, "utf8");
        assert.deepEqual(
            Object.values(DIRECTION).sort(),
            rustVariants(protocol, "Direction").sort(),
            "Direction 标签不一致",
        );
        assert.deepEqual(
            Object.values(WRITE_SIZE).sort(),
            rustVariants(model, "WriteSize").sort(),
            "WriteSize 标签不一致",
        );
        assert.deepEqual(
            Object.values(HALT_REASON).sort(),
            rustVariants(protocol, "HaltReason").sort(),
            "HaltReason 标签不一致",
        );
        assert.deepEqual(
            Object.values(WATCH_ACCESS).sort(),
            rustVariants(model, "WatchAccess").sort(),
            "WatchAccess 标签不一致",
        );
    });

    test("每个变体的内层字段名与 Rust 逐字一致（不只是变体名）", { skip: !hasRust }, () => {
        const protocol = readFileSync(protocolRs, "utf8");
        // One entry per variant — the JS builder's *result*, i.e. the
        // value serde would put behind the `Command` / `Event` tag.
        const jsPayloads = {
            CommandPayload: {
                Nop: Command.nop(),
                LoadProject: Command.loadProject({}),
                Continue: Command.continueRun(),
                StepN: Command.stepN(1),
                Pause: Command.pause(),
                SetBreakpoint: Command.setBreakpoint({}),
                ClearBreakpoint: Command.clearBreakpoint(1),
                SetWatchpoint: Command.setWatchpoint({}),
                ClearWatchpoint: Command.clearWatchpoint(1),
                ReadMemory: Command.readMemory(0, "Word"),
                WriteMemory: Command.writeMemory(0, "Word", 0),
                Subscribe: Command.subscribe("t"),
                Unsubscribe: Command.unsubscribe("t"),
                Snapshot: Command.snapshot(),
                Restore: Command.restore({}),
                Admin: Command.admin({}),
            },
            EventPayload: {
                Tick: Event.tick(0, 0),
                Halted: Event.halted("Trap", 0),
                Error: Event.error("E_X-01", "m"),
                MemoryRead: Event.memoryRead(0, "Word", 0),
                WaveformDelta: Event.waveformDelta(waveform([])),
                SnapshotCreated: Event.snapshotCreated(1),
                LinkUp: Event.linkUp("l"),
                LinkDown: Event.linkDown("l"),
            },
        };

        for (const [enumName, payloads] of Object.entries(jsPayloads)) {
            for (const variant of rustVariants(protocol, enumName)) {
                const shape = rustVariantShape(protocol, enumName, variant);
                const value = payloads[variant];
                assert.ok(
                    value !== undefined,
                    `Rust 的 ${enumName}::${variant} 在 JS 侧没有对应构造器`,
                );
                if (shape.kind === "unit") {
                    // A unit variant serialises as a bare string.
                    assert.equal(
                        value,
                        variant,
                        `${enumName}::${variant} 应当序列化成裸字符串`,
                    );
                    continue;
                }
                // A struct / newtype variant is a single-key object.
                assert.deepEqual(
                    Object.keys(value),
                    [variant],
                    `${enumName}::${variant} 应当是单键对象`,
                );
                if (shape.kind === "struct") {
                    assert.deepEqual(
                        Object.keys(value[variant]).sort(),
                        shape.fields.slice().sort(),
                        `${enumName}::${variant} 的字段名与 Rust 不一致`,
                    );
                } else {
                    // A newtype variant carries a whole DTO — the field
                    // names live in that DTO (checked separately below).
                    assert.equal(
                        typeof value[variant],
                        "object",
                        `${enumName}::${variant} 应当携带一个 DTO 对象`,
                    );
                }
            }
        }
    });

    test("newtype 变体携带的 DTO 字段名与 Rust 一致", { skip: !hasRust }, () => {
        const model = readFileSync(modelRs, "utf8");
        const project = buildMockProject();
        // The DTOs the front end actually builds.
        const cases = [
            ["Project", project],
            ["ProductTree", project.tree],
            ["TreeNode", project.tree.root],
            ["ProjectVersion", project.version],
            ["Waveform", waveform([])],
        ];
        for (const [structName, dto] of cases) {
            assert.deepEqual(
                Object.keys(dto).sort(),
                rustStructFields(model, structName).sort(),
                `${structName} 的字段名与 Rust 不一致`,
            );
        }
        // The nested stream / edge shapes travel with the waveform DTO.
        const w = waveform([{ id: "cpu.gpr.x1", label: "x1", edges: [{ t_ps: 5, value: 42 }] }]);
        assert.deepEqual(
            Object.keys(w.streams[0]).sort(),
            rustStructFields(model, "WaveformStream").sort(),
            "WaveformStream 的字段名与 Rust 不一致",
        );
        assert.deepEqual(
            Object.keys(w.streams[0].edges[0]).sort(),
            rustStructFields(model, "Edge").sort(),
            "Edge 的字段名与 Rust 不一致",
        );
    });
});

describe("传输帧（M-07 TransportFrame）", () => {
    test("帧结构含会话、角色与关联 id", () => {
        const frame = encodeFrame({
            type: FRAME_TYPE.Command,
            seq: 3,
            sessionId: "sess-1",
            role: "embedded",
            reqId: 9,
            payload: commandMessage(3, Command.nop()),
        });
        assert.deepEqual(Object.keys(frame), ["type", "seq", "sessionId", "role", "reqId", "payload"]);
        assert.equal(frame.reqId, 9);
    });

    test("帧编解码往返一致", () => {
        const frame = encodeFrame({
            type: FRAME_TYPE.Event,
            seq: 5,
            sessionId: null,
            role: null,
            reqId: null,
            payload: eventMessage(5, Event.tick(1, 2)),
        });
        const decoded = decodeFrame(JSON.parse(JSON.stringify(frame)));
        assert.equal(decoded.type, "event");
        assert.equal(decoded.seq, 5);
        assert.equal(decoded.reqId, null);
        assert.equal(decodeMessage(decoded.payload).variant, "Tick");
    });

    test("畸形帧被拒（E_PROTO-08）", () => {
        assert.throws(() => decodeFrame(null), /E_PROTO-08/);
        assert.throws(() => decodeFrame({ type: "nope", seq: 1, payload: {} }), /E_PROTO-08/);
        assert.throws(() => decodeFrame({ type: "event", seq: 1 }), /E_PROTO-08/);
    });
});
