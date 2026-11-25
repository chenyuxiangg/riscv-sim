// M-07 传输适配器 (Transport Adapter).
//
// Two adapters, one protocol:
//   - browser-mode : postMessage to a Web Worker hosting the kernel
//                    (`front/kernel-worker.js` — the WASM kernel from
//                    `crates/kernel`, stage 1).
//   - server-mode  : **两条通道** —— 事件走 WebSocket
//                    (`GET /api/sessions/{id}/events/ws`，服务端只往外推，R-13)；
//                    命令走 `POST /api/sessions/{id}/command`（整条契约消息
//                    进出，回体是应答帧，v1.11）。
//                    给 `url`，或给 `serverUrl + sessionId` 让它自己推地址。
//
// Both carry the **frozen** `riscv_sim_contract::protocol::Message`
// envelope inside an M-07 transport frame (see `front/lib/frame.js` and
// `front/lib/message.js` — the two are the only places that know the
// wire format).
//
// Hard constraint (AGENTS.md): ES modules *and* Web Workers require a
// same-origin HTTP(S) origin. Opening `front/index.html` via `file://`
// is blocked by the browser — serve it with
// `python3 tools/serve-front.py` (the repo's one landing point; it also
// sends `Cache-Control: no-store` ⇒ ⇒ the browser never serves a stale
// module ⇒ ⇒ "I changed it but the page looks the same" cannot happen).
//
// The status is **live**: `getTransportStatus()` returns the current
// snapshot, and `onStatus()` delivers every change to subscribers (a
// one-shot snapshot would let the status bar go stale the moment the
// kernel connects).

import { encodeFrame, decodeFrame, FRAME_TYPE } from "../lib/frame.js";
import {
    Event,
    eventMessage,
    FRONT_ERROR_CODES,
    MIRRORED_ERROR_CODES,
} from "../lib/message.js";
import { postCommand } from "./m07_servercommand.js";

/**
 * @typedef {object} TransportStatus
 * @property {"none"|"worker"|"websocket"} mode      — which adapter is in use
 * @property {boolean} connected                    — is the channel open now
 * @property {"connecting"|"ready"|"closed"|"reconnecting"} connState
 * @property {string|null} lastError                — last error, if any
 * @property {string|null} lastResume                — last back-pressure notice
 *                                                     from the server (R-13)
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
    lastResume: null,
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
 * The server's event-stream URL for a session (R-13).
 *
 * `riscv_sim_server` 的端点形状是 `GET /api/sessions/{id}/events/ws`，
 * 它推的就是 M-07 帧 ⇒ ⇒ server-mode 只要把地址指到这里就能解，不需要
 * 第二套解码器。
 *
 * 接受三种入参：已经完整的 `ws(s)://…`、`http(s)://host:port`（自动换成
 * `ws(s)`）、裸 `host:port`（补 `http://` 再换）。
 *
 * @param {string} serverUrl
 * @param {string} sessionId
 * @returns {string}
 */
