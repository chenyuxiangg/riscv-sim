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
    TRACE_SOURCE,
    TRACE_FORMAT,
    Command,
    Event,
    commandMessage,
    eventMessage,
    decodeMessage,
    eventTopic,
    waveform,
    deltaRegister,
    disasmEntry,
} from "../lib/message.js";
import { encodeFrame, decodeFrame, FRAME_TYPE } from "../lib/frame.js";
import { buildMockProject } from "../dev/mock-project.js";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const protocolRs = resolve(repoRoot, "crates/contract/src/protocol.rs");
const modelRs = resolve(repoRoot, "crates/contract/src/model.rs");
const snapshotRs = resolve(repoRoot, "crates/contract/src/snapshot.rs");
const libRs = resolve(repoRoot, "crates/contract/src/lib.rs");

/**
 * 契约层（Rust）的 `SCHEMA_VERSION` —— **从源码推导**，不抄。
 *
 * ⚠️ **为什么不能抄**（v0.20 检视 B · D-7）：此前这条门禁写死
 * `SCHEMA_VERSION === 3` ⇒ 契约升 v4 时它会**误红**（而错不在 JS 镜像），
 * 于是人要么去改断言、要么把门禁删掉 ⇒ **一个会自己坏的断言**比没有还糟。
 * ⇒ 唯一真相是契约层源码（`tools/wasm-smoke.mjs` 已经这么做了）。
 */
function rustSchemaVersion() {
    const src = readFileSync(libRs, "utf8");
    const m = /pub const SCHEMA_VERSION:\s*u32\s*=\s*(\d+)/.exec(src);
    assert.notEqual(m, null, "契约里读不到 SCHEMA_VERSION");
    return Number(m[1]);
}
const hasRust = existsSync(protocolRs) && existsSync(modelRs);

/**
 * The Rust contract sources must be **present**.
 *
 * ⚠️ **Why this is a hard failure, not a skip** (V-B 检视 · B-6):
 * these six cases used to carry `{ skip: !hasRust }`. `node --test`
 * reports a skipped test as **not failed** and still exits **0** — so
 * a tree without `crates/contract/src/` turned the **whole cross-layer
 * gate** into a silent no-op that still reported "green".
 *
 * That is the exact failure mode this project's reviews keep hitting:
 * the gate looked like it was holding the Rust↔JS mirror in sync, while
 * in fact it was checking nothing at all. `front/` and `crates/` live in
 * the same repository, so "the Rust source is missing" is never a
 * legitimate state here — it means the tree is broken, and a broken
 * tree must **fail**, not skip.
 *
 * Same reasoning as `tools/wasm-smoke.mjs`: "did not run" must never be
 * indistinguishable from "passed".
 */
function requireRustSource() {
    assert.ok(
        hasRust,
        `Rust 契约源码缺失（${protocolRs} / ${modelRs}）——` +
            "对拍门禁必须**失败**，不能静默跳过（静默跳过 = 假绿）",
    );
}

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

/**
 * Every field declaration inside a Rust `struct` or `enum`, together with
 * the `#[serde(...)]` attributes that sit **directly above** it.
 *
 * The other helpers in this file only look at *names* and *shapes*, so
 * they cannot see the **serde representation** of a field. That blind
 * spot is exactly what let the §2 numeric policy drift: dropping the
 * `#[serde(with = "crate::numstr::u64_decimal")]` attribute of a single
 * field puts the Rust side back to a JSON number while this gate stays
 * green — and `numstr::u64_decimal` accepts **both** shapes, so nothing
 * fails at runtime either (F2).
 *
 * The parser is deliberately small: it walks the container body line by
 * line, collecting attributes (they may span several lines) and handing
 * them to the field declaration that follows. Doc comments and blank
 * lines are skipped without dropping the pending attributes.
 *
 * @param {string} source
 * @param {string} containerName — a `struct` or `enum` name
 * @returns {Array<{owner: string, name: string, attributes: string[]}>}
 *   `owner` is the struct name, or the **variant** name for a field of a
 *   struct variant.
 */
