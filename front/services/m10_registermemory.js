// M-10 寄存器内存服务 (Register & Memory service).
//
// The service owns the **front-end view of the target state**: the
// general-purpose register file, the program counter, the virtual time
// and the memory cells the workbench has read.
//
// Where the data comes from (all through M-05 → M-07):
//   - registers / pc : the kernel pushes them as edge-encoded waveform
//                      streams (`EventPayload::WaveformDelta`, streams
//                      `cpu.gpr.xN` / `cpu.pc`). The frozen contract has
//                      no "register snapshot" variant, so the edge
//                      stream is the only contract-native carrier — see
//                      the review doc's "contract gaps" section.
//   - virtual time   : `EventPayload::Tick { elapsed_ps, pc }`.
//   - memory         : `EventPayload::MemoryRead { addr, size, value }`.
//
// Read / write requests go the other way: `readRegisters()` asks the
// kernel for a fresh snapshot (`Subscribe` on the register topic — the
// frozen contract has no "read registers" command), `readMemory()` uses
// `ReadMemory`, `writeMemory()` uses `WriteMemory` and reads back to
// confirm.
//
// The service pushes its snapshot into the M-03 view store (the
// "寄存器/内存" and "调试" views consume it) and asks the workbench to
// re-render, throttled to ≤200 ms (M-10 LLD §2.2 — the delta push
// budget).

import { Command, WRITE_SIZE } from "../lib/message.js";

/** Render push throttle (M-10 LLD — ≤200ms). */
export const RENDER_THROTTLE_MS = 200;

/** Stream ids the kernel uses for the register file. */
const GPR_STREAM = /^cpu\.gpr\.x(\d+)$/;
const PC_STREAM = "cpu.pc";

/**
 * Mount the register & memory service.
 *
 * @param {{commandRouter?: object|null, transport?: object|null, viewStore?: object|null, viewIds?: string[]}} [options]
 */
