// M-10 寄存器内存服务 (Register & Memory service).
//
// The service owns the **front-end view of the target state**: the
// general-purpose register file, the program counter, the virtual time
// and the memory cells the workbench has read.
//
// Where the data comes from (all through M-05 → M-07):
//   - registers     : `EventPayload::Registers { index, values }` — the
//                     **first-class** carrier added in contract v2
//                     (topic `sim.registers`). The kernel may still also
//                     push the register file as edge-encoded waveform
//                     streams (`EventPayload::WaveformHistory`, streams
//                     `cpu.gpr.xN` / `cpu.pc`); that path is kept so the
//                     service works against a kernel that has not been
//                     rewired to the new event yet.
//   - virtual time  : `EventPayload::Tick { elapsed_ps, pc }` —
//                     `elapsed_ps` travels as a **decimal string**
//                     (contract v2 §2.2), so it is read back through
//                     `psToNumber()`, which **refuses** to round a value
//                     that does not fit a JS number.
//   - memory        : `EventPayload::MemoryRead { addr, size, value }`.
//
// Read / write requests go the other way: `readRegisters()` uses the
// contract v2 `ReadRegisters` command, `writeRegister()` uses
// `WriteRegister`, `readMemory()` uses `ReadMemory` and `writeMemory()`
// uses `WriteMemory` and reads back to confirm.
//
// ⚠️ **读是钳位、写是拒绝**（规格 §5）：内核把读的 `index` / `count`
// 钳到 32 个寄存器的范围内，并**回报实际起点**；写 x0 被丢弃，写
// 越界（`index > 31`）是**拒绝**（E_DBG-01）而不是钳位——写钳位会
// **静默**写到别的寄存器。所以本服务**不**替内核钳位写索引。
//
// The service pushes its snapshot into the M-03 view store (the
// "寄存器/内存" and "调试" views consume it) and asks the workbench to
// re-render, throttled to ≤200 ms (M-10 LLD §2.2 — the delta push
// budget).

import { Command, WRITE_SIZE, psToNumber, protocolError } from "../lib/message.js";

/** Render push throttle (M-10 LLD — ≤200ms). */
export const RENDER_THROTTLE_MS = 200;

