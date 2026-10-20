// WASM 内核传输宿主（第 1 阶段）。
//
// 这是「桥」的**前端一侧**：一侧是 M-07 传输帧，另一侧是真 WASM 内核
// （`crates/kernel` 经 wasm-pack 编译出的 ESM 产物）。形状由
// `docs/detailed-design/STAGE-1_Wasm_Bridge.md` §4 冻结：
//
//   命令帧 ──▶ JSON.stringify(frame.payload) ──▶ WASM `process_message(json)`
//   事件帧 ◀── JSON.parse(返回值) → 每个契约消息包成一个传输帧
//
// 分层口径（与 `front/lib/frame.js` 头注释一致）：**内核只认契约消息**
// （`riscv_sim_contract::protocol::Message`）。传输帧上的
// `sessionId` / `role` / `reqId` 是前端自己的事——内核不解释它们，
// 这里原样回传。
//
// ⚠️ 同源约束（AGENTS.md）：Worker（和 ES module 一样）必须走 HTTP(S)，
// `file://` 直开会被浏览器拒绝。
//
// `self.document` 探针用来区分 Worker 全局作用域与 window：window 有
// `document`，Worker 没有。守卫保证本文件在 Worker 之外被 import 时
// **无副作用**（前端门禁的 ESM 链接检查就在 Node 里 import 它，而那里
// 连 `Worker` 全局都没有）。

import { decodeFrame, FRAME_TYPE } from "./lib/frame.js";
import { eventMessage, Event } from "./lib/message.js";

/**
 * Default location of the wasm-pack ESM build of `crates/kernel`
 * (`tools/build-wasm.sh` → `front/assets/pkg/riscv_sim_kernel.js`).
 *
 * Resolved against **this module's** URL, so it works no matter where
 * the front end is served from. It is a pure computation — evaluating
 * it has no side effect, so it is safe outside the Worker guard.
 */
const DEFAULT_WASM_URL = new URL("./assets/pkg/riscv_sim_kernel.js", import.meta.url).href;

/** Error code for every failure raised by this host (M-07 family). */
const HOST_ERROR_CODE = "E_TRX-04";

if (typeof self !== "undefined" && typeof self.document === "undefined") {
    startKernelHost();
}

/**
 * Boot the WASM kernel host inside a Worker.
 *
 * Everything that can fail — the dynamic `import()`, the wasm-pack init
 * — happens **inside** this function, so importing the module outside a
 * Worker is a no-op.
 */
