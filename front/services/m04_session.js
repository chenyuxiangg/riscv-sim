// M-04 会话管理器 (Session Manager).
//
// A session is the *identity* half of the front end: it mints the
// session id, binds it to the M-07 transport (every frame then carries
// `sessionId`, so the kernel can scope snapshots per session — FR-13),
// and owns the session lifecycle:
//
//   idle ──create──▶ creating ──link up──▶ ready
//                                  │          │
//                                  │          ├─link down──▶ reconnecting
//                                  │          │                    │
//                                  │          │◀───link up──────────┘
//                                  ▼          ▼
//                               closing ──▶ closed
//
// The transport connection state and the session state are **not** the
// same thing: the transport can go down while the session is still
// alive (that is what `reconnecting` means). An illegal transition is
// rejected with E_CTX-01 (M-04 LLD §6.1) and leaves the state untouched.
//
// Capability negotiation: contract v2 **does** provide a `Negotiate`
// command (`CommandPayload::Negotiate { min_schema_version, cap_bits }`
// → `EventPayload::Negotiated`), but the front end does **not** call it
// yet — the frozen spec (STAGE-2 §11) does not require the front end to
// negotiate. Until that changes the workbench keeps using the static role
// capability bits of M-01.

import { FRONT_ERROR_CODES } from "../lib/message.js";

/** Legal session state transitions. */
const LEGAL_TRANSITIONS = Object.freeze({
    idle: ["creating", "closed"],
    creating: ["ready", "reconnecting", "closed"],
    ready: ["reconnecting", "closing", "closed"],
    reconnecting: ["ready", "closing", "closed"],
    closing: ["closed"],
    closed: [],
});

/**
 * @typedef {object} SessionDescriptor
 * @property {string|null} id
 * @property {string|null} role
 * @property {string} state
 * @property {number|null} createdAt
 * @property {string|null} lastError
 */

/**
 * @param {string} code
 * @param {string} message
 * @returns {Error & {code: string}}
 */
function sessionError(code, message) {
    const err = new Error(`${code}: ${message}`);
    /** @type {any} */ (err).code = code;
    return err;
}

let sessionCounter = 0;

/**
 * Mint a session id.
 *
 * @returns {string}
 */
function mintSessionId() {
    sessionCounter += 1;
    const rand = Math.floor(Math.random() * 0xffffff).toString(16).padStart(6, "0");
    return `sess-${sessionCounter}-${rand}`;
}

/**
 * Mount the session manager.
 *
 * @param {{transport?: object|null, commandRouter?: object|null}} [options]
 */
export async function mountSession(options = {}) {
    const transport = options.transport ?? null;

    /** @type {SessionDescriptor} */
    let session = {
        id: null,
        role: null,
        state: "idle",
        createdAt: null,
        lastError: null,
    };

    /** @type {Set<(session: SessionDescriptor) => void>} */
    const subscribers = new Set();

    /**
     * Apply a state transition. Illegal transitions are rejected and the
     * state is left untouched.
     *
     * @param {string} next
     * @param {string|null} [error]
     * @returns {SessionDescriptor}
     */
    function transition(next, error = null) {
        const legal = LEGAL_TRANSITIONS[session.state] ?? [];
        if (!legal.includes(next)) {
            const err = sessionError(
                FRONT_ERROR_CODES["E_CTX-01"],
                `非法会话状态迁移：${session.state} → ${next}`,
            );
            session = { ...session, lastError: err.message };
            notify();
            throw err;
        }
        session = { ...session, state: next, lastError: error };
        notify();
        return session;
    }

    function notify() {
        for (const fn of [...subscribers]) {
            try {
                fn(getSession());
            } catch (err) {
                console.warn("[M-04] session subscriber failed:", err);
            }
        }
    }

    /**
     * Create a session and bind it to the transport.
     *
     * The session becomes `ready` as soon as the transport channel is
     * open; when the transport is still down the session stays in
     * `creating` and flips to `ready` as soon as the link comes up.
     *
     * @param {{role?: string, id?: string}} [opts]
     * @returns {Promise<SessionDescriptor>}
     */
    async function createSession(opts = {}) {
        if (session.state === "creating" || session.state === "ready") {
            throw sessionError(FRONT_ERROR_CODES["E_CTX-01"], `会话已存在（state=${session.state}）`);
        }
        const id = opts.id ?? mintSessionId();
        const role = opts.role ?? null;
        session = {
            id,
            role,
            state: "idle",
            createdAt: Date.now(),
            lastError: null,
        };
        transition("creating");
        transport?.bindSession?.(id, role);
        // `bindSession` notifies the transport subscribers synchronously —
        // and this session is one of them, so the link-up handler may
        // already have moved the session to `ready`. Only advance when
        // it is still `creating` (an illegal `ready → ready` transition
        // would abort the whole session handshake).
        if (session.state === "creating" && transport?.getTransportStatus?.().connected) {
            transition("ready");
        }
        return getSession();
    }

    /**
     * Close the current session. Idempotent on an already closed
     * session (returns the descriptor instead of throwing) — closing a
     * session that never existed is E_CTX-11.
     *
     * @returns {Promise<SessionDescriptor>}
     */
    async function closeSession() {
        if (session.state === "closed" || session.state === "idle") {
            if (session.id === null) {
                // ⚠️ **v0.44 批（STAGE-29 · 裁决 E）之前**这里写的是裸字面量
                //    `throw sessionError("E_CTX-11", "会话不存在")` ⇒ ⇒ 那个码
                //    **没有定义处** ⇒ ⇒ 「唯一定义」对前端码结构上无法生效
                //    ⇒ ⇒ 现在从 `FRONT_ERROR_CODES` 取 ⇒ ⇒ 定义处在前端。
                throw sessionError(FRONT_ERROR_CODES["E_CTX-11"], "会话不存在");
            }
            return getSession();
        }
        transition("closing");
        transport?.unbindSession?.();
        transition("closed");
        return getSession();
    }

    /**
     * The current session descriptor (a copy — callers cannot mutate the
     * manager's state by accident).
     *
     * @returns {SessionDescriptor}
     */
    function getSession() {
        return { ...session };
    }

    /**
     * Subscribe to session state changes. Returns the unsubscribe fn.
     *
     * @param {(session: SessionDescriptor) => void} handler
     * @returns {() => void}
     */
    function subscribe(handler) {
        subscribers.add(handler);
        return () => subscribers.delete(handler);
    }

    // The transport's connection state drives the reconnect semantics.
    const offStatus = transport?.onStatus?.((st) => {
        if (st.connected) {
            if (session.state === "creating" || session.state === "reconnecting") {
                // Transition **before** re-binding: `bindSession` notifies
                // the transport subscribers synchronously, so this
                // handler runs again — and it must not re-enter.
                transition("ready");
                transport?.bindSession?.(session.id, session.role);
            }
            return;
        }
        if (session.state === "ready") {
            transition("reconnecting");
        }
    }) ?? null;

    return {
        createSession,
        closeSession,
        getSession,
        subscribe,
        /** True when the session can carry commands. */
        isReady: () => session.state === "ready",
        close() {
            offStatus?.();
            subscribers.clear();
        },
    };
}