function rustFieldDeclarations(source, containerName) {
    const start = source.search(new RegExp(`^pub (?:struct|enum) ${containerName} \\{`, "m"));
    assert.notEqual(start, -1, `契约里找不到 ${containerName}`);
    const body = source.slice(start);
    const end = body.indexOf("\n}");
    assert.notEqual(end, -1, `${containerName} 没有闭合`);
    const lines = body.slice(0, end).split("\n");

    /** @type {Array<{owner: string, name: string, attributes: string[]}>} */
    const decls = [];
    /** @type {string[]} */
    let pending = [];
    /** @type {string|null} */
    let partial = null;
    let owner = containerName;

    for (const line of lines) {
        const trimmed = line.trim();
        // -- an attribute, possibly spanning several lines -----------------
        if (partial !== null) {
            partial += ` ${trimmed}`;
            if (trimmed.includes("]")) {
                pending.push(partial);
                partial = null;
            }
            continue;
        }
        if (trimmed.startsWith("#[")) {
            if (trimmed.includes("]")) pending.push(trimmed);
            else partial = trimmed;
            continue;
        }
        // -- doc comments / blank lines keep the pending attributes -------
        if (trimmed === "" || trimmed.startsWith("//")) continue;
        // -- an enum variant: it owns the fields that follow --------------
        const variant = /^ {4}([A-Z][A-Za-z0-9]*)\s*(\{|\(|,)/.exec(line);
        if (variant) {
            owner = variant[1];
            pending = [];
            continue;
        }
        // -- a field of a struct variant (indented by eight spaces) -------
        const nested = /^ {8}(?:pub\s+)?([a-z_][A-Za-z0-9_]*)\s*:/.exec(line);
        if (nested) {
            decls.push({ owner, name: nested[1], attributes: pending });
            pending = [];
            continue;
        }
        // -- a field of a top-level struct (indented by four spaces) ------
        const top = /^ {4}(?:pub\s+)?([a-z_][A-Za-z0-9_]*)\s*:/.exec(line);
        if (top) {
            decls.push({ owner: containerName, name: top[1], attributes: pending });
            pending = [];
            continue;
        }
        pending = [];
    }
    return decls;
}

describe("Message 信封（与 crates/contract 逐字对齐）", () => {
    test("JS 镜像的 schema 版本与契约层一致（契约 v4 · Console + 逐条源码位置）", () => {
        // ⚠️ **不写死数字**：契约层升版时这条断言**不该**变红（错的不是
        // JS 镜像）—— 见 `rustSchemaVersion()` 的说明。
        assert.equal(SCHEMA_VERSION, rustSchemaVersion());
    });

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
        // 虚拟时间走**十进制字符串**（契约 v2 §2.2）。
        assert.deepEqual(eventMessage(0, Event.tick(11, 22)).payload.Event, { Tick: { elapsed_ps: "11", pc: 22 } });
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
        // D-34：事件变体随**内容**改名，topic 跟随变体名。
        assert.equal(eventTopic("WaveformHistory"), "waveform.history");
        // 契约 v2 新增的三个事件变体。
        assert.equal(eventTopic("Registers"), "sim.registers");
        assert.equal(eventTopic("Negotiated"), "sim.negotiated");
        assert.equal(eventTopic("SimStateDelta"), "sim.state.delta");
        // 契约 v3 新增的四个事件变体（`debug.` 命名空间）。
        assert.equal(eventTopic("Disasm"), "debug.disasm");
        assert.equal(eventTopic("SourceLocation"), "debug.source");
        assert.equal(eventTopic("TraceExport"), "debug.export");
        assert.equal(eventTopic("TraceState"), "debug.trace");
        // 契约 v4 新增：guest 的 console 字节流。
        assert.equal(eventTopic("Console"), "sim.console");
        // 前向兼容：未知变体不抛异常。
        assert.equal(eventTopic("SomethingNew"), "unknown");
    });

    test("waveform 构造器对齐 model::WaveformHistory 字段名", () => {
        const w = waveform([{ id: "cpu.gpr.x1", label: "x1", edges: [{ t_ps: 5, value: 42 }] }]);
        assert.deepEqual(Object.keys(w), ["schema_version", "streams"]);
        assert.deepEqual(Object.keys(w.streams[0]), ["id", "label", "edges"]);
        assert.deepEqual(Object.keys(w.streams[0].edges[0]), ["t_ps", "value"]);
        // 边沿的虚拟时间走十进制字符串（契约 v2 §2.2）。
        assert.equal(w.streams[0].edges[0].t_ps, "5");
    });
});

