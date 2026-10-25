// M-07 传输适配器 (Transport Adapter).
//
// Two adapters, one protocol:
//   - browser-mode : postMessage to a Web Worker hosting the kernel
//                    (`front/kernel-worker.js` — the WASM kernel from
//                    `crates/kernel`, stage 1).
//   - server-mode  : WebSocket to `riscv_sim_server` (P1). Skeleton only:
//                    without a URL it stays in an explicit "not connected"
//                    state.
//
// Both carry the **frozen** `riscv_sim_contract::protocol::Message`
// envelope inside an M-07 transport frame (see `front/lib/frame.js` and
// `front/lib/message.js` — the two are the only places that know the
// wire format).
//
// Hard constraint (AGENTS.md): ES modules *and* Web Workers require a
// same-origin HTTP(S) origin. Opening `front/index.html` via `file://`
// is blocked by the browser — serve it with
// `python3 -m http.server 8080 --directory front`.
//
// The status is **live**: `getTransportStatus()` returns the current
// snapshot, and `onStatus()` delivers every change to subscribers (a
// one-shot snapshot would let the status bar go stale the moment the
// kernel connects).

import { encodeFrame, decodeFrame, FRAME_TYPE } from "../lib/frame.js";
import { FRONT_ERROR_CODES } from "../lib/message.js";

/**
 * @typedef {object} TransportStatus
 * @property {"none"|"worker"|"websocket"} mode      — which adapter is in use
 * @property {boolean} connected                    — is the channel open now
 * @property {"connecting"|"ready"|"closed"|"reconnecting"} connState
 * @property {string|null} lastError                — last error, if any
 * @property {string|null} sessionId                — bound session (M-04)
 * @property {string|null} role                      — bound workbench role
 * @property {number} seq                           — last transmitted sequence
 * @property {string} label                         — human-readable adapter label
 */

/** @type {Omit<TransportStatus, "seq">} */
let status = {
    mode: "none",
    connected: false,
    connState: "closed",
    lastError: null,
    sessionId: null,
    role: null,
    label: "none",
};

/** Monotonic tx sequence — the M-07 `seqNo` anchor (M-07 LLD §2.1.1). */
let txSeq = 0;

/**
 * Error code for a **transport channel** failure (the Worker `error` event /
 * a WebSocket error).
 *
 * ⚠️ **D-48（语义收敛）**：这个码**只**表示「传输通道挂了」。WASM 宿主侧
 * 的失败（WASM 初始化失败 / 内核抛错 / 返回值非法）**不是**传输通道错误
 * ——它由 `front/kernel-worker.js` 用 **`E_TRX-05`** 报。
 *
 * 此前两边共用 `E_TRX-04` ⇒ 前端拿到错误**无法按码分派**（「通道断了要
 * 重连」与「内核算错了要报错并拒掉这条命令」是两种处置）。
 */
export const TRANSPORT_CHANNEL_ERROR_CODE = FRONT_ERROR_CODES["E_TRX-04"];

/** @type {Set<(status: TransportStatus) => void>} */
const statusSubscribers = new Set();

/** @type {Set<(frame: object) => void>} */
const messageSubscribers = new Set();

/** @type {{send: (frame: object) => void, close: () => void}|null} */
let adapter = null;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/**
 * Merge a patch into the transport status and notify subscribers when
 * something actually changed.
 *
 * @param {Partial<TransportStatus>} patch
 * @returns {TransportStatus}
 */
function setStatus(patch) {
    const next = { ...status, ...patch };
    const changed = Object.keys(next).some((k) => /** @type {any} */ (next)[k] !== /** @type {any} */ (status)[k]);
    status = next;
    if (changed) {
        for (const fn of [...statusSubscribers]) {
            try {
                fn(getTransportStatus());
            } catch (err) {
                console.warn("[M-07] status subscriber failed:", err);
            }
        }
    }
    return getTransportStatus();
}

/**
 * The transport state as of now.
 *
 * The front-end gate reads the state from here rather than from a
 * dev-only global (FRONTEND_Review.md R-18).
 *
 * @returns {TransportStatus}
 */
export function getTransportStatus() {
    return { ...status, seq: txSeq };
}

/**
 * Subscribe to transport status changes. Returns the unsubscribe fn.
 *
 * @param {(status: TransportStatus) => void} handler
 * @returns {() => void}
 */
