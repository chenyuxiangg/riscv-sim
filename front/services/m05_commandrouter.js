// M-05 命令路由与事件分发 (Command router & event dispatch).
//
// Two directions, one contract:
//
//   outbound  : a caller hands over a `CommandPayload` value (the
//               `front/lib/message.js` builders) → the router wraps it in
//               the frozen `Message` envelope → M-07 puts it on the wire.
//   inbound   : M-07 hands over a transport frame → the router unwraps
//               the frozen `Message` envelope → the `EventPayload` is
//               dispatched to the topic subscribers.
//
// Correlation: the frozen `Message` envelope has no request-id field, so
// the correlation id travels in the M-07 transport frame (`reqId`, see
// `front/lib/frame.js`). A command is resolved by the first event that
// carries its `reqId`; an `Error` event rejects the command instead.
//
// Timeouts follow the M-05 LLD: a command that is not answered within
// 500 ms is rejected with E_CTX-06 (read-only / idempotent commands may
// be retried by the caller).

import {
    Command,
    commandMessage,
    decodeMessage,
    eventTopic,
    protocolError,
    TOPIC_ALL,
} from "../lib/message.js";

/** Default command timeout (M-05 LLD §2.2 — ≤500ms). */
export const DEFAULT_COMMAND_TIMEOUT_MS = 500;

/**
 * Mount the command router.
 *
 * @param {{transport?: object|null}} [options]
 */
