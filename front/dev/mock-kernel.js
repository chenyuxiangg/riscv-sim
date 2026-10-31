// Dev-only mock kernel (pure JS, no DOM / no Worker API).
//
// ⚠️ **mock ≠ 真内核**（台账 D-55）：mock 的语义**可能**与真内核不同。
// 这里给的是「形状正确的假数据」，不是真的反汇编器 / 真的 trace 记录器。
//
// The real WASM kernel has **no export layer yet** (`crates/kernel` has
// no `wasm-bindgen` dependency, no `#[wasm_bindgen]` export and no
// cdylib entry), so `tools/build-wasm.sh` cannot produce a loadable
// module yet. Until the Wave-2 export layer lands, this mock is what the
// browser Worker hosts: it speaks the *frozen* contract
// (`riscv_sim_contract::protocol::Message`) so the whole front-end chain
// — M-07 → M-05 → M-04 → M-10 — can be exercised end to end.
//
// This file is **pure**: it has no Worker/DOM dependency, so the same
// code runs inside the browser Worker and inside the node:test gate
// (which loads the real worker file in a `node:worker_threads` Worker).
//
// Scope / behaviour boundary (documented in
// `docs/reviews/FRONTEND_ServiceLayer_Review.md`):
//   - it models a 6-instruction toy program, not the RV32IMC ISS;
//   - it never fabricates a register snapshot outside the frozen
//     contract — contract v2 has a **first-class** register carrier
//     (`EventPayload::Registers` / `EventPayload::SimStateDelta`), and
//     the probe signals travel as `EventPayload::WaveformHistory`;
//   - it is a test double, not a simulator: no interrupts, no MMU, no
//     peripherals, no deterministic-replay guarantees.

import {
    Event,
    eventMessage,
    decodeMessage,
    waveform,
    HALT_REASON,
    WRITE_SIZE,
    SCHEMA_VERSION,
    DUMP_MAX_BYTES,
    SYMBOL_CANDIDATE_LIMIT,
} from "../lib/message.js";
import { decodeFrame, encodeFrame, FRAME_TYPE } from "../lib/frame.js";

/** RAM window of the toy platform. */
export const MOCK_RAM = Object.freeze({ base: 0x0000_0000, size: 0x1_0000 });

/** Address of the synthetic breakpoint at the end of the toy program. */
export const MOCK_BREAKPOINT_ADDR = 0x40;

/** Kernel descriptor reported by the `Negotiate` reply. */
export const MOCK_KERNEL_ID = "riscv_sim_mock/0.1.0 RV32IMC";

/**
 * Capability bits the mock kernel **grants** (contract v2 spec §5).
 *
 * ⚠️ The reply carries what the **kernel** grants — **not** what the host
 * offers. `CommandPayload::Negotiate { cap_bits }` is the host's
 * *offer*; the spec defines no intersection, so echoing the offer back
 * would be a second, silently different negotiation protocol. The real
 * kernel answers with its own `KERNEL_CAP_BITS` (`0b111`); the mock
 * mirrors that shape so the two behave the same.
 *
 * The bits must never be **empty**: an empty set means "the front end
 * degrades every capability", which is not what this mock does.
 */
export const MOCK_CAP_BITS = 0b111;

/** Number of general-purpose registers. */
const GPR_COUNT = 32;

/** Virtual time added per executed instruction (picoseconds). */
const TICK_PS = 1000;

// ---------------------------------------------------------------------------
// M-11 debug surface (contract v3) — fake data
// ---------------------------------------------------------------------------

/**
 * The mock's disassembly entries — **shape-correct fake data**.
 *
 * ⚠️ The mock is **not** a disassembler: the real kernel renders the
 * mnemonics with M-11's renderer (ISA mnemonics only, compressed
 * instructions keep the `c.` prefix — ruling C). The mock only has to
 * produce a well-shaped `DisasmEntryDto` (`addr` / `code` / `mnemonic` /
 * `width` / `file` / `line`) so the front end can be exercised end to end.
 *
 * ⚠️ **契约 v4（D-102）**：`file` / `line` 是**可选**的逐条源码位置 ⇒
 * mock 给的是 **`null`**（= 没有行表）。**不**伪造文件名与行号。
 *
 * The addresses are consistent with the widths: a 4-byte entry advances
 * the address by 4, the 2-byte compressed entry by 2.
 */