export function serverEventsUrl(serverUrl, sessionId) {
    if (!sessionId) {
        throw transportError(FRONT_ERROR_CODES["E_TRX-03"], "server-mode 需要 sessionId");
    }
    const base = String(serverUrl ?? "").trim().replace(/\/+$/, "");
    if (!base) {
        throw transportError(FRONT_ERROR_CODES["E_TRX-03"], "server-mode 需要 serverUrl");
    }
    let ws;
    if (/^wss?:\/\//i.test(base)) ws = base;
    else if (/^https?:\/\//i.test(base)) ws = base.replace(/^http/i, "ws");
    else ws = `ws://${base}`;
    return `${ws}/api/sessions/${encodeURIComponent(sessionId)}/events/ws`;
}

/**
 * 把服务端 `resume` 帧的 `payload` 说成**人话**（R-13：丢帧必须显式报
 * 「丢了多少、从哪个 seq 起」）。
 *
 * 服务端目前有三种 `resume` 用途，`payload` 的形状各不相同：
 *   - 事件环丢窗口 `{lost:true, next}`；
 *   - console 丢窗口 `{topic:"sim.console", lost:true, from, base}`；
 *   - 会话没了 / 关闭 `{closed:true, reason, status}`。
 *
 * @param {object} payload
 * @returns {string}
 */
export function summarizeResume(payload) {
    const p = /** @type {any} */ (payload ?? {});
    if (p.closed) {
        return `通道关闭（${p.reason ?? "unknown"}${p.status ? ` / HTTP ${p.status}` : ""}）`;
    }
    if (p.lost) {
        if (p.topic === "sim.console") {
            return `console 丢窗口（从 ${p.from} 起，服务端能给的起始 offset = ${p.base}）`;
        }
        return `事件丢帧（从 seq ${p.next} 起续读）`;
    }
    return "resume（无丢帧信息）";
}

/**
 * Server-mode adapter: WebSocket to `riscv_sim_server`'s event stream (R-13).
 *
 * 两条通道，一件事：
 *   - **事件**走 WebSocket（服务端只往外推，帧形就是 M-07）；
 *   - **命令**走 `POST /api/sessions/{id}/command`（契约命令端点，v1.11）
 *     —— 请求体是 M-05 发的那条契约消息原样 + 帧的 `reqId`，回体是这条
 *     命令的**应答帧**（带 `reqId`）⇒ ⇒ 喂进同一条 `emitFrame`，M-05 的
 *     配对 / 超时 / 错误分派一个字不用改。
 *
 * ⚠️ 只给了 `url`（裸事件通道地址）而没有 `serverUrl + sessionId` 时，
 *    命令**无处可发** ⇒ ⇒ `send` 明确报 `E_TRX-03`，不做「发出去没人看」的
 *    静默失败。
 *
 * @param {{url?: string|null, serverUrl?: string|null, sessionId?: string|null,
 *         token?: string|null, fetchImpl?: typeof fetch}} options
 * @returns {{ok: true, adapter: object, ready: boolean}|{ok: false, error: string}}
 */
function createWebSocketAdapter(options) {
    let url = options.url ?? null;
    if (!url && options.serverUrl && options.sessionId) {
        try {
            url = serverEventsUrl(options.serverUrl, options.sessionId);
        } catch (err) {
            return { ok: false, error: err?.message ?? String(err) };
        }
    }
    if (!url) {
        return {
            ok: false,
            error: `${FRONT_ERROR_CODES["E_TRX-03"]} 未配置 WebSocket URL（给 url，或给 serverUrl + sessionId 由它推出 /api/sessions/{id}/events/ws）`,
        };
    }
    if (typeof WebSocket === "undefined") {
        return { ok: false, error: `${FRONT_ERROR_CODES["E_TRX-03"]} 当前环境没有 WebSocket` };
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
        // ⚠️ `resume` 帧**先入状态**再交给订阅者：状态栏能立刻说「丢了什么」，
        //    订阅者照样能拿到原始帧（谁想自己处理都行）。
        if (frame.type === FRAME_TYPE.Resume) {
            setStatus({ lastResume: summarizeResume(frame.payload) });
        }
        emitFrame(frame);
    };
    // 命令面要有 REST 基址：裸 `url` 只够订阅事件 ⇒ ⇒ 这里一次算好，
    // 每次发命令不再重复拼。
    const commandBase = options.serverUrl && options.sessionId
        ? {
            serverUrl: options.serverUrl,
            sessionId: options.sessionId,
            token: options.token ?? null,
            fetchImpl: options.fetchImpl,
        }
        : null;
    return {
        ok: true,
        // A WebSocket is only up after the handshake — the adapter
        // drives the status from its `onopen` / `onclose` callbacks.
        ready: false,
        adapter: {
            send(frame) {
                if (!commandBase) {
                    throw transportError(
                        FRONT_ERROR_CODES["E_TRX-03"],
                        "只配了事件通道地址（url）：命令无处可发（给 serverUrl + sessionId 走契约命令端点）",
                    );
                }
                // ⚠️ 异步失败也必须回成一条 **Error 事件帧**：M-05 按帧的
                //    `reqId` 配对，`throw` 只对同步路径有效 ⇒ ⇒ 不回帧就是
                //    让命令白等 500 ms 再报「超时」（把真因藏起来）。
                void postCommand({
                    ...commandBase,
                    message: frame.payload,
                    reqId: frame.reqId ?? null,
                })
                    .then((frames) => {
                        for (const raw of frames) {
                            try {
                                emitFrame(decodeFrame(raw));
                            } catch (err) {
                                setStatus({ lastError: err?.message ?? String(err) });
                            }
                        }
                    })
                    .catch((err) => emitSyntheticError(frame, err));
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

/**
 * 合成帧的号段（负数）。
 *
 * ⚠️ 这些帧**不上线**，只在本页的路由里走一遭（把异步失败变成一条
 *    `Error` 事件，让 M-05 拒掉对应命令）；用负数是为了让人一眼看出
 *    「它不是服务端发来的帧」，也保证不会与事件流的 seq 撞号。
 */
let syntheticSeq = 0;

/**
 * 把一次命令失败回成一条 `Error` 事件帧（保留原命令的 `reqId`）。
 *
 * @param {{sessionId?: string|null, role?: string|null, reqId?: number|null}} frame
 * @param {unknown} err
 */
function emitSyntheticError(frame, err) {
    syntheticSeq -= 1;
    const code = /** @type {any} */ (err)?.code ?? MIRRORED_ERROR_CODES["E_CTX-05"];
    emitFrame({
        type: FRAME_TYPE.Event,
        seq: syntheticSeq,
        sessionId: frame?.sessionId ?? null,
        role: frame?.role ?? null,
        reqId: frame?.reqId ?? null,
        crc: null,
        payload: eventMessage(
            syntheticSeq,
            Event.error(code, /** @type {any} */ (err)?.message ?? String(err)),
        ),
    });
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
 * @param {{mode?: "worker"|"websocket", workerUrl?: string, url?: string|null, serverUrl?: string|null, sessionId?: string|null, role?: string|null, createWorker?: (url: string) => any}} [options]
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
    const label = mode === "websocket" ? "server websocket" : "wasm worker";
    setStatus({
        mode,
        connected: false,
        connState: "connecting",
        lastError: null,
        lastResume: null,
        // ⚠️ server-mode 的会话在**挂载时就绑定**（R-13：端点地址要靠它）⇒ ⇒
        //    状态里的 `sessionId` 必须与请求里的那一条是同一个。
        sessionId: mode === "websocket" ? options.sessionId ?? null : null,
        role: mode === "websocket" ? options.role ?? null : null,
        label,
    });

    const built = mode === "websocket"
        ? createWebSocketAdapter({
            url: options.url ?? null,
            serverUrl: options.serverUrl ?? null,
            sessionId: options.sessionId ?? null,
            token: options.token ?? null,
            fetchImpl: options.fetchImpl,
        })
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