export async function mountCommandRouter(options = {}) {
    const transport = options.transport ?? null;

    /** Monotonic protocol sequence (the `Message.seq` of outbound commands). */
    let seq = 0;
    /** Monotonic correlation counter (the transport frame's `reqId`). */
    let reqCounter = 0;

    /** @type {Map<number, {resolve: Function, reject: Function, timer: any}>} */
    const pending = new Map();
    /** @type {Map<string, Set<Function>>} */
    const subscribers = new Map();

    // -- events --------------------------------------------------------------

    /**
     * Dispatch an event to its topic subscribers (and to `"*"`).
     *
     * @param {object} event
     */
    function publish(event) {
        for (const topic of [event.topic, TOPIC_ALL]) {
            const set = subscribers.get(topic);
            if (!set) continue;
            for (const fn of [...set]) {
                try {
                    fn(event);
                } catch (err) {
                    console.warn(`[M-05] subscriber for "${topic}" failed:`, err);
                }
            }
        }
    }

    /**
     * Subscribe to an event topic. Returns the unsubscribe fn.
     *
     * @param {string} topic — e.g. `"tick"`, `"halted"`, `"memory.read"`, or `"*"`
     * @param {(event: object) => void} handler
     * @returns {() => void}
     */
    function subscribe(topic, handler) {
        let set = subscribers.get(topic);
        if (!set) {
            set = new Set();
            subscribers.set(topic, set);
        }
        set.add(handler);
        return () => {
            unsubscribe(topic, handler);
        };
    }

    /**
     * Remove one subscriber.
     *
     * @param {string} topic
     * @param {Function} handler
     * @returns {boolean} whether a subscriber was removed
     */
    function unsubscribe(topic, handler) {
        const set = subscribers.get(topic);
        if (!set) return false;
        const removed = set.delete(handler);
        if (set.size === 0) subscribers.delete(topic);
        return removed;
    }

    /**
     * One-shot subscription: resolves with the first matching event.
     *
     * The optional observer is called with the matched event **once**:
     * the subscription is already gone by then, so a second event of the
     * same topic never reaches it. Without the observer the one-shot
     * semantics were only observable from the outside through
     * `topics()` — the handler itself was invisible, so no gate could
     * prove that it stops firing (F8).
     *
     * @param {string} topic
     * @param {(event: object) => void} [onEvent]
     * @returns {Promise<object>}
     */
    function once(topic, onEvent) {
        return new Promise((resolve) => {
            const off = subscribe(topic, (event) => {
                off();
                resolve(event);
                onEvent?.(event);
            });
        });
    }

    // -- inbound -------------------------------------------------------------

    /**
     * Handle one inbound transport frame.
     *
     * @param {object} frame
     */
    function handleFrame(frame) {
        if (!frame || frame.type !== "event") return;
        let decoded;
        try {
            decoded = decodeMessage(frame.payload);
        } catch (err) {
            publish({
                variant: "Error",
                value: {
                    code: err?.code ?? "E_PROTO-08",
                    message: err?.message ?? String(err),
                },
                topic: "error",
                seq: frame.seq ?? null,
                reqId: null,
                sessionId: frame.sessionId ?? null,
            });
            return;
        }
        if (decoded.direction !== "Event") return;

        const event = {
            variant: decoded.variant,
            value: decoded.value,
            topic: eventTopic(decoded.variant),
            seq: decoded.seq,
            reqId: frame.reqId ?? null,
            sessionId: frame.sessionId ?? null,
        };

        // Correlate with the command that caused it.
        if (event.reqId !== null && pending.has(event.reqId)) {
            const entry = pending.get(event.reqId);
            pending.delete(event.reqId);
            clearTimeout(entry.timer);
            if (event.variant === "Error") {
                entry.reject(protocolError(
                    event.value?.code ?? "E_CTX-05",
                    event.value?.message ?? "kernel error",
                ));
            } else {
                entry.resolve(event);
            }
        }

        publish(event);
    }

    // -- outbound ------------------------------------------------------------

    /**
     * Send a command and wait for the matching response event.
     *
     * @param {object|string} payload — inner `CommandPayload` value
     * @param {{timeoutMs?: number}} [opts]
     * @returns {Promise<object>} the response event
     */
    function dispatchCommand(payload, opts = {}) {
        const timeoutMs = Number(opts.timeoutMs ?? DEFAULT_COMMAND_TIMEOUT_MS);
        if (!transport) {
            return Promise.reject(protocolError("E_TRX-01", "未挂载传输适配器（M-07）"));
        }
        seq += 1;
        const message = commandMessage(seq, payload);
        const reqId = (reqCounter += 1);
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                pending.delete(reqId);
                reject(protocolError(
                    "E_CTX-06",
                    `命令超时（${timeoutMs}ms）：${describePayload(payload)}`,
                ));
            }, timeoutMs);
            pending.set(reqId, { resolve, reject, timer });
            try {
                transport.sendCommand(message, { reqId });
            } catch (err) {
                pending.delete(reqId);
                clearTimeout(timer);
                reject(err);
            }
        });
    }

    // -- lifecycle -----------------------------------------------------------

    /** Reject every in-flight command (the link went away). */
    function failPending(code, message) {
        for (const [reqId, entry] of [...pending.entries()]) {
            pending.delete(reqId);
            clearTimeout(entry.timer);
            entry.reject(protocolError(code, message));
        }
    }

    const offMessage = transport?.onMessage?.(handleFrame) ?? null;
    const offStatus = transport?.onStatus?.((st) => {
        if (!st.connected) failPending("E_CTX-06", "传输通道断开，命令未完成");
    }) ?? null;

    function close() {
        failPending("E_CTX-06", "命令路由器已关闭");
        offMessage?.();
        offStatus?.();
        subscribers.clear();
    }

    return {
        dispatchCommand,
        subscribe,
        unsubscribe,
        once,
        publish,
        handleFrame,
        close,
        /** Number of commands still waiting for a response. */
        pendingCount: () => pending.size,
        /** Topics that currently have at least one subscriber. */
        topics: () => [...subscribers.keys()],
        /** The command builders, re-exported so callers need one import. */
        Command,
    };
}

/**
 * Human-readable label of a command payload (for error messages).
 *
 * @param {object|string} payload
 * @returns {string}
 */
function describePayload(payload) {
    if (typeof payload === "string") return payload;
    if (payload && typeof payload === "object") {
        const keys = Object.keys(payload);
        if (keys.length === 1) return keys[0];
    }
    return "command";
}