const MOCK_DISASM_ENTRIES = Object.freeze([
    { addr: 0x0000_0000, code: 0x02a0_0093, mnemonic: "addi x1, x0, 42", width: 4, file: null, line: null },
    { addr: 0x0000_0004, code: 0x0070_0113, mnemonic: "addi x2, x0, 7", width: 4, file: null, line: null },
    { addr: 0x0000_0008, code: 0x0020_81b3, mnemonic: "add x3, x1, x2", width: 4, file: null, line: null },
    { addr: 0x0000_000a, code: 0x0000_2a00, mnemonic: "c.addi sp,-16", width: 2, file: null, line: null },
    { addr: 0x0000_000c, code: 0x1000_21a3, mnemonic: "sw x3, 256(x0)", width: 4, file: null, line: null },
    { addr: 0x0000_0010, code: 0x0400_006f, mnemonic: "jal x0, 64", width: 4, file: null, line: null },
]);

/**
 * The mock's export blocks — one **line** per entry.
 *
 * The key is `"{source}:{format}"`. Only the **legal** combinations of
 * ruling D are present (`Waveform` + `Csv` / `Vcd`, `Execution` + `Csv` /
 * `Json`); the illegal ones are missing **on purpose** so the mock
 * rejects them with `E_CTX-03` exactly like the real kernel does.
 */
const MOCK_EXPORT_LINES = Object.freeze({
    "Waveform:Csv": Object.freeze([
        "t_ps,signal,value",
        "1000,GPIO0.OUT,1",
        "2000,GPIO0.OUT,0",
        "3000,GPIO0.OUT,1",
    ]),
    "Waveform:Vcd": Object.freeze([
        "$date mock $end",
        "$var wire 1 ! GPIO0.OUT $end",
        "$enddefinitions $end",
        "$dumpvars",
        "0!",
        "1!",
        "$end",
    ]),
    "Execution:Csv": Object.freeze([
        "kind,addr,value",
        "instr,00000000,02a00093",
        "instr,00000004,00700113",
        "mem,00000100,0000002a",
    ]),
    "Execution:Json": Object.freeze([
        '{"kind":"instr","addr":0,"code":"02a00093"}',
        '{"kind":"instr","addr":4,"code":"00700113"}',
        '{"kind":"mem","addr":256,"value":42}',
    ]),
});

/**
 * Default number of entries per export block (the mock's "kernel default
 * limit" — `limit = 0` in the command means "you decide").
 *
 * It is deliberately **smaller** than the block size: that is what makes
 * the continuation path (`next_offset < total`) reachable at all.
 */
const MOCK_EXPORT_BLOCK = 2;

/**
 * The mock's **fake** symbol table (contract v9 · `ResolveSymbol`).
 *
 * ⚠️ **这些是假数据**：地址是**编的**，**不对应**任何真实 ELF 的符号表
 *   ⇒ ⇒ mock ≠ 真内核（台账 D-55）⇒ ⇒ 它只保证「形状正确」，让前端那条
 *   链（查符号 ⇒ 出候选 ⇒ 点候选下断点）能被**端到端**跑一遍。
 *
 * ⚠️ **契约语义照做**（不是编的）：**精确命中**与**前缀候选**分开
 *   （精确命中**不**在候选里）⇒ ⇒ 候选按**名字升序** ⇒ ⇒ 条数 ≤
 *   `SYMBOL_CANDIDATE_LIMIT` ⇒ ⇒ `total` = 这张表的条数。
 */
const MOCK_SYMBOLS = Object.freeze([
    { name: "main", addr: 0x0000_1000, size: 64 },
    { name: "main_loop", addr: 0x0000_1040, size: 32 },
    { name: "helper", addr: 0x0000_1080, size: 16 },
]);

/**
 * 假字节（契约 v9 · `DumpMemory`）。
 *
 * ⚠️ **这是假数据**：内容是 `addr + i` 的低字节 ⇒ ⇒ **不对应**任何真实
 *   内存 ⇒ ⇒ 只保证「长度 == 请求的 `len`」与「字节在 0..255」。
 *
 * @param {number} addr
 * @param {number} len
 * @returns {number[]}
 */
function mockDumpBytes(addr, len) {
    const out = [];
    for (let i = 0; i < len; i += 1) out.push((addr + i) & 0xff);
    return out;
}

/**
 * A non-negative integer from a command field.
 *
 * A malformed field is **not** an error here — it degrades to `0` (the
 * mock is a test double, not a validator; the real kernel owns the
 * input limits).
 *
 * @param {unknown} raw
 * @param {number} [fallback]
 * @returns {number}
 */
