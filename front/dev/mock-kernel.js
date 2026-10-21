// Dev-only mock kernel (pure JS, no DOM / no Worker API).
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
} from "../lib/message.js";
import { decodeFrame, encodeFrame, FRAME_TYPE } from "../lib/frame.js";

/** RAM window of the toy platform. */
export const MOCK_RAM = Object.freeze({ base: 0x0000_0000, size: 0x1_0000 });

/** Address of the synthetic breakpoint at the end of the toy program. */
export const MOCK_BREAKPOINT_ADDR = 0x40;

/** Kernel descriptor reported by the `Negotiate` reply. */
export const MOCK_KERNEL_ID = "riscv_sim_mock/0.1.0 RV32IMC";

/** Number of general-purpose registers. */
const GPR_COUNT = 32;

/** Virtual time added per executed instruction (picoseconds). */
const TICK_PS = 1000;

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
                if (count === 0) break; // 空区间没有内容 → 不回复
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
                frames.push(makeFrame(reqId, Event.negotiated(
                    SCHEMA_VERSION,
                    Number(value.cap_bits ?? 0),
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
            seed,
        }),
    };
}
