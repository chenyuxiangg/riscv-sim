// M-07 · 服务端模式的浏览器门槛（R-13 的「指到那就能用」）。
//
// `m07_transport.js` 只负责**通道**：给 `serverUrl + sessionId` 它就把
// WebSocket 指到 `/api/sessions/{id}/events/ws`。可它不知道该用**哪一条**
// 会话 —— 服务端的会话要先建（`POST /api/sessions`）、要验活（
// `GET /api/sessions/{id}`）、开页时还要把 `pc / 寄存器 / 虚拟时间`
// **取一次**（不然视图要等第一次执行推进才有数）。
//
// 这一层就是这三件事：**读 URL 参数 → 建/验会话 → 水合一次状态**。
// 全部是 REST（`fetch`），WS 那条只读通道由 M-07 自己挂 —— 两件事分开，
// 因为它们的失败模式不同（REST 挂了报 E_TRX-03；WS 挂了报通道错误）。
//
// ⚠️ 服务端有 CORS 闸门（`crates/server/src/api.rs` 的 `cors_headers`）：
//    回环 origin 默认放行；前端不在本机时，起服务的人要给
//    `--cors-origins http://host:port`。
//
// ⚠️ 这一层**只做观察面**：服务端事件通道**只往外推**，命令走 REST。
//    界面上「运行 / 单步 / 断点」那些按钮在 server 模式下仍会报
//    `E_TRX-03`（那是 M-05 → M-07 的写通道），本轮不伪装成能用。

import { FRONT_ERROR_CODES } from "../lib/message.js";

/**
 * @typedef {object} ServerParams
 * @property {string} serverUrl   — 规范化后的 `http(s)://host:port`
 * @property {string|null} sessionId
 * @property {string|null} token
 * @property {string|null} board
 */

/**
 * 解析 `?server=…&session=…&token=…&board=…`。
 *
 * 没给 `server` ⇒ `null`（= 本地 Worker 模式，一个字都不改）。给了就是
 * 服务端模式：`server` 允许缺 scheme（补 `http://`）、允许 `ws(s)://`
 * （换成 `http(s)://`，REST 用 http，WS 由 M-07 自己换回去）。
 *
 * @param {string} [search]
 * @returns {ServerParams|null}
 */
export function parseServerParams(search = "") {
    const query = new URLSearchParams(String(search ?? ""));
    const rawServer = query.get("server");
    if (rawServer === null || rawServer.trim() === "") return null;

    const serverUrl = normalizeServerUrl(rawServer);
    const sessionId = cleanParam(query.get("session"));
    const token = cleanParam(query.get("token"));
    const board = cleanParam(query.get("board"));
    return { serverUrl, sessionId, token, board };
}

/**
 * `?session=` 可能是 `?session=`（空串）或干脆没给 —— 两种都当「没指定」。
 *
 * @param {string|null} value
 * @returns {string|null}
 */
function cleanParam(value) {
    if (value === null) return null;
    const trimmed = value.trim();
    return trimmed === "" ? null : trimmed;
}

/**
 * REST 用的基地址：`ws(s)://` 换成 `http(s)://`，裸 `host:port` 补 `http://`，
 * 末尾的 `/` 去掉（拼端点时不会再长出 `//`）。
 *
 * @param {string} serverUrl
 * @returns {string}
 */
