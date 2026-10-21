// Transport frame — the M-07 wire wrapper around the frozen contract
// message (M-07 LLD §2.1.1 `TransportFrame`, M-05 LLD §2.1.1
// `ProtocolMessage`).
//
// Layering (M-07 LLD 评审 07-2):
//
//   M-07  transport frame  { type, seq, sessionId, role, reqId, crc, payload }
//   M-05  protocol message  = the frozen `riscv_sim_contract::protocol::Message`
//
// The **payload** is the contract message, byte-for-byte the serde JSON
// form (see `front/lib/message.js`). The frame adds the four things the
// frozen envelope does not carry:
//
//   - `sessionId` — which session the message belongs to (M-04 binding);
//   - `role`       — the workbench role (capability面, NFR-04);
//   - `reqId`      — the command/response correlation id;
//   - `crc`        — the payload's integrity checksum (contract v2 §7.2,
//                    台账 D-32 缺口 6).
//
// `reqId` is the correlation handle M-05 needs: the frozen `Message` has
// no request-id field, so a response could not otherwise be matched back
// to the command that caused it.
//
// `crc` 覆盖的是**载荷**（契约消息）的规范 JSON 序列化：`sessionId` /
// `role` / `reqId` 是会话绑定信息，由 M-04 负责；`seq` 由 M-05 的缺口
// 检测负责。CRC 的职责是「载荷在传输中没被损坏」。
//
// ⚠️ **帧的两端都在 JS 里**（内核只认契约消息，帧是前端的包裹层），
// 所以这一处实现同时覆盖 worker 与主线程两侧。

/** Frame kinds — M-07 LLD §2.1.1 `TransportFrame.type`. */
export const FRAME_TYPE = Object.freeze({
    /** Host → kernel. */
    Command: "command",
    /** Kernel → host. */
    Event: "event",
    /** Kernel → host, resume request for a sequence gap (M-07 `resumeBySeq`). */
    Resume: "resume",
});

/** Error code for a frame whose `crc` does not match its payload. */
export const CRC_MISMATCH_CODE = "E_PROTO-06";

/**
 * CRC-32 lookup table — IEEE 802.3 / zlib polynomial `0xEDB88320`,
 * reflected form.
 *
 * @type {Uint32Array}
 */
const CRC_TABLE = (() => {
    const table = new Uint32Array(256);
    for (let i = 0; i < 256; i += 1) {
        let c = i;
        for (let bit = 0; bit < 8; bit += 1) {
            c = (c & 1) !== 0 ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1);
        }
        table[i] = c >>> 0;
    }
    return table;
})();

/** UTF-8 encoder — the CRC runs over the **bytes** that go on the wire. */
const UTF8 = new TextEncoder();

/**
 * CRC-32 (IEEE 802.3 / zlib, reflected) of a string.
 *
 * The checksum runs over the string's **UTF-8 bytes** — that is what
 * actually travels on the wire, and it is byte-identical to the
 * code-unit interpretation for the pure-ASCII payloads the contract uses.
 *
 * @param {string} text
 * @returns {number} the unsigned 32-bit checksum
 */
export function crc32(text) {
    const bytes = UTF8.encode(String(text));
    let crc = 0xffffffff;
    for (let i = 0; i < bytes.length; i += 1) {
        crc = (crc >>> 8) ^ CRC_TABLE[(crc ^ bytes[i]) & 0xff];
    }
    return (crc ^ 0xffffffff) >>> 0;
}

/**
 * The canonical JSON serialisation of a value: `JSON.stringify` over a
 * **recursively key-sorted** copy.
 *
 * `JSON.stringify` emits the keys in *insertion* order, which is not
 * stable across producers — two frames carrying the same payload could
 * serialise differently and then hash differently. Sorting the keys
 * recursively makes the serialisation a function of the payload's
 * *content* only.
 *
 * ⚠️ **Arrays keep their order** — an array is an ordered sequence, not a
 * set; sorting its elements would change the payload's meaning.
 *
 * @param {unknown} value
 * @returns {any} a value with every object's keys in lexicographic order
 */
export function canonicalize(value) {
    if (Array.isArray(value)) return value.map(canonicalize);
    if (value && typeof value === "object") {
        const out = {};
        for (const key of Object.keys(value).sort()) {
            out[key] = canonicalize(value[key]);
        }
        return out;
    }
    return value;
}

/**
 * The canonical JSON text of a value — the byte sequence the `crc`
 * covers.
 *
 * @param {unknown} value
 * @returns {string}
 */
export function canonicalJson(value) {
    return JSON.stringify(canonicalize(value));
}

/**
 * The `crc` field of a frame: CRC-32 of the canonical JSON of the
 * payload, as **8 lowercase hex digits** (no `0x` prefix).
 *
 * @param {unknown} payload
 * @returns {string}
 */
export function frameCrc(payload) {
    return crc32(canonicalJson(payload)).toString(16).padStart(8, "0");
}

/**
 * Build a transport frame, including its `crc`.
 *
 * @param {{type: string, seq: number, sessionId?: string|null, role?: string|null, reqId?: number|null, payload: object}} parts
 * @returns {object}
 */
export function encodeFrame(parts) {
    if (!parts || typeof parts !== "object") {
        throw frameError("E_PROTO-08", "帧参数非法");
    }
    const payload = parts.payload;
    if (!payload || typeof payload !== "object") {
        throw frameError("E_PROTO-08", "帧 payload 缺失");
    }
    return {
        type: parts.type,
        seq: Number(parts.seq),
        sessionId: parts.sessionId ?? null,
        role: parts.role ?? null,
        reqId: parts.reqId ?? null,
        crc: frameCrc(payload),
        payload,
    };
}

/**
 * Validate + normalise a frame received from a transport.
 *
 * The `crc` check has **two** branches (contract v2 §7.2):
 *
 *   - **no `crc` field** → accepted. Old producers don't carry one, and
 *     the contract's "only add, never change" rule says a frame we have
 *     never seen before must not break the link (E_PROTO-02 is log-only).
 *   - **`crc` present but wrong** → **rejected** with E_PROTO-06. A
 *     producer that computes a checksum and gets it wrong has either a
 *     broken payload or a broken producer — either way the frame must not
 *     be handed on as if it were intact.
 *
 * @param {unknown} raw
 * @returns {{type: string, seq: number, sessionId: string|null, role: string|null, reqId: number|null, crc: string|null, payload: object}}
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
    // `undefined` / `null` means "this producer does not carry a crc" —
    // the old-producer branch. Anything else has to be *exactly* the
    // checksum of the canonical JSON of the payload.
    const crc = f.crc === undefined || f.crc === null ? null : f.crc;
    if (crc !== null) {
        const expected = frameCrc(f.payload);
        if (typeof crc !== "string" || crc !== expected) {
            throw frameError(
                CRC_MISMATCH_CODE,
                `帧 CRC 不符：期望 ${expected}，实得 ${String(crc)}`,
            );
        }
    }
    return {
        type: f.type,
        seq: f.seq,
        sessionId: typeof f.sessionId === "string" ? f.sessionId : null,
        role: typeof f.role === "string" ? f.role : null,
        reqId: typeof f.reqId === "number" && Number.isFinite(f.reqId) ? f.reqId : null,
        crc,
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