function toCount(raw, fallback = 0) {
    const n = Math.trunc(Number(raw));
    return Number.isFinite(n) && n >= 0 ? n : fallback;
}

/**
 * Create a mock kernel instance.
 *
 * @param {{seed?: number}} [options]
 */
export function createMockKernel(options = {}) {
    const seed = Number(options.seed ?? 1);

    const state = {
        gpr: new Uint32Array(32),
        pc: 0,
        timeVirt: 0,
        halted: false,
        haltReason: null,
        memory: new Map(),
        breakpoints: new Map(),
        topics: new Set(),
        project: null,
        snapshotCounter: 0,
        executed: 0,
        // Trace recording (contract v3 — `SetTraceEnabled`). The mock
        // records a **count**, not real trace entries: `len` grows with
        // the executed instructions while recording is on.
        traceEnabled: false,
        traceLen: 0,
        traceDropped: 0,
    };

    /** Registers changed since the last delta push. */
    const dirtyRegisters = new Set();
    let pcDirty = true;
    let seq = 0;

    // -- state helpers -------------------------------------------------------

    function reset() {
        state.gpr.fill(0);
        state.pc = 0;
        state.timeVirt = 0;
        state.halted = false;
        state.haltReason = null;
        state.memory.clear();
        state.breakpoints.clear();
        // A new project is a new session: the old topic subscriptions
        // die with it (FR-13 — every session id owns its namespace).
        // The host has to subscribe again after a `LoadProject`.
        state.topics.clear();
        state.snapshotCounter = 0;
        state.executed = 0;
        state.traceEnabled = false;
        state.traceLen = 0;
        state.traceDropped = 0;
        for (let i = 0; i < GPR_COUNT; i += 1) dirtyRegisters.add(i);
        pcDirty = true;
    }

    function writeReg(index, value) {
        if (index === 0) return; // x0 is hard-wired to 0
        state.gpr[index] = value >>> 0;
        dirtyRegisters.add(index);
    }

    function executeOne() {
        switch (state.pc) {
            case 0x00: // addi x1, x0, 42
                writeReg(1, 42);
                state.pc = 0x04;
                break;
            case 0x04: // addi x2, x0, 7
                writeReg(2, 7);
                state.pc = 0x08;
                break;
            case 0x08: // add x3, x1, x2
                writeReg(3, (state.gpr[1] + state.gpr[2]) >>> 0);
                state.pc = 0x0c;
                break;
            case 0x0c: // sw x3, 0x100(x0)
                state.memory.set(0x100, state.gpr[3]);
                state.pc = 0x10;
                break;
            case 0x10: // j 0x40
                state.pc = 0x40;
                break;
            case 0x40: // breakpoint
                state.halted = true;
                state.haltReason = HALT_REASON.Breakpoint;
                break;
            default:
                state.halted = true;
                state.haltReason = HALT_REASON.Trap;
                break;
        }
        state.timeVirt += TICK_PS;
        state.executed += 1;
        // Trace recording is **off** by default and costs nothing while
        // it is off (ruling H — the "zero overhead" rule).
        if (state.traceEnabled) state.traceLen += 1;
        pcDirty = true;
    }

    function run(maxSteps) {
        let steps = 0;
        while (steps < maxSteps && !state.halted) {
            if (state.breakpoints.has(state.pc)) {
                state.halted = true;
                state.haltReason = HALT_REASON.Breakpoint;
                break;
            }
            executeOne();
            steps += 1;
        }
        return steps;
    }

    function isMapped(addr) {
        return addr >= MOCK_RAM.base && addr < MOCK_RAM.base + MOCK_RAM.size;
    }

    // -- payload builders ----------------------------------------------------

    function probeStreams() {
        return [
            {
                id: "probe.led0",
                label: "LED0",
                edges: [{ t_ps: state.timeVirt, value: state.gpr[3] & 1 }],
            },
            {
                id: "probe.uart0.tx",
                label: "UART0.TX",
                edges: [{ t_ps: state.timeVirt, value: state.gpr[1] }],
            },
        ];
    }

    // -- frame plumbing ------------------------------------------------------

    function makeFrame(reqId, payload) {
        seq += 1;
        return encodeFrame({
            type: FRAME_TYPE.Event,
            seq,
            sessionId: state.sessionId ?? null,
            role: state.role ?? null,
            reqId: reqId ?? null,
            payload: eventMessage(seq, payload),
        });
    }

    /**
     * The registers changed since the last push, in ascending index order.
     *
     * @returns {Array<{index: number, value: number}>}
     */
    function dirtySnapshot() {
        const registers = [];
        for (const i of [...dirtyRegisters].sort((a, b) => a - b)) {
            registers.push({ index: i, value: state.gpr[i] });
        }
        return registers;
    }

    /** Forget the pending changes (after they have been pushed). */
    function clearDirty() {
        dirtyRegisters.clear();
        pcDirty = false;
    }

    /**
     * Push the register file to the `sim.registers` subscribers.
     *
     * `Registers` carries a **contiguous** range, so the push is the whole
     * file; the *scattered* increments ride on `SimStateDelta` instead.
     *
     * @param {object[]} frames
     */
    function pushRegisters(frames) {
        if (!state.topics.has("sim.registers")) return;
        const values = [];
        for (let i = 0; i < GPR_COUNT; i += 1) values.push(state.gpr[i]);
        frames.push(makeFrame(null, Event.registers(0, values)));
    }

    /**
     * Push the changed registers to the `sim.state.delta` subscribers.
     *
     * @param {object[]} frames
     */
    function pushStateDelta(frames) {
        if (!state.topics.has("sim.state.delta")) return;
        const registers = dirtySnapshot();
        if (registers.length === 0 && !pcDirty) return;
        frames.push(makeFrame(null, Event.simStateDelta(
            registers,
            pcDirty ? state.pc : null,
            state.timeVirt,
        )));
    }

    function pushWaveform(frames) {
        if (!state.topics.has("sim.waveform")) return;
        frames.push(makeFrame(null, Event.waveformHistory(waveform(probeStreams()))));
    }

    /**
     * The mock's disassembly sequence: the entries at (or after) `addr`,
     * at most `count` of them.
     *
     * ⚠️ The sequence may be **shorter** than `count` — that is
     * truncation (the real kernel stops at the first unmapped address),
     * **not** an error.
     *
     * @param {number} addr
     * @param {number} count
     * @returns {Array<{addr: number, code: number, mnemonic: string, width: number}>}
     */
    function mockDisasmEntries(addr, count) {
        if (count <= 0) return [];
        const exact = MOCK_DISASM_ENTRIES.findIndex((e) => e.addr === addr);
        const from = exact >= 0
            ? exact
            : MOCK_DISASM_ENTRIES.findIndex((e) => e.addr > addr);
        if (from < 0) return [];
        return MOCK_DISASM_ENTRIES.slice(from, from + count).map((e) => ({ ...e }));
    }

    // -- command handling ----------------------------------------------------

    /**
     * Handle one inbound command frame. Returns the frames to send back
     * (an empty array means "nothing to say").
     *
     * @param {unknown} rawFrame
     * @returns {object[]}
     */
    function handle(rawFrame) {
        const frames = [];
        let frame;
        try {
            frame = decodeFrame(rawFrame);
        } catch (_err) {
            return frames;
        }
        if (frame.type !== FRAME_TYPE.Command) return frames;
        state.sessionId = frame.sessionId ?? state.sessionId ?? null;
        state.role = frame.role ?? state.role ?? null;

        let decoded;
        try {
            decoded = decodeMessage(frame.payload);
        } catch (err) {
            frames.push(makeFrame(frame.reqId, Event.error(
                err?.code ?? "E_PROTO-08",
                err?.message ?? String(err),
            )));
            return frames;
        }
        if (decoded.direction !== "Command") {
            frames.push(makeFrame(frame.reqId, Event.error("E_PROTO-08", "收到非命令方向的消息")));
            return frames;
        }

        const reqId = frame.reqId ?? null;
        const value = decoded.value ?? {};

        switch (decoded.variant) {
            case "Nop":
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                break;

            case "LoadProject":
                reset();
                state.project = value;
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                pushRegisters(frames);
                pushStateDelta(frames);
                clearDirty();
                break;

            case "Continue":
                run(64);
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                pushRegisters(frames);
                pushStateDelta(frames);
                clearDirty();
                if (state.halted) {
                    frames.push(makeFrame(null, Event.halted(state.haltReason, state.pc)));
                }
                pushWaveform(frames);
                break;

            case "StepN": {
                const n = Math.max(0, Math.min(Number(value.n) || 0, 1024));
                run(n);
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                pushRegisters(frames);
                pushStateDelta(frames);
                clearDirty();
                if (state.halted) {
                    frames.push(makeFrame(null, Event.halted(state.haltReason, state.pc)));
                }
                pushWaveform(frames);
                break;
            }

            case "ReadRegisters": {
                // 规格 §5：读的 `index` / `count` **钳位**到 32 个寄存器，
                // 回复带的是**实际**起点。
                const requested = Number(value.index) || 0;
                const start = Math.max(0, Math.min(Math.trunc(requested), GPR_COUNT - 1));
                const available = GPR_COUNT - start;
                const wanted = Math.max(0, Math.trunc(Number(value.count) || 0));
                const count = Math.min(wanted, available);
                // ⚠️ `count = 0` 也**必须回复**（规格 §5 已订正）：回复
                // `Registers { index, values: [] }`，`index` 仍是钳位后的
                // 实际起点。
                //
                // 「不回复」不在 M-05 的回复模型里：它按 `reqId` 配对请求与
                // 回复，所以「没有回复」与「消息丢了」在协议上**不可区分**——
                // 一个完全合法的请求（「读 0 个寄存器」）会变成一条**假超时**
                // （E_CTX-06），用户看到的是「内核没应答」而不是「读到了
                // 空区间」。
                const values = [];
                for (let i = 0; i < count; i += 1) values.push(state.gpr[start + i]);
                frames.push(makeFrame(reqId, Event.registers(start, values)));
                break;
            }

            case "WriteRegister": {
                const index = Number(value.index);
                if (!Number.isInteger(index) || index < 0 || index >= GPR_COUNT) {
                    // 写越界是**拒绝**，不是钳位——写钳位会静默写到别的寄存器。
                    frames.push(makeFrame(reqId, Event.error(
                        "E_DBG-01",
                        `寄存器下标越界：${String(value.index)}`,
                    )));
                    break;
                }
                if (index !== 0) {
                    state.gpr[index] = Number(value.value) >>> 0;
                    dirtyRegisters.add(index);
                }
                // 写 x0 被丢弃（D-04），但**仍回复**——值恒为 0。
                frames.push(makeFrame(reqId, Event.registers(index, [state.gpr[index]])));
                break;
            }

            case "Negotiate": {
                const min = Number(value.min_schema_version ?? 0);
                if (min > SCHEMA_VERSION) {
                    frames.push(makeFrame(reqId, Event.error(
                        "E_PROTO-01",
                        `协议版本不兼容：宿主最低 ${min} > 内核 ${SCHEMA_VERSION}`,
                    )));
                    break;
                }
                // 规格 §5：回的是**内核授予的**能力位（`MOCK_CAP_BITS`），
                // **不是**宿主提供的 `cap_bits`——后者是宿主的「报价」，
                // 规格没有定义取交集，回显它等于凭空造出第二套协商语义。
                frames.push(makeFrame(reqId, Event.negotiated(
                    SCHEMA_VERSION,
                    MOCK_CAP_BITS,
                    MOCK_KERNEL_ID,
                )));
                break;
            }

            case "Pause":
                state.halted = true;
                state.haltReason = HALT_REASON.UserPause;
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                frames.push(makeFrame(null, Event.halted(state.haltReason, state.pc)));
                break;

            case "ReadMemory": {
                const addr = Number(value.addr) >>> 0;
                const size = value.size ?? WRITE_SIZE.Word;
                if (!isMapped(addr)) {
                    frames.push(makeFrame(reqId, Event.error(
                        "E_BUS-01",
                        `未映射地址 0x${addr.toString(16).padStart(8, "0")}`,
                    )));
                    break;
                }
                const word = state.memory.get(addr) ?? 0;
                frames.push(makeFrame(reqId, Event.memoryRead(addr, size, word)));
                break;
            }

            case "WriteMemory": {
                const addr = Number(value.addr) >>> 0;
                const size = value.size ?? WRITE_SIZE.Word;
                if (!isMapped(addr)) {
                    frames.push(makeFrame(reqId, Event.error(
                        "E_BUS-01",
                        `未映射地址 0x${addr.toString(16).padStart(8, "0")}`,
                    )));
                    break;
                }
                state.memory.set(addr, Number(value.value) >>> 0);
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                break;
            }

            case "Subscribe": {
                const topic = String(value.topic ?? "");
                state.topics.add(topic);
                if (topic === "sim.registers") {
                    // The snapshot **is** the response — the caller asked
                    // for the current register file.
                    const values = [];
                    for (let i = 0; i < GPR_COUNT; i += 1) values.push(state.gpr[i]);
                    frames.push(makeFrame(reqId, Event.registers(0, values)));
                } else {
                    frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                    if (topic === "sim.waveform") pushWaveform(frames);
                    if (topic === "sim.state.delta") pushStateDelta(frames);
                }
                clearDirty();
                break;
            }

            case "Unsubscribe": {
                state.topics.delete(String(value.topic ?? ""));
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                break;
            }

            case "Snapshot":
                state.snapshotCounter += 1;
                frames.push(makeFrame(reqId, Event.snapshotCreated(state.snapshotCounter)));
                break;

            case "Restore":
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                break;

            case "SetBreakpoint": {
                const id = Number(value.id ?? 0);
                const addr = Number(value.addr ?? 0) >>> 0;
                state.breakpoints.set(addr, id);
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                break;
            }

            case "ClearBreakpoint": {
                // The frozen contract carries **only** the breakpoint id
                // (`CommandPayload::ClearBreakpoint { id: u32 }`) — there
                // is no `addr` field. The mock table is keyed by address,
                // so the id has to be resolved to an address first; the
                // old code deleted address 0 and left the breakpoint in
                // place (F1).
                const rawId = value.id;
                const id = Number(rawId);
                if (rawId !== undefined && rawId !== null && Number.isFinite(id)) {
                    for (const [addr, bpId] of state.breakpoints) {
                        if (bpId === id) {
                            state.breakpoints.delete(addr);
                            break;
                        }
                    }
                }
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                break;
            }

            case "SetWatchpoint":
            case "ClearWatchpoint":
                frames.push(makeFrame(reqId, Event.tick(state.timeVirt, state.pc)));
                break;

            case "Admin":
                frames.push(makeFrame(reqId, Event.error(
                    "E_CTX-05",
                    "管理通道未启用（admin op 不在当前会话的能力面内）",
                )));
                break;

            // -- M-11 debug surface (contract v3) ----------------------------

            case "Disasm": {
                // ⚠️ The sequence may be **shorter** than `count` — that is
                // truncation (the real kernel stops at the first unmapped
                // address), **not** an error.
                const addr = toCount(value.addr);
                const count = toCount(value.count);
                frames.push(makeFrame(reqId, Event.disasm(mockDisasmEntries(addr, count))));
                break;
            }

            case "ResolveLine": {
                const addr = toCount(value.addr);
                frames.push(makeFrame(reqId, Event.sourceLocation(addr, "demo.c", 7)));
                break;
            }

            case "ExportTrace": {
                const source = String(value.source ?? "");
                const format = String(value.format ?? "");
                const lines = MOCK_EXPORT_LINES[`${source}:${format}`];
                if (!lines) {
                    // An illegal source/format combination is **rejected**
                    // by the kernel (ruling D / §9.5) — the mock rejects it
                    // the same way, so the front end's error path can be
                    // exercised.
                    frames.push(makeFrame(reqId, Event.error(
                        "E_CTX-03",
                        `导出组合非法：source=${source} / format=${format}`,
                    )));
                    break;
                }
                const total = lines.length;
                const offset = toCount(value.offset);
                const wanted = toCount(value.limit);
                // `limit = 0` means "the kernel picks the block size".
                const limit = wanted > 0 ? wanted : MOCK_EXPORT_BLOCK;
                const start = Math.min(offset, total);
                const end = Math.min(start + limit, total);
                // ⚠️ **每块必须以换行结尾**（**与真内核逐字一致** · v0.62 批）：
                //   真内核的 `render_csv` 逐行 `push('\n')` ⇒ ⇒ 块与块
                //   **直接拼接**就是完整的一份 ⇒ ⇒ 「下载轨迹到本地」那个
                //   「翻完所有块再拼起来」的做法才成立。
                //   ⚠️ 此前这里用 `join("\n")`（**末尾无换行**）⇒ ⇒ 拼起来会
                //   **粘行**（上一块最后一行与下一块第一行连在一起）⇒ ⇒ 前端
                //   门禁因此变红 ⇒ ⇒ **这是替身不保真**（同一族：STAGE-34 批给
                //   dom-stub 补 `className` —— 「保真度不足 ⇒ ⇒ 整套前端门禁的
                //   结论不可信」）。
                // ⚠️ **空块必须真的是空**（末尾不能多一个换行）⇒ ⇒ 判空在前。
                const slice = lines.slice(start, end);
                const chunk = slice.length > 0 ? slice.join("\n") + "\n" : "";
                // `next_offset` is the **next** block's start (= this
                // block's end); at the end of the buffer it equals
                // `total` (STAGE-5 §3.2).
                frames.push(makeFrame(reqId, Event.traceExport(
                    source, format, offset, end, total, chunk.length, chunk,
                    // ⚠️ `overflow_marked` = the exported window is **not**
                    // the complete history (the source ring overflowed).
                    // The mock keeps a single overflow counter for both
                    // sources (D-55: the mock is a test double, its
                    // numbers are not the kernel's numbers).
                    state.traceDropped > 0,
                )));
                break;
            }

            case "SetTraceEnabled": {
                const enabled = Boolean(value.enabled);
                state.traceEnabled = enabled;
                // ⚠️ `len` does **not** change when recording is turned off
                // (acceptance #15: "zero recording, `len` unchanged") — the
                // ring keeps what it already holds.
                frames.push(makeFrame(reqId, Event.traceState(
                    enabled, state.traceLen, state.traceDropped,
                )));
                break;
            }

            // -- 契约 v9 · 调试器增强批 ---------------------------------------

            case "ResolveSymbol": {
                // ⚠️ **空名字 ⇒ 拒绝**（契约 v9 §2.2：`E_DBG-04`）⇒ ⇒
                //   **不发** `SymbolResolved` ⇒ ⇒ 前端会显示内核原文。
                const name = String(value.name ?? "");
                if (name.trim() === "") {
                    frames.push(makeFrame(reqId, Event.error(
                        "E_DBG-04",
                        "符号名不能为空",
                    )));
                    break;
                }
                const exact = MOCK_SYMBOLS.find((s) => s.name === name) ?? null;
                // ⚠️ **前缀候选不含精确命中那条**，按**名字升序**，条数 ≤ 上限。
                const candidates = MOCK_SYMBOLS
                    .filter((s) => s.name.startsWith(name) && s.name !== name)
                    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
                    .slice(0, SYMBOL_CANDIDATE_LIMIT)
                    .map((s) => ({ ...s }));
                frames.push(makeFrame(reqId, Event.symbolResolved(
                    name,
                    exact ? { ...exact } : null,
                    candidates,
                    MOCK_SYMBOLS.length,
                )));
                break;
            }

            case "DumpMemory": {
                // ⚠️ **原样拒，不静默截断**（契约 v9 §3 裁决 D）⇒ ⇒
                //   `len == 0` 或超上限 ⇒ ⇒ `E_DBG-03` ⇒ ⇒ **不发**
                //   `MemoryDump`。
                const addr = Number(value.addr ?? 0) >>> 0;
                const len = Number(value.len ?? 0);
                if (!Number.isInteger(len) || len <= 0 || len > DUMP_MAX_BYTES) {
                    frames.push(makeFrame(reqId, Event.error(
                        "E_DBG-03",
                        `转储长度越界：${String(value.len)}（必须是 1 .. ${DUMP_MAX_BYTES}）`,
                    )));
                    break;
                }
                frames.push(makeFrame(reqId, Event.memoryDump(
                    addr,
                    mockDumpBytes(addr, len),
                )));
                break;
            }

            default:
                frames.push(makeFrame(reqId, Event.error(
                    "E_CTX-05",
                    `未注册路由：${decoded.variant}`,
                )));
                break;
        }
        return frames;
    }

    return {
        handle,
        /** Read-only view of the mock state (dev / test introspection). */
        snapshot: () => ({
            gpr: Array.from(state.gpr),
            pc: state.pc,
            timeVirt: state.timeVirt,
            halted: state.halted,
            haltReason: state.haltReason,
            memory: Object.fromEntries(state.memory),
            topics: [...state.topics],
            // Breakpoint table as `[addr, id]` pairs — the table is keyed
            // by address, so the id → address resolution done by
            // `ClearBreakpoint` is observable here (dev / test only).
            breakpoints: [...state.breakpoints],
            // Trace recording state (contract v3) — dev / test only.
            traceEnabled: state.traceEnabled,
            traceLen: state.traceLen,
            traceDropped: state.traceDropped,
            seed,
        }),
    };
}