export function onStatus(handler) {
    statusSubscribers.add(handler);
    return () => statusSubscribers.delete(handler);
}

/**
 * Subscribe to inbound transport frames (kernel → host). Returns the
 * unsubscribe fn.
 *
 * @param {(frame: object) => void} handler
 * @returns {() => void}
 */
export function onMessage(handler) {
    messageSubscribers.add(handler);
    return () => messageSubscribers.delete(handler);
}

/** Deliver a frame to every message subscriber. */
function emitFrame(frame) {
    for (const fn of [...messageSubscribers]) {
        try {
            fn(frame);
        } catch (err) {
            console.warn("[M-07] message subscriber failed:", err);
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/**
 * @param {string} code
 * @param {string} message
 * @returns {Error & {code: string}}
 */
function transportError(code, message) {
    const err = new Error(`${code}: ${message}`);
    /** @type {any} */ (err).code = code;
    return err;
}

// ---------------------------------------------------------------------------
// Adapters
// ---------------------------------------------------------------------------

/**
 * Bind a worker-like object. Browsers expose `addEventListener`; the
 * Node `worker_threads` shim used by the gate does not — both are
 * supported so the same adapter code runs in the browser and under test.
 *
 * @param {{addEventListener?: Function, on?: Function, onmessage?: any, onerror?: any}} worker
 * @param {(data: unknown) => void} onFrame
 * @param {(err: unknown) => void} onError
 */
function bindWorker(worker, onFrame, onError) {
    if (typeof worker.addEventListener === "function") {
        worker.addEventListener("message", (ev) => onFrame(ev?.data ?? ev));
        worker.addEventListener("error", (ev) => onError(ev));
        return;
    }
    if (typeof worker.on === "function") {
        worker.on("message", (data) => onFrame(data));
        worker.on("error", (err) => onError(err));
        return;
    }
    throw transportError(FRONT_ERROR_CODES["E_TRX-02"], "Worker 适配器缺少事件接口");
}

/**
 * Browser-mode adapter: postMessage to a Web Worker.
 *
 * @param {{workerUrl: string, createWorker?: (url: string) => any}} options
 * @returns {{ok: true, adapter: object, ready: boolean}|{ok: false, error: string}}
 */
function createWorkerAdapter(options) {
    const factory = options.createWorker
        ?? ((url) => new Worker(url, { type: "module" }));
    let worker;
    try {
        worker = factory(options.workerUrl);
    } catch (err) {
        return {
            ok: false,
            error: `${FRONT_ERROR_CODES["E_TRX-02"]} 无法创建 Worker（${options.workerUrl}）：${err?.message ?? err}`,
        };
    }
    try {
        bindWorker(
            worker,
            (data) => {
                let frame;
                try {
                    frame = decodeFrame(data);
                } catch (err) {
                    setStatus({ lastError: err?.message ?? String(err) });
                    return;
                }
                emitFrame(frame);
            },
            (err) => {
                setStatus({
                    connected: false,
                    connState: "closed",
                    lastError: `${TRANSPORT_CHANNEL_ERROR_CODE} Worker 通道错误：${err?.message ?? err}`,
                });
            },
        );
    } catch (err) {
        return { ok: false, error: err?.message ?? String(err) };
    }
    return {
        ok: true,
        // A Worker is "up" as soon as it was constructed — a script that
        // fails to load surfaces through the `error` event, which flips
        // the status to `closed`.
        ready: true,
        adapter: {
            send(frame) {
                worker.postMessage(frame);
            },
            close() {
                try {
                    worker.terminate?.();
                } catch (_e) {
                    // The worker is already gone — nothing to clean up.
                }
            },
        },
    };
}

/**
 * Server-mode adapter: WebSocket (P1). Without a URL the transport stays
 * in an explicit "not connected" state — it never pretends to be up.
 *
 * @param {{url?: string|null}} options
 * @returns {{ok: true, adapter: object, ready: boolean}|{ok: false, error: string}}
 */
function createWebSocketAdapter(options) {
    const url = options.url ?? null;
    if (!url) {
        return {
            ok: false,
            error: `${FRONT_ERROR_CODES["E_TRX-03"]} 未配置 WebSocket URL（服务端形态为 P1，当前仅保留接口骨架）`,
        };
    }
    if (typeof WebSocket === "undefined") {
        return { ok: false, error: `${FRONT_ERROR_CODES["E_TRX-03"]} 当前环境没有 WebSocket（服务端形态为 P1）` };
    }
    let socket;
    try {
        socket = new WebSocket(url);
    } catch (err) {
        return { ok: false, error: `${FRONT_ERROR_CODES["E_TRX-03"]} WebSocket 创建失败：${err?.message ?? err}` } ;
    }
    socket.onopen = () => {
        setStatus({ connected: true, connState: "ready", lastError: null });
    };
    socket.onclose = () => {
        setStatus({ connected: false, connState: "closed" });
    };
    socket.onerror = (ev) => {
        setStatus({
            connected: false,
            connState: "closed",
            lastError: `${TRANSPORT_CHANNEL_ERROR_CODE} WebSocket 错误：${ev?.message ?? "unknown"}`,
        });
    };
    socket.onmessage = (ev) => {
        let frame;
        try {
            frame = decodeFrame(JSON.parse(ev.data));
        } catch (err) {
            setStatus({ lastError: err?.message ?? String(err) });
            return;
        }
        emitFrame(frame);
    };
    return {
        ok: true,
        // A WebSocket is only up after the handshake — the adapter
        // drives the status from its `onopen` / `onclose` callbacks.
        ready: false,
        adapter: {
            send(frame) {
                socket.send(JSON.stringify(frame));
            },
            close() {
                try {
                    socket.close();
                } catch (_e) {
                    // already closed
                }
            },
        },
    };
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Mount the transport adapter.
 *
 * The mount **never throws**: a missing `Worker` (Node under the test
 * gate), a `file://` origin, or an absent WebSocket URL all end up as an
 * explicit "not connected" status, so the workbench still comes up and
 * the status bar can explain itself.
 *
 * @param {{mode?: "worker"|"websocket", workerUrl?: string, url?: string|null, createWorker?: (url: string) => any}} [options]
 * @returns {Promise<TransportStatus>}
 */
export async function mountTransport(options = {}) {
    // A re-mount tears the previous adapter down first.
    if (adapter) {
        try {
            adapter.close();
        } catch (_e) {
            // best effort
        }
        adapter = null;
    }
    const mode = options.mode === "websocket" ? "websocket" : "worker";
    const label = mode === "websocket" ? "server websocket (P1)" : "wasm worker";
    setStatus({
        mode,
        connected: false,
        connState: "connecting",
        lastError: null,
        sessionId: null,
        role: null,
        label,
    });

    const built = mode === "websocket"
        ? createWebSocketAdapter({ url: options.url ?? null })
        : createWorkerAdapter({
            workerUrl: options.workerUrl ?? "./kernel-worker.js",
            createWorker: options.createWorker,
        });

    if (!built.ok) {
        setStatus({ connected: false, connState: "closed", lastError: built.error });
        return getTransportStatus();
    }
    adapter = built.adapter;
    if (built.ready) {
        setStatus({ connected: true, connState: "ready", lastError: null });
    }
    return getTransportStatus();
}

/**
 * Send a command message to the kernel.
 *
 * @param {object} message — the frozen contract `Message` envelope
 * @param {{reqId?: number|null}} [options]
 * @returns {number} the transport sequence number of the sent frame
 */
export function sendCommand(message, options = {}) {
    if (!adapter) {
        throw transportError(FRONT_ERROR_CODES["E_TRX-01"], "传输通道未就绪");
    }
    txSeq += 1;
    const frame = encodeFrame({
        type: FRAME_TYPE.Command,
        seq: txSeq,
        sessionId: status.sessionId,
        role: status.role,
        reqId: options.reqId ?? null,
        payload: message,
    });
    adapter.send(frame);
    return txSeq;
}

/**
 * Bind the transport to a session (M-04). The session id travels in
 * every frame so the kernel can scope snapshots per session (FR-13).
 *
 * @param {string|null} sessionId
 * @param {string|null} [role]
 * @returns {TransportStatus}
 */
export function bindSession(sessionId, role = null) {
    return setStatus({ sessionId, role: role ?? status.role });
}

/**
 * Unbind the current session.
 *
 * @returns {TransportStatus}
 */
export function unbindSession() {
    return setStatus({ sessionId: null });
}

/**
 * Close the transport. Idempotent.
 *
 * @returns {TransportStatus}
 */
export function close() {
    if (adapter) {
        try {
            adapter.close();
        } catch (_e) {
            // best effort
        }
        adapter = null;
    }
    return setStatus({ connected: false, connState: "closed" });
}
