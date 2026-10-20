// Transport frame — the M-07 wire wrapper around the frozen contract
// message (M-07 LLD §2.1.1 `TransportFrame`, M-05 LLD §2.1.1
// `ProtocolMessage`).
//
// Layering (M-07 LLD 评审 07-2):
//
//   M-07  transport frame  { type, seq, sessionId, role, reqId, payload }
//   M-05  protocol message  = the frozen `riscv_sim_contract::protocol::Message`
//
// The **payload** is the contract message, byte-for-byte the serde JSON
// form (see `front/lib/message.js`). The frame adds the three things the
// frozen envelope does not carry:
//
//   - `sessionId` — which session the message belongs to (M-04 binding);
//   - `role`       — the workbench role (capability面, NFR-04);
//   - `reqId`      — the command/response correlation id.
//
// `reqId` is the correlation handle M-05 needs: the frozen `Message` has
// no request-id field, so a response could not otherwise be matched back
// to the command that caused it. `crc` (M-07 LLD, P1) is not implemented
// yet and is therefore absent.

/** Frame kinds — M-07 LLD §2.1.1 `TransportFrame.type`. */
export const FRAME_TYPE = Object.freeze({
    /** Host → kernel. */
    Command: "command",
    /** Kernel → host. */
    Event: "event",
    /** Kernel → host, resume request for a sequence gap (M-07 `resumeBySeq`). */
    Resume: "resume",
});

/**
 * Build a transport frame.
 *
 * @param {{type: string, seq: number, sessionId?: string|null, role?: string|null, reqId?: number|null, payload: object}} parts
 * @returns {object}
 */
export function encodeFrame(parts) {
    return {
        type: parts.type,
        seq: Number(parts.seq),
        sessionId: parts.sessionId ?? null,
        role: parts.role ?? null,
        reqId: parts.reqId ?? null,
        payload: parts.payload,
    };
}

/**
 * Validate + normalise a frame received from a transport.
 *
 * @param {unknown} raw
 * @returns {{type: string, seq: number, sessionId: string|null, role: string|null, reqId: number|null, payload: object}}
 * @throws {Error & {code: string}} on a malformed frame
 */
export function decodeFrame(raw) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
        throw frameError("E_PROTO-08", "传输帧不是对象");
    }
    const f = /** @type {any} */ (raw);
    if (f.type !== FRAME_TYPE.Command && f.type !== FRAME_TYPE.Event) {
        throw frameError("E_PROTO-08", `未知帧类型：${String(f.type)}`);
    }
    if (typeof f.seq !== "number" || !Number.isFinite(f.seq)) {
        throw frameError("E_PROTO-08", "帧 seq 缺失或非法");
    }
    if (!f.payload || typeof f.payload !== "object") {
        throw frameError("E_PROTO-08", "帧 payload 缺失");
    }
    return {
        type: f.type,
        seq: f.seq,
        sessionId: typeof f.sessionId === "string" ? f.sessionId : null,
        role: typeof f.role === "string" ? f.role : null,
        reqId: typeof f.reqId === "number" && Number.isFinite(f.reqId) ? f.reqId : null,
        payload: f.payload,
    };
}

/**
 * @param {string} code
 * @param {string} message
 * @returns {Error & {code: string}}
 */
function frameError(code, message) {
    const err = new Error(`${code}: ${message}`);
    /** @type {any} */ (err).code = code;
    return err;
}