function startKernelHost() {
    /**
     * The init promise (created once, awaited by every frame).
     *
     * @type {Promise<object>|null}
     */
    let initPromise = null;

    /**
     * Host-synthesised event counter.
     *
     * The **kernel** owns the `seq` space of the events it produces
     * (spec §2.1) — the host must not reuse it. The few events this
     * host synthesises by itself (the error frames) therefore get their
     * own counter.
     */
    let hostSeq = 0;

    /**
     * Number of commands currently **waiting for the WASM init**.
     *
     * Used to decide who reports an init failure: if a command is in
     * flight, its own error frame (which carries the command's `reqId`)
     * is the one that matters — adding a second, boot-level frame for the
     * same fault would report the same failure twice with two different
     * wordings.
     *
     * @type {number}
     */
    let inFlight = 0;

    /**
     * Whether the boot-level (command-less) init-failure frame was already
     * posted. One failure, one boot frame.
     *
     * @type {boolean}
     */
    let bootErrorPosted = false;

    /**
     * Serial chain: frames are handled strictly in arrival order.
     *
     * The kernel is stateful (registers / memory / breakpoints), so two
     * commands must never interleave. A frame that arrives while the
     * wasm init is still running simply waits here — it is processed
     * after the init resolved, in order (the brief's "queue" option).
     *
     * @type {Promise<void>}
     */
    let chain = Promise.resolve();

    /**
     * The URL of the WASM module.
     *
     * ⚠️ **Test seam** — `self.__RISCV_WASM_URL__` overrides the
     * default. The wasm-pack output is **not** committed
     * (`front/assets/pkg/` is gitignored), so the front-end gate injects
     * a fake module through this hook and can therefore run the *real*
     * worker file on a machine that has never built the wasm. The hook
     * lives inside the Worker global scope, so the browser cannot reach
     * it by accident.
     *
     * @returns {string}
     */
    function wasmUrl() {
        const override = self.__RISCV_WASM_URL__;
        return typeof override === "string" && override.length > 0
            ? override
            : DEFAULT_WASM_URL;
    }

    /**
     * Import + initialise the WASM module (once).
     *
     * @returns {Promise<object>} the module namespace
     */
    function ensureKernel() {
        if (!initPromise) {
            initPromise = (async () => {
                const mod = await import(wasmUrl());
                // The wasm-pack `--target web` build exports its init
                // function as `default`; it must run before any other
                // export is callable.
                if (typeof mod?.default === "function") {
                    await mod.default();
                }
                if (typeof mod?.process_message !== "function") {
                    throw new Error("WASM 模块未导出 process_message");
                }
                return mod;
            })();
            // The init can fail before any command arrives. The
            // rejection is reported as an error frame and kills the
            // worker (spec §4: "init failure → the worker throws"), so
            // it must never surface as an *unhandled* rejection.
            initPromise.catch(() => {});
            initPromise.catch((err) => failFast(err));
        }
        return initPromise;
    }

    /**
     * The kernel died at boot: report it once and let the Worker throw
     * so the M-07 adapter turns the worker `error` event into a
     * `closed` transport (spec §4).
     *
     * The boot-level frame (`reqId = null`) is only posted when **no**
     * command is in flight: an in-flight command gets its own error frame
     * with its own `reqId`, and posting both would report the same fault
     * twice with two different wordings.
     *
     * @param {unknown} err
     */
    function failFast(err) {
        if (!bootErrorPosted && inFlight === 0) {
            bootErrorPosted = true;
            const seq = (hostSeq += 1);
            self.postMessage({
                type: FRAME_TYPE.Event,
                seq,
                sessionId: null,
                role: null,
                reqId: null,
                payload: eventMessage(seq, Event.error(
                    HOST_ERROR_CODE,
                    initFailureMessage(err),
                )),
            });
        }
        // Deferred by one tick so the error frame above is delivered
        // before the worker dies.
        setTimeout(() => {
            throw err;
        }, 0);
    }

    /**
     * The canonical wording of an init failure.
     *
     * Both the boot-level frame and the per-command frame use it, so one
     * fault never shows up with two different texts.
     *
     * @param {unknown} err
     * @returns {string}
     */
    function initFailureMessage(err) {
        return `WASM 内核初始化失败：${err?.message ?? err}`;
    }

    /**
     * Report a failure as an `Error` event frame.
     *
     * The frame keeps the command's `reqId`, so M-05 rejects the
     * pending command instead of letting it time out.
     *
     * @param {{sessionId: string|null, role: string|null, reqId: number|null}} frame
     * @param {unknown} err
     */
    function postError(frame, err) {
        const seq = (hostSeq += 1);
        self.postMessage({
            type: FRAME_TYPE.Event,
            seq,
            sessionId: frame.sessionId,
            role: frame.role,
            reqId: frame.reqId,
            payload: eventMessage(seq, Event.error(
                HOST_ERROR_CODE,
                err?.message ?? String(err),
            )),
        });
    }

    /**
     * Check the kernel's answer: a JSON array of contract **event**
     * messages, at least one (spec §2.1).
     *
     * @param {unknown} messages
     * @returns {string|null} the problem, or null when the answer is fine
     */
    function validateEventList(messages) {
        if (!Array.isArray(messages)) {
            return "内核返回的不是事件数组";
        }
        if (messages.length === 0) {
            return "内核没有返回任何事件";
        }
        for (const msg of messages) {
            if (!msg || typeof msg !== "object" || Array.isArray(msg)) {
                return "内核返回了非法事件";
            }
            if (typeof msg.seq !== "number" || !Number.isFinite(msg.seq)) {
                return "内核返回的事件缺少合法 seq";
            }
            if (msg.direction !== "Event") {
                return "内核返回了非事件方向的消息";
            }
        }
        return null;
    }

    /**
     * Handle one inbound frame.
     *
     * @param {unknown} raw
     */
    async function handleFrame(raw) {
        let frame;
        try {
            frame = decodeFrame(raw);
        } catch (_err) {
            // A malformed frame is dropped — the host has nothing to
            // answer with (the mock kernel drops it too).
            return;
        }
        if (frame.type !== FRAME_TYPE.Command) {
            // Only command frames travel host → kernel; anything else
            // (e.g. an event frame echoed back) is ignored.
            return;
        }

        let mod;
        inFlight += 1;
        try {
            mod = await ensureKernel();
        } catch (err) {
            // The init failed. The frame carries the command's `reqId`,
            // so M-05 rejects the pending command instead of letting it
            // time out — and `failFast` will **not** add a second,
            // boot-level frame for the same fault (a command is in
            // flight right now).
            postError(frame, new Error(initFailureMessage(err)));
            return;
        } finally {
            inFlight -= 1;
        }

        let messages;
        try {
            messages = JSON.parse(mod.process_message(JSON.stringify(frame.payload)));
        } catch (err) {
            // A throwing kernel (or a non-JSON answer) must not take the
            // worker down — the command is answered with an error event.
            postError(frame, err);
            return;
        }
        const problem = validateEventList(messages);
        if (problem) {
            postError(frame, new Error(problem));
            return;
        }

        messages.forEach((msg, index) => {
            self.postMessage({
                type: FRAME_TYPE.Event,
                // The frame's seq is the event's own seq (same wording as
                // the mock kernel).
                seq: msg.seq,
                sessionId: frame.sessionId,
                role: frame.role,
                // The **first** event answers the command (M-05 matches
                // it by reqId); the rest are pushed events.
                reqId: index === 0 ? frame.reqId : null,
                payload: msg,
            });
        });
    }

    self.addEventListener("message", (ev) => {
        const raw = ev?.data;
        chain = chain
            .then(() => handleFrame(raw))
            .catch(() => {
                // `handleFrame` never throws — this is the safety net
                // that keeps one bad frame from breaking the chain.
            });
    });

    // Boot the kernel right away: the init is asynchronous, so the
    // frames that arrive before it resolves queue up behind it.
    void ensureKernel();
}