describe("契约一致性（JS 镜像 vs Rust 源码）", () => {
    test("命令变体集合与 Rust CommandPayload 一致", () => {
        requireRustSource();
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
            variantOf(Command.readRegisters(0, 1)),
            variantOf(Command.writeRegister(1, 0)),
            variantOf(Command.negotiate(2, 0)),
            variantOf(Command.subscribe("t")),
            variantOf(Command.unsubscribe("t")),
            variantOf(Command.snapshot()),
            variantOf(Command.restore({})),
            variantOf(Command.admin({})),
            // 契约 v3 新增的四条命令（M-11 调试面）。
            variantOf(Command.disasm(0, 8)),
            variantOf(Command.resolveLine(0)),
            variantOf(Command.exportTrace("Execution", "Csv", 0, 16)),
            variantOf(Command.setTraceEnabled(true)),
            // 契约 v5 新增：往当前会话唯一的 UART0 写字节。
            variantOf(Command.writeConsole([104, 105])),
            // 契约 v6 新增：设置板级内存图（用户在前端配的那张板）。
            variantOf(Command.setMemoryMap({})),
            // 契约 v7 新增：卸载工程（unit 变体）+ 差量更新。
            variantOf(Command.unloadProject()),
            variantOf(Command.patchMemoryMap([], null)),
        ].sort();
        assert.deepEqual(js, rust, "JS 命令变体与 Rust CommandPayload 不一致");
    });

    test("事件变体集合与 Rust EventPayload 一致", () => {
        requireRustSource();
        const rust = rustVariants(readFileSync(protocolRs, "utf8"), "EventPayload").sort();
        const js = [
            variantOf(Event.tick(0, 0)),
            variantOf(Event.halted("Trap", 0)),
            variantOf(Event.error("E_X-01", "m")),
            variantOf(Event.memoryRead(0, "Word", 0)),
            variantOf(Event.registers(0, [])),
            variantOf(Event.negotiated(2, 0, "k")),
            variantOf(Event.simStateDelta([], null, 0)),
            variantOf(Event.waveformHistory({})),
            variantOf(Event.snapshotCreated(1)),
            variantOf(Event.linkUp("l")),
            variantOf(Event.linkDown("l")),
            // 契约 v3 新增的四个事件（M-11 调试面）。
            variantOf(Event.disasm([])),
            variantOf(Event.sourceLocation(0, "demo.c", 7)),
            variantOf(Event.traceExport("Waveform", "Vcd", 0, 10, 42, 128, "$var", false)),
            variantOf(Event.traceState(true, 5, 2)),
            // 契约 v4 新增：guest 的 console 字节流。
            variantOf(Event.console([104, 105])),
            // 契约 v6 新增：当前生效的板级内存图（v7 多了 loaded / revision）。
            variantOf(Event.memoryMap({}, false, true, 0)),
            // 契约 v8 新增：管理员通道的**回复侧**（D-129）。
            variantOf(Event.adminLogs([])),
            variantOf(Event.adminDiag({})),
        ].sort();
        assert.deepEqual(js, rust, "JS 事件变体与 Rust EventPayload 不一致");
    });

    test("Direction / WriteSize / HaltReason / WatchAccess 标签逐字一致", () => {
        requireRustSource();
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
        assert.deepEqual(
            Object.values(TRACE_SOURCE).sort(),
            rustVariants(model, "TraceSource").sort(),
            "TraceSource 标签不一致",
        );
        assert.deepEqual(
            Object.values(TRACE_FORMAT).sort(),
            rustVariants(model, "TraceFormat").sort(),
            "TraceFormat 标签不一致",
        );
    });

    test("契约 v4 的 Console 事件：字节是**数组**，不是字符串（与 Rust 的 serde 逐字对齐）", () => {
        requireRustSource();
        // Rust 侧：`EventPayload::Console { bytes: Vec<u8> }` ⇒ serde 的
        // 默认表示是 **JSON 数组**（`[104, 105]`）——**不是** base64、
        // **不是**字符串。
        //
        // ⚠️ 判别力：把 JS 侧改成 `{ text: bytes.join("") }`（字符串）⇒
        // 本用例**变红**（断言的是**形状**，不是「有东西」）。
        const evt = Event.console([104, 105]);
        assert.deepEqual(evt, { Console: { bytes: [104, 105] } });
        assert.deepEqual(Object.keys(evt.Console), ["bytes"]);
        assert.ok(Array.isArray(evt.Console.bytes), "bytes 必须是数组");
        assert.equal(typeof evt.Console.bytes[0], "number", "字节必须是数字");
        // 线上文本（帧 crc 覆盖的就是它）也必须是数组形态。
        assert.equal(JSON.stringify(evt), '{"Console":{"bytes":[104,105]}}');
        // 空字节流是**空数组**，不是 null。
        assert.deepEqual(Event.console([]), { Console: { bytes: [] } });
        // Rust 侧的变体名与字段名（源码推导）。
        const protocol = readFileSync(protocolRs, "utf8");
        const shape = rustVariantShape(protocol, "EventPayload", "Console");
        assert.equal(shape.kind, "struct");
        assert.deepEqual(shape.fields, ["bytes"]);
    });

    test("每个变体的内层字段名与 Rust 逐字一致（不只是变体名）", () => {
        requireRustSource();
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
                ReadRegisters: Command.readRegisters(0, 1),
                WriteRegister: Command.writeRegister(1, 0),
                Negotiate: Command.negotiate(2, 0),
                Subscribe: Command.subscribe("t"),
                Unsubscribe: Command.unsubscribe("t"),
                Snapshot: Command.snapshot(),
                Restore: Command.restore({}),
                Admin: Command.admin({}),
                // 契约 v3（M-11 调试面）。
                Disasm: Command.disasm(0, 8),
                ResolveLine: Command.resolveLine(0),
                ExportTrace: Command.exportTrace("Execution", "Csv", 0, 16),
                SetTraceEnabled: Command.setTraceEnabled(true),
                // 契约 v5：往当前会话唯一的 UART0 写字节。
                WriteConsole: Command.writeConsole([104, 105]),
                // 契约 v6：设置板级内存图（用户在前端配的那张板）。
                SetMemoryMap: Command.setMemoryMap({}, null),
                // 契约 v7：卸载工程（unit 变体）+ 差量更新。
                UnloadProject: Command.unloadProject(),
                PatchMemoryMap: Command.patchMemoryMap([], null),
                // ⚠️ **契约 v9**（STAGE-42 · 调试器增强批）⇒ ⇒ 这两个
                //   构造器**必须**在这儿登记 ⇒ ⇒ 否则 Rust 侧一加变体，
                //   跨层门禁**看不见**它们的字段名 ⇒ ⇒ Rust 改了键名而 JS
                //   没改 ⇒ ⇒ **静默**（F-13）。
                ResolveSymbol: Command.resolveSymbol("main"),
                DumpMemory: Command.dumpMemory(0, 16),
            },
            EventPayload: {
                Tick: Event.tick(0, 0),
                Halted: Event.halted("Trap", 0),
                Error: Event.error("E_X-01", "m"),
                MemoryRead: Event.memoryRead(0, "Word", 0),
                Registers: Event.registers(0, [0]),
                Negotiated: Event.negotiated(2, 0, "k"),
                SimStateDelta: Event.simStateDelta([], null, 0),
                WaveformHistory: Event.waveformHistory(waveform([])),
                SnapshotCreated: Event.snapshotCreated(1),
                LinkUp: Event.linkUp("l"),
                LinkDown: Event.linkDown("l"),
                // 契约 v3（M-11 调试面）。
                Disasm: Event.disasm([]),
                SourceLocation: Event.sourceLocation(0, "demo.c", 7),
                TraceExport: Event.traceExport("Waveform", "Vcd", 0, 10, 42, 128, "$var", false),
                TraceState: Event.traceState(true, 5, 2),
                // 契约 v4：guest 的 console 字节流。
                Console: Event.console([104, 105]),
                // 契约 v6：当前生效的板级内存图（v7 多了 loaded / revision）。
                MemoryMap: Event.memoryMap({}, false, true, 0),
                // 契约 v8：管理员通道的**回复侧**（D-129）。
                AdminLogs: Event.adminLogs([]),
                AdminDiag: Event.adminDiag({}),
                // ⚠️ **契约 v9**（STAGE-42 · 调试器增强批）⇒ ⇒ 同上：这两
                //   个事件构造器**必须**在这儿登记 ⇒ ⇒ Rust 侧一加变体，
                //   字段名就被**逐字**比上。
                SymbolResolved: Event.symbolResolved("main", null, [], 0),
                MemoryDump: Event.memoryDump(0, [0]),
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

    test("newtype 变体携带的 DTO 字段名与 Rust 一致", () => {
        requireRustSource();
        const model = readFileSync(modelRs, "utf8");
        const project = buildMockProject();
        // The DTOs the front end actually builds.
        const cases = [
            ["Project", project],
            ["ProductTree", project.tree],
            ["TreeNode", project.tree.root],
            ["ProjectVersion", project.version],
            ["WaveformHistory", waveform([])],
            // `DeltaRegister` 嵌在 `SimStateDelta` 的 `registers[]` 里
            // ——变体级的字段名检查只比**外层**，进不到元素里（F3）。
            ["DeltaRegister", deltaRegister(1, 42)],
            // ⚠️ `DisasmEntryDto` 嵌在 `Disasm` 的 `entries[]` 里
            // ——同样进不到元素里（v0.20 检视 B · D-3：把 Rust 侧的
            // `mnemonic` 改名成 `mnem`，**前端门禁 228/228 全绿** ⇒
            // Rust 改而 JS 忘改 ⇒ 线上字段名不匹配 ⇒ **静默**）。
            // ⇒ 这里**造一个非空**的 `entries[]`，把元素字段名也钉住。
            ["DisasmEntryDto", disasmEntry({ addr: 0, code: 0x02a00093, mnemonic: "addi ra,zero,42", width: 4 })],
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

    test("契约 v4：反汇编条目的源码位置是**可选**的（没有就是 null，绝不伪造）", () => {
        requireRustSource();
        const model = readFileSync(modelRs, "utf8");
        // Rust 侧 `DisasmEntryDto` 多了 `file` / `line` 两个 `Option` 字段
        // （台账 D-102）⇒ JS 镜像必须**也**带这两个键，且缺失时是
        // **`null`**（serde 的 `Option` 默认表示）。
        //
        // ⚠️ 判别力：`null` 与「空串 / 0」是**不同**的东西——空串与 0 会
        // 与 `SourceLocation` 事件的两种「没有答案」（`line == 0` = 根本
        // 没有 DWARF，`line == u32::MAX` = 没命中行表）撞车 ⇒ 同一个地址
        // 在两个 DTO 里讲出**互相矛盾**的故事。
        const withoutSource = disasmEntry({
            addr: 0x1000,
            code: 0x02a00093,
            mnemonic: "addi ra,zero,42",
            width: 4,
        });
        assert.equal(withoutSource.file, null, "缺失的源码文件必须是 null");
        assert.equal(withoutSource.line, null, "缺失的源码行号必须是 null");
        assert.deepEqual(
            Object.keys(withoutSource).sort(),
            rustStructFields(model, "DisasmEntryDto").sort(),
            "DisasmEntryDto 的字段名与 Rust 不一致",
        );

        const withSource = disasmEntry({
            addr: 0x1000,
            code: 0x02a00093,
            mnemonic: "addi ra,zero,42",
            width: 4,
            file: "src/main.c",
            line: 42,
        });
        assert.equal(withSource.file, "src/main.c");
        assert.equal(withSource.line, 42);
    });

    test("虚拟时间字段在 Rust 侧必须挂 numstr 属性（§2 数值策略）", () => {
        requireRustSource();
        // 规格 §2：凡是「随仿真时长增长」的量（虚拟时间），线上表示是
        // **十进制字符串**。Rust 侧靠字段上的
        // `#[serde(with = "crate::numstr::u64_decimal")]` 实现。
        //
        // ⚠️ 这条门禁原来**看不见**这个属性：名字 / 形状的对拍只看变体
        // 与字段名，看不到 serde 的**表示**。把 `Tick::elapsed_ps` 的
        // numstr 属性删掉 → Rust 侧发 JSON 数字、JS 镜像发字符串，而
        // `numstr::u64_decimal` **两种形态都收** → **静默**退回旧形态，
        // 运行时也不会报错（同一个盲点覆盖全部 8+1 个虚拟时间字段）。
        const model = readFileSync(modelRs, "utf8");
        const protocol = readFileSync(protocolRs, "utf8");
        const snapshot = readFileSync(snapshotRs, "utf8");

        // [源码, 容器, 字段, 归属变体（null = 不限）, 期望条数]
        const cases = [
            [model, "SimState", "virtual_time_ps", null, 1],
            [model, "Edge", "t_ps", null, 1],
            [model, "TraceEvent", "t_ps", null, 1],
            [model, "EdgeEvent", "t_ps", null, 1],
            [model, "WaveformDelta", "t_virt", null, 1],
            [protocol, "EventPayload", "elapsed_ps", "Tick", 1],
            [protocol, "EventPayload", "t_ps", "SimStateDelta", 1],
            // `SchedEventDto` 的两个变体各带一个 `at_ps`。
            [snapshot, "SchedEventDto", "at_ps", null, 2],
            [snapshot, "QueuedEventDto", "at_ps", null, 1],
        ];

        const numstr = /serde\s*\(\s*with\s*=\s*"crate::numstr::u64_decimal"\s*\)/;
        for (const [source, container, field, owner, expected] of cases) {
            const decls = rustFieldDeclarations(source, container).filter(
                (d) => d.name === field && (owner === null || d.owner === owner),
            );
            assert.equal(
                decls.length,
                expected,
                `${container} 里叫 ${field} 的字段数量与预期不符（Rust 侧改名 / 改结构了？）`,
            );
            for (const decl of decls) {
                assert.ok(
                    decl.attributes.some((a) => numstr.test(a)),
                    `${container}::${decl.owner}::${field} 缺少 `
                    + '#[serde(with = "crate::numstr::u64_decimal")]'
                    + "——虚拟时间会退回 JSON 数字（§2 数值策略失效）",
                );
            }
        }
    });
});

describe("传输帧（M-07 TransportFrame）", () => {
    test("帧结构含会话、角色、关联 id 与 crc", () => {
        const frame = encodeFrame({
            type: FRAME_TYPE.Command,
            seq: 3,
            sessionId: "sess-1",
            role: "embedded",
            reqId: 9,
            payload: commandMessage(3, Command.nop()),
        });
        assert.deepEqual(
            Object.keys(frame),
            ["type", "seq", "sessionId", "role", "reqId", "crc", "payload"],
        );
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
