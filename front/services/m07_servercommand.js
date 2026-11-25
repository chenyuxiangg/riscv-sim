// M-07 · 服务端模式的**写通道**（v1.11）。
//
// 服务端的事件流只往外推；命令面是另一个端点：
//
//   POST /api/sessions/{id}/command
//   体 : { message: <冻结的契约 Message>, reqId: <M-07 帧的配对号> }
//   回 : { frames: [ <M-07 事件帧，即这条命令的**应答**，带 reqId> ] }
//
// ⇒ ⇒ 前端**不需要**「契约命令 → REST 端点」的映射表：M-05 发出去的那条契约
// 消息原样进请求体，原生内核 `handle_message` 原样收（服务端只是同一个执行体
// 换了入口）。回体帧与事件流帧同形 ⇒ ⇒ 喂给同一条 `handleFrame` 就能配对。
//
// ⚠️ 回体只有**应答**那一帧：这条命令引发的其余事件（状态增量 / console /
//    波形）走事件流 ⇒ ⇒ 同一份事件只有一条路到页面，不存在「算两遍」。
//
// ⚠️ 这一层只做「拼地址 / 发请求 / 读回体 / 把失败变成可配对的错误」；它
//    **不**翻译任何命令，也不缓存任何状态。

import { RUST_ERROR_CODE_REFS, FRONT_ERROR_CODES, protocolError } from "../lib/message.js";
import { authHeaders, normalizeServerUrl } from "./m07_servermode.js";

/**
 * 契约命令端点的地址。
 *
 * @param {string} serverUrl — `http(s)://host:port`（`ws(s)://` / 裸 `host:port` 也认）
 * @param {string} sessionId
 * @returns {string}
 */
export function commandUrl(serverUrl, sessionId) {
    if (!sessionId) {
        throw protocolError(FRONT_ERROR_CODES["E_TRX-03"], "server-mode 需要 sessionId");
    }
    const base = normalizeServerUrl(serverUrl);
    return `${base}/api/sessions/${encodeURIComponent(sessionId)}/command`;
}

/**
 * 请求体：**整条**契约消息 + 配对号。
 *
 * @param {object} message — 冻结的契约 `Message`
 * @param {number|null} [reqId]
 * @returns {{message: object, reqId: number|null}}
 */
export function commandBody(message, reqId = null) {
    return { message, reqId: reqId ?? null };
}

/**
 * 读服务端的错误体（R-09 的 `{code, detail, status}`），转成可配对的错误。
 *
 * @param {Response} response
 * @returns {Promise<Error & {code: string}>}
 */
async function failureFrom(response) {
    let body = null;
    try {
        body = await response.json();
    } catch (_err) {
        body = null;
    }
    const code = body?.code ?? FRONT_ERROR_CODES["E_TRX-03"];
    const detail = body?.detail ?? response.statusText ?? "server error";
    return protocolError(code, `${detail}（HTTP ${body?.status ?? response.status}）`);
}

/**
 * 回体的 `frames`（这条命令的应答帧）。
 *
 * @param {unknown} body
 * @returns {object[]}
 */
export function framesOf(body) {
    const frames = /** @type {any} */ (body)?.frames;
    if (!Array.isArray(frames) || frames.length === 0) {
        throw protocolError(
            RUST_ERROR_CODE_REFS["E_PROTO-08"],
            "服务端回体没有 frames 数组（这条命令的应答丢了）",
        );
    }
    return frames;
}

/**
 * 发一条契约命令，取回它的应答帧。
 *
 * @param {{serverUrl?: string|null, sessionId?: string|null, token?: string|null,
 *         message: object, reqId?: number|null,
 *         fetchImpl?: typeof fetch}} options
 * @returns {Promise<object[]>}
 */
export async function postCommand(options) {
    const fetchImpl = options?.fetchImpl ?? globalThis.fetch;
    if (typeof fetchImpl !== "function") {
        throw protocolError(FRONT_ERROR_CODES["E_TRX-03"], "当前环境没有 fetch");
    }
    const url = commandUrl(options.serverUrl, options.sessionId);
    let response;
    try {
        response = await fetchImpl(url, {
            method: "POST",
            headers: {
                "content-type": "application/json",
                ...authHeaders(options.token ?? null),
            },
            body: JSON.stringify(commandBody(options.message, options.reqId ?? null)),
        });
    } catch (err) {
        // 网络层失败（服务没起 / 端口不通）也要有码：M-05 按码分派。
        throw protocolError(
            FRONT_ERROR_CODES["E_TRX-03"],
            `命令发不出去（${url}）：${err?.message ?? err}`,
        );
    }
    if (!response.ok) throw await failureFrom(response);
    let body;
    try {
        body = await response.json();
    } catch (err) {
        throw protocolError(
            RUST_ERROR_CODE_REFS["E_PROTO-08"],
            `服务端回体不是 JSON：${err?.message ?? err}`,
        );
    }
    return framesOf(body);
}