export async function mountRegisterMemory(options = {}) {
    const router = options.commandRouter ?? null;
    const transport = options.transport ?? null;
    const viewStore = options.viewStore ?? null;
    const viewIds = options.viewIds ?? ["debug", "regmem"];

    /** @type {number[]} */
    const gpr = new Array(32).fill(0);
    let pc = 0;
    let timeVirt = 0;
    /** @type {Record<string, number>} */
    const memory = {};
    let lastError = null;
    let subscribed = false;

    /** @type {Set<(snapshot: object) => void>} */
    const subscribers = new Set();

    /** @type {any} */
    let renderTimer = null;

    // -- snapshot ------------------------------------------------------------

    /**
     * The current target-state snapshot.
     *
     * @returns {{gpr: number[], pc: number, timeVirt: number, memory: Record<string, number>, lastError: string|null}}
     */
    function getSnapshot() {
        return {
            gpr: gpr.slice(),
            pc,
            timeVirt,
            memory: { ...memory },
            lastError,
        };
    }

    function notify() {
        const snapshot = getSnapshot();
        for (const fn of [...subscribers]) {
            try {
                fn(snapshot);
            } catch (err) {
                console.warn("[M-10] snapshot subscriber failed:", err);
            }
        }
    }

    // -- inbound events ------------------------------------------------------

    /**
     * Merge one waveform delta into the register cache.
     *
     * @param {object} value — the `Waveform` DTO
     */
    function applyWaveform(value) {
        const streams = value?.streams ?? [];
        for (const stream of streams) {
            const edges = stream?.edges ?? [];
            if (edges.length === 0) continue;
            const last = edges[edges.length - 1];
            const gprMatch = GPR_STREAM.exec(stream.id ?? "");
            if (gprMatch) {
                const index = Number(gprMatch[1]);
                if (index >= 0 && index < 32) gpr[index] = last.value >>> 0;
                continue;
            }
            if (stream.id === PC_STREAM) {
                pc = last.value >>> 0;
            }
        }
    }

    const offWaveform = router?.subscribe?.("waveform.delta", (event) => {
        applyWaveform(event.value);
        notify();
        scheduleRender();
    }) ?? null;

    const offTick = router?.subscribe?.("tick", (event) => {
        const elapsed = Number(event.value?.elapsed_ps ?? 0);
        if (Number.isFinite(elapsed)) timeVirt = elapsed;
        if (typeof event.value?.pc === "number") pc = event.value.pc >>> 0;
        notify();
        scheduleRender();
    }) ?? null;

    const offMemory = router?.subscribe?.("memory.read", (event) => {
        const addr = event.value?.addr;
        const value = event.value?.value;
        if (typeof addr === "number" && typeof value === "number") {
            memory[formatAddr(addr)] = value >>> 0;
        }
        notify();
        scheduleRender();
    }) ?? null;

    const offError = router?.subscribe?.("error", (event) => {
        lastError = `${event.value?.code ?? "E_PROTO-08"}: ${event.value?.message ?? ""}`;
        notify();
        scheduleRender();
    }) ?? null;

    // -- store / render ------------------------------------------------------

    function scheduleRender() {
        if (renderTimer) return;
        renderTimer = setTimeout(() => {
            renderTimer = null;
            flushRender();
        }, RENDER_THROTTLE_MS);
        // Node's timer keeps the event loop alive; the front-end gate
        // must not hang on a pending render push.
        renderTimer?.unref?.();
    }

    /** Push the snapshot into the view store and ask for a re-render. */
    function flushRender() {
        if (!viewStore) return;
        const snapshot = getSnapshot();
        for (const viewId of viewIds) {
            viewStore.setState(viewId, (prev) => ({
                ...prev,
                timeVirt: snapshot.timeVirt,
                sim: {
                    ...(prev.sim ?? {}),
                    gpr: snapshot.gpr.slice(),
                    pc: snapshot.pc,
                    mem: { ...snapshot.memory },
                },
            }));
            const state = viewStore.getState(viewId);
            if (typeof window !== "undefined" && typeof window.dispatchEvent === "function") {
                window.dispatchEvent(new CustomEvent("riscv:render", {
                    detail: { viewId, type: "update", state },
                }));
            }
        }
    }

    // -- requests ------------------------------------------------------------

    /**
     * Is the transport link up right now? A service mounted without a
     * transport counts as "up" — only a real disconnect invalidates the
     * kernel-side subscription.
     *
     * @returns {boolean}
     */
    function linkIsUp() {
        const status = transport?.getTransportStatus?.();
        return status ? status.connected !== false : true;
    }

    /**
     * Ask the kernel for a fresh register snapshot. Resolves with the
     * snapshot event (a `WaveformDelta`).
     *
     * @returns {Promise<object>}
     */
    async function readRegisters() {
        if (!router) throw new Error("E_TRX-01: 未挂载命令路由器（M-05）");
        const event = await router.dispatchCommand(Command.subscribe("sim.registers"));
        // The kernel-side subscription only survives while the link is
        // up: a disconnect during the round-trip invalidates it, and
        // `ensureSubscribed()` has to subscribe again after the
        // reconnect — otherwise the register deltas are lost (F7).
        if (linkIsUp()) subscribed = true;
        return event;
    }

    /**
     * Read one memory cell.
     *
     * @param {number} addr
     * @param {"Byte"|"Half"|"Word"} [size]
     * @returns {Promise<{addr: number, size: string, value: number}>}
     */
    async function readMemory(addr, size = WRITE_SIZE.Word) {
        if (!router) throw new Error("E_TRX-01: 未挂载命令路由器（M-05）");
        const event = await router.dispatchCommand(Command.readMemory(addr, size));
        return event.value;
    }

    /**
     * Write one memory cell and read it back (the read-back is what
     * updates the cache — the front end never guesses the kernel state).
     *
     * @param {number} addr
     * @param {"Byte"|"Half"|"Word"} [size]
     * @param {number} value
     * @returns {Promise<{addr: number, size: string, value: number}>}
     */
    async function writeMemory(addr, size = WRITE_SIZE.Word, value = 0) {
        if (!router) throw new Error("E_TRX-01: 未挂载命令路由器（M-05）");
        await router.dispatchCommand(Command.writeMemory(addr, size, value));
        return readMemory(addr, size);
    }

    /**
     * Subscribe to snapshot updates. Returns the unsubscribe fn.
     *
     * @param {(snapshot: object) => void} handler
     * @returns {() => void}
     */
    function subscribe(handler) {
        subscribers.add(handler);
        return () => subscribers.delete(handler);
    }

    /**
     * Make sure the kernel is pushing register updates. Idempotent; it
     * retries whenever the transport comes back up.
     */
    async function ensureSubscribed() {
        if (subscribed || !router) return;
        if (!linkIsUp()) return;
        try {
            await readRegisters();
        } catch (err) {
            lastError = err?.message ?? String(err);
            notify();
        }
    }

    const offStatus = transport?.onStatus?.((st) => {
        if (st?.connected === false) {
            // The link went down: the kernel-side subscription died with
            // it. Reset the flag so the next `ensureSubscribed()` (the
            // reconnect path below) subscribes again — otherwise the
            // service keeps believing it is subscribed and the register
            // deltas never come back (F7).
            subscribed = false;
            return;
        }
        ensureSubscribed();
    }) ?? null;

    /**
     * **强制**重新订阅内核侧的寄存器流。
     *
     * `ensureSubscribed()` 是**幂等**的：本地 `subscribed` 标志为真时它
     * 直接返回，什么都不发。但内核侧的订阅是**会死的**——`LoadProject`
     * 会清掉内核的主题集合（新工程 = 新会话，FR-13），此时本地的标志
     * 还是「已订阅」，实际却没有订阅。
     *
     * 结果是：寄存器推送**静默停掉**，视图停在旧值上，没有任何报错。
     * 这个缺陷在 mock 内核时代就存在（mock 的 `reset()` 同样清主题），
     * 直到第 1 阶段前端接上**真内核**、并把「x1 必须是 42」写进真机
     * 断言，它才暴露出来。
     *
     * 所以：**任何 `LoadProject` 之后都必须调这个**，而不是
     * `ensureSubscribed()`。
     *
     * @returns {Promise<void>}
     */
    async function resubscribe() {
        subscribed = false;
        await ensureSubscribed();
    }

    /**
     * 装载一个工程，并**强制**重新订阅寄存器流。
     *
     * `LoadProject` 会清掉内核侧的主题集合（新工程 = 新会话，FR-13），
     * 所以装载之后必须重新订阅，否则寄存器推送**静默停掉**——视图停在
     * 旧值上，没有任何报错。
     *
     * 把这道不变量放在**服务**里（而不是散落在调用点的「记得再订阅一次」
     * 注释里）是它正确的归属：调用点只管调 `loadProject()`，不会有人
     * 忘记那一步。
     *
     * @param {object} project — `model::Project` shaped value
     * @returns {Promise<void>}
     */
    async function loadProject(project) {
        if (!router) throw new Error("E_TRX-01: 未挂载命令路由器（M-05）");
        await router.dispatchCommand(Command.loadProject(project));
        await resubscribe();
    }

    return {
        readRegisters,
        readMemory,
        writeMemory,
        loadProject,
        getSnapshot,
        subscribe,
        ensureSubscribed,
        resubscribe,
        /** Force an immediate store push (the gate uses it instead of waiting). */
        flushRender,
        close() {
            offWaveform?.();
            offTick?.();
            offMemory?.();
            offError?.();
            offStatus?.();
            subscribers.clear();
            if (renderTimer) {
                clearTimeout(renderTimer);
                renderTimer = null;
            }
        },
    };
}

/**
 * Format an address as the canonical `0x%08x` key of the memory cache.
 *
 * @param {number} addr
 * @returns {string}
 */
function formatAddr(addr) {
    return `0x${(addr >>> 0).toString(16).padStart(8, "0")}`;
}