export function normalizeServerUrl(serverUrl) {
    const raw = String(serverUrl ?? "").trim().replace(/\/+$/, "");
    if (!raw) {
        throw serverModeError("server-mode 的 serverUrl 是空的");
    }
    if (/^wss?:\/\//i.test(raw)) {
        return raw.replace(/^ws/i, "http");
    }
    if (/^https?:\/\//i.test(raw)) return raw;
    return `http://${raw}`;
}

/**
 * @param {string} message
 * @returns {Error & {code: string}}
 */
function serverModeError(message) {
    const err = new Error(`${FRONT_ERROR_CODES["E_TRX-03"]}: ${message}`);
    /** @type {any} */ (err).code = FRONT_ERROR_CODES["E_TRX-03"];
    return err;
}

/**
 * 请求头：有令牌就带 `x-auth-token`（服务端的 `require_token` 只认这一个头）。
 *
 * @param {string|null} token
 * @returns {Record<string, string>}
 */
export function authHeaders(token) {
    return token ? { "x-auth-token": token } : {};
}

/**
 * 把服务端的错误体（`{code, detail, status}`）读成人话。
 *
 * @param {Response} response
 * @returns {Promise<string>}
 */
async function readError(response) {
    try {
        const body = await response.json();
        const code = body?.code ? `${body.code} ` : "";
        return `${code}${body?.detail ?? response.statusText ?? ""}（HTTP ${body?.status ?? response.status}）`;
    } catch (_err) {
        return `HTTP ${response.status}${response.statusText ? ` ${response.statusText}` : ""}`;
    }
}

/**
 * 一条请求（统一 `credentials: "omit"` —— 令牌走请求头，不需要 cookie）。
 *
 * @param {string} url
 * @param {{method?: string, headers?: Record<string,string>, body?: string, fetchImpl?: Function, timeoutMs?: number}} options
 * @returns {Promise<any>}
 */
async function requestJson(url, options = {}) {
    const fetchImpl = options.fetchImpl ?? globalThis.fetch;
    if (typeof fetchImpl !== "function") {
        throw serverModeError("当前环境没有 fetch（服务端模式需要它）");
    }
    const init = {
        method: options.method ?? "GET",
        headers: options.headers ?? {},
        credentials: "omit",
    };
    if (options.body !== undefined) init.body = options.body;
    console.debug?.("[M-07] server-mode →", init.method, url);
    const response = await fetchImpl(url, init);
    if (!response.ok) {
        const err = serverModeError(`${url} → ${await readError(response)}`);
        // ⚠️ **HTTP 状态挂在错误对象上**：调用方要按状态分派（「会话没了」是
        // 404 ⇒ 可以重建；500 / 连不上 ⇒ 原样抛）。不靠**消息文本**里认码
        // ——消息是给人看的，不是判据（前端服务层也不许留裸场景码字面量）。
        /** @type {any} */ (err).status = response.status;
        throw err;
    }
    if (response.status === 204) return null;
    return response.json();
}

/**
 * 建/验会话（`POST /api/sessions` · `GET /api/sessions/{id}`）。
 *
 * 语义：
 *   - 给了 `sessionId` ⇒ 先**验活**；活着就用它（不改服务端任何东西）。
 *   - 给了但**已经没了**（404）⇒ 重建一条，并把 `restarted` 标出来 —— 页面
 *     至少还能看，而不是对着一条死会话的 404 白屏。
 *   - 没给 ⇒ 建一条（`board` 可带；不带就是服务端默认板）。
 *
 * @param {ServerParams & {fetchImpl?: Function}} options
 * @returns {Promise<{sessionId: string, created: boolean, restarted: boolean, board: string|null}>}
 */
export async function ensureSession(options) {
    const base = normalizeServerUrl(options.serverUrl);
    const headers = authHeaders(options.token ?? null);
    const fetchImpl = options.fetchImpl;

    if (options.sessionId) {
        const id = options.sessionId;
        const detail = await requestJson(
            `${base}/api/sessions/${encodeURIComponent(id)}`,
            { headers, fetchImpl },
        ).catch((err) => {
            // 只有「没了」（HTTP 404）才重建；连不上 / 令牌不对 / 服务端 500
            // 一律原样抛出 —— 把「连不上」当「会话没了」重建，会掩盖真正的
            // 原因，还会白白占掉一个会话额度。
            if (/** @type {any} */ (err)?.status === 404) {
                return null;
            }
            throw err;
        });
        if (detail) {
            return {
                sessionId: id,
                created: false,
                restarted: false,
                board: detail?.session?.board ?? null,
            };
        }
    }

    const body = options.board ? JSON.stringify({ board: options.board }) : "{}";
    const created = await requestJson(`${base}/api/sessions`, {
        method: "POST",
        headers: { "content-type": "application/json", ...headers },
        body,
        fetchImpl,
    });
    const sessionId = created?.session_id;
    if (!sessionId) {
        throw serverModeError("POST /api/sessions 没回 session_id");
    }
    return {
        sessionId: String(sessionId),
        created: true,
        restarted: Boolean(options.sessionId),
        board: created?.board ?? null,
    };
}

/**
 * 取一次状态：`GET /api/sessions/{id}/state` → `{pc, registers[32], executed, t_ps}`。
 *
 * @param {{serverUrl: string, sessionId: string, token?: string|null, fetchImpl?: Function}} options
 * @returns {Promise<{pc?: number, registers?: number[], executed?: number, t_ps?: number}|null>}
 */
export async function fetchSessionState(options) {
    const base = normalizeServerUrl(options.serverUrl);
    return requestJson(
        `${base}/api/sessions/${encodeURIComponent(options.sessionId)}/state`,
        { headers: authHeaders(options.token ?? null), fetchImpl: options.fetchImpl },
    );
}

/**
 * 把服务端的状态翻译成前端**已经在用**的 `SimStateDelta` 事件值。
 *
 * 服务端的 `registers` 是**32 个裸数**，内核推的 `SimStateDelta` 是
 * `{pc, registers:[{index,value}]}` ⇒ ⇒ 这一层做一次形态对齐，上层（M-10）
 * 不必认第二种寄存器形状。
 *
 * @param {{pc?: number, registers?: (number|{index: number, value: number})[]}|null} state
 * @returns {{pc: number, registers: {index: number, value: number}[]}|null}
 */
export function stateDeltaEvent(state) {
    if (!state) return null;
    const raw = Array.isArray(state.registers) ? state.registers : [];
    const registers = raw.map((entry, index) => {
        if (entry && typeof entry === "object") {
            return { index: Number(entry.index), value: Number(entry.value) >>> 0 };
        }
        return { index, value: Number(entry) >>> 0 };
    });
    const pc = Number(state.pc ?? 0) >>> 0;
    return { pc, registers };
}

/**
 * 把服务端的状态翻译成一条 `Tick` 事件值（虚拟时间 `elapsed_ps` 是**十进制
 * 字符串**，契约 v2 §2.2 —— 前端 `psToNumber()` 就是按这个读的）。
 *
 * `t_ps` 缺失（老服务端 / 未跑过）⇒ 回 `null`：**不**编一个 0 出来，那会让
 * 时间轴看起来倒退。
 *
 * @param {{pc?: number, t_ps?: number|string}|null} state
 * @returns {{pc: number, elapsed_ps: string}|null}
 */
export function tickEvent(state) {
    if (!state || state.t_ps === undefined || state.t_ps === null) return null;
    const ps = String(state.t_ps);
    if (!/^\d+$/.test(ps)) return null;
    return { pc: Number(state.pc ?? 0) >>> 0, elapsed_ps: ps };
}
