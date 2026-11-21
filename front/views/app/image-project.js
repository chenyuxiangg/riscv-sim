// App 层 · 仓外镜像 / 字节工程族（从 app.js 抽出）。
//
// 本模块先收编零外部依赖的纯函数：参数归一化、字节数组校验、URL 取字节、
// 单镜像工程构造。依赖装载链路（loadProjectIntoKernel）的入口函数在后续
// 批次连同命令族一并迁入。

/**
 * A non-negative integer from a command parameter (malformed → fallback).
 *
 * The parameters arrive from the DOM (`data-*` attributes ⇒ strings), so
 * they need a conversion before they reach the contract. A malformed
 * value must not become `NaN` on the wire — that is not valid JSON.
 *
 * @param {unknown} raw
 * @param {number} [fallback]
 * @returns {number}
 */
export function toInt(raw, fallback = 0) {
    const n = Math.trunc(Number(raw));
    return Number.isFinite(n) ? n : fallback;
}

/**
 * 一串 **0..255 的整数**（契约 v5 的 `WriteConsole.bytes`）。
 *
 * ⚠️ **传字符串会被内核按 `E_PROTO-08` 拒掉**（不会默默接受）⇒ 前端
 * 在这里就把它挡下来，并给出**说清是哪个字节**的错。
 *
 * @param {unknown} raw
 * @returns {number[]}
 */
export function toByteArray(raw) {
    if (!Array.isArray(raw)) {
        throw new Error(
            `console-send 的 bytes 必须是字节数组（0..255 的整数），收到：${typeof raw}`,
        );
    }
    return raw.map((b) => {
        const n = Number(b);
        if (!Number.isInteger(n) || n < 0 || n > 255) {
            throw new Error(`console-send 的字节必须是 0..255 的整数：${JSON.stringify(b)}`);
        }
        return n;
    });
}

/**
 * 按 URL 取镜像的**字节**。
 *
 * ⚠️ **内核不联网**：契约的 `ImageSource::Url` 会报 `E_LDR-01` ⇒ 前端
 * 必须**先把字节取回来**，再把字节塞进 `LoadProject`。
 *
 * ⚠️ **不许截断**：`arrayBuffer()` 拿的是**完整**响应体（镜像有几百 KiB：
 * osz 的 ELF 是 282 280 字节）⇒ 不能用「读一部分就停」的方式取。
 *
 * @param {string} url
 * @returns {Promise<Uint8Array>}
 */
export async function fetchImageBytes(url) {
    const res = await fetch(url);
    if (!res.ok) {
        throw new Error(`取镜像字节失败（HTTP ${res.status}）：${url}`);
    }
    const buf = await res.arrayBuffer();
    return new Uint8Array(buf);
}

/**
 * 造一个「只含一个镜像」的工程（`model::Project` 的线上形态）。
 *
 * ⚠️ **`source` 是外部标签枚举**：`model::ImageSource::Bytes(Vec<u8>)`
 * 的 serde 形态是 `{"Bytes":[...]}`（**不是** base64、**不是**字符串）。
 *
 * ⚠️ **`isa` 必须是 `"RV32IMC"`**：契约层的 `ISA` 常量，别的值会被
 * 内核判「工程 ISA 与内核支持的不一致」。
 *
 * @param {string} url — 镜像的来源（只用来取默认名字）
 * @param {Uint8Array|number[]} bytes — 镜像字节
 * @param {{id?: string, name?: string, imageName?: string, kind?: string, boardName?: string, toolchain?: string}} [options]
 * @returns {object}
 */
export function buildImageProject(url, bytes, options = {}) {
    const fileName = String(url).split("/").filter(Boolean).pop() ?? "";
    const imageName = options.imageName ?? (fileName || "image.elf");
    return {
        id: options.id ?? "proj-external-image",
        name: options.name ?? imageName,
        tree: {
            root: {
                name: options.boardName ?? "MainBoard",
                component_id: null,
                children: [],
            },
        },
        images: [{
            name: imageName,
            kind: options.kind ?? "Elf",
            source: { Bytes: Array.from(bytes) },
        }],
        version: {
            schema: 1,
            isa: "RV32IMC",
            toolchain: options.toolchain ?? "external (browser)",
        },
    };
}