/** Number of general-purpose registers (x0..x31). */
export const GPR_COUNT = 32;

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
     * Merge one waveform history into the register cache.
     *
     * The edge-encoded register streams are the **legacy** carrier: the
     * kernel pushes `cpu.gpr.xN` / `cpu.pc` streams, and only the *last*
     * edge of each stream carries the current value.
     *
     * @param {object} value — the `WaveformHistory` DTO
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
                if (index >= 0 && index < GPR_COUNT) gpr[index] = last.value >>> 0;
                continue;
            }
            if (stream.id === PC_STREAM) {
                pc = last.value >>> 0;
            }
        }
    }

    /**
     * Merge a `Registers` batch into the register cache.
     *
     * ⚠️ The batch starts at the index the **kernel** reports — the kernel
     * clamps the request (contract v2 §5) and replies with the *actual*
     * start index, which is not necessarily the requested one. The
     * service therefore trusts the reply, never the request.
     *
     * @param {object} value — the `Registers` payload
     */
    function applyRegisters(value) {
        const start = Number(value?.index ?? 0);
        const values = Array.isArray(value?.values) ? value.values : [];
        for (let i = 0; i < values.length; i += 1) {
            const reg = start + i;
            if (reg < 0 || reg >= GPR_COUNT) continue;
            gpr[reg] = Number(values[i]) >>> 0;
        }
    }

    /**
     * Merge a `SimStateDelta` into the register cache.
     *
     * @param {object} value — the `SimStateDelta` payload
     */
    function applyStateDelta(value) {
        for (const reg of value?.registers ?? []) {
            const index = Number(reg?.index);
            if (!Number.isInteger(index) || index < 0 || index >= GPR_COUNT) continue;
            gpr[index] = Number(reg?.value) >>> 0;
        }
        if (typeof value?.pc === "number") pc = value.pc >>> 0;
    }

    const offWaveform = router?.subscribe?.("waveform.history", (event) => {
        applyWaveform(event.value);
        notify();
        scheduleRender();
    }) ?? null;

    const offRegisters = router?.subscribe?.("sim.registers", (event) => {
        applyRegisters(event.value);
        notify();
        scheduleRender();
    }) ?? null;

    const offStateDelta = router?.subscribe?.("sim.state.delta", (event) => {
        applyStateDelta(event.value);
        notify();
        scheduleRender();
    }) ?? null;

    const offTick = router?.subscribe?.("tick", (event) => {
        // `elapsed_ps` is a **decimal string** (contract v2 §2.2) — read it
        // back through `psToNumber()`, which throws instead of silently
        // rounding a value that does not fit a JS number.
        //
        // ⚠️ **缺失**的 `elapsed_ps` 是**畸形**（E_PROTO-08），**不是 0**：
        // 把它当成 0 会让虚拟时间**倒退**（上一个 tick 的时间被抹成 0），
        // 时间轴上表现为「时间倒流」。所以这里区分三种情况：
        //   - 缺失 / null → 畸形，保留旧值 + 记 E_PROTO-08；
        //   - 形态非法 → 保留旧值 + 记 E_PROTO-08（`psToNumber` 抛的）；
        //   - 超过 2^53 → 保留旧值 + 记 E_PROTO-04（`psToNumber` 抛的）。
        let elapsed = null;
        const raw = event.value?.elapsed_ps;
        if (raw === undefined || raw === null) {
            lastError = protocolError(
                "E_PROTO-08",
                "Tick 事件缺少 elapsed_ps（虚拟时间字段是必需的）",
            ).message;
        } else {
            try {
                elapsed = psToNumber(raw);
            } catch (err) {
                // The time is **left alone** — a value we cannot represent
                // exactly must not overwrite the last good one.
                lastError = err?.message ?? String(err);
            }
        }
        if (elapsed !== null) timeVirt = elapsed;
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
     * Ask the kernel for a fresh register snapshot.
     *
     * Contract v2 has a **first-class** read command, so this no longer
     * piggybacks on a topic subscription: the kernel clamps the request
     * to the 32-register file and replies with the *actual* start index
     * (spec §5).
     *
     * @param {number} [index] — first register to read (0 = x0)
     * @param {number} [count] — how many consecutive registers to read
     * @returns {Promise<object>} the `Registers` event
     */
    async function readRegisters(index = 0, count = GPR_COUNT) {
        if (!router) throw new Error("E_TRX-01: 未挂载命令路由器（M-05）");
        const event = await router.dispatchCommand(Command.readRegisters(index, count));
        applyRegisters(event.value);
        notify();
        scheduleRender();
        return event;
    }

    /**
     * Write one general-purpose register.
     *
     * ⚠️ The index is **not** clamped here: the kernel rejects an
     * out-of-range write (E_DBG-01) instead of silently writing to a
     * different register (spec §5). Writing x0 is discarded by the
     * kernel but still answered.
     *
     * @param {number} index
     * @param {number} value
     * @returns {Promise<object>} the `Registers` event
     */
    async function writeRegister(index, value) {
        if (!router) throw new Error("E_TRX-01: 未挂载命令路由器（M-05）");
        const event = await router.dispatchCommand(Command.writeRegister(index, value));
        applyRegisters(event.value);
        notify();
        scheduleRender();
        return event;
    }

    /**
     * Subscribe to the kernel-side register topic.
     *
     * This is the **push** side: the kernel keeps sending register
     * updates to the `sim.registers` topic until the subscription dies
     * (a `LoadProject` kills it, and so does a disconnect).
     *
     * @returns {Promise<object>}
     */
    async function subscribeRegisterTopic() {
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
            await subscribeRegisterTopic();
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

    /**
     * 清**寄存器缓存**（新会话 = 新状态）。
     *
     * ⚠️ **状态必须跟着会话一起重建**（D-45 的坑）：卸载工程之后，视图上
     * 留着**上一个程序**的寄存器 / PC / 虚拟时间 ⇒ 用户看到的是一份**不
     * 存在**的状态。
     *
     * ⚠️ **只清缓存，不碰订阅** ⇒ 重订是**另一件事**（`resubscribe()`）⇒
     * 两件事各归各处（与 `waveform.reset()` / `console.reset()` 同形）。
     *
     * @returns {void}
     */
    function reset() {
        for (let i = 0; i < gpr.length; i += 1) gpr[i] = 0;
        pc = 0;
        timeVirt = 0;
        for (const key of Object.keys(memory)) delete memory[key];
        notify();
        flushRender();
    }

    return {
        readRegisters,
        writeRegister,
        readMemory,
        writeMemory,
        loadProject,
        getSnapshot,
        subscribe,
        ensureSubscribed,
        resubscribe,
        /** 清缓存（新会话 = 新状态；不动订阅）。 */
        reset,
        /** Force an immediate store push (the gate uses it instead of waiting). */
        flushRender,
        close() {
            offWaveform?.();
            offRegisters?.();
            offStateDelta?.();
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
