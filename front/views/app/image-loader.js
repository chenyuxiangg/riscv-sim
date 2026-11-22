// App 层 · 镜像装载路径（字节 → 工程 → 装入内核）与脚本口（从 app.js 下沉）。
//
// ⚠️ 单向依赖：project-board 装载底座、image-project 纯构建、视图状态（M-03）
// 与纯函数 lib；不回引门面 ⇒ ⇒ 不形成子模块 ↔ 门面循环。

import * as viewStore from "../m03_viewstore.js";
import { setNote } from "./shared.js";
import { Command } from "../../lib/message.js";
import { fetchImageBytes, buildImageProject } from "./image-project.js";
import { loadProjectIntoKernel } from "./project-board.js";
import { setImageLoadState, publishProductState } from "../components/image-loader.js";

/**
 * 「加载固件」这条命令的**唯一**处理函数（STAGE-33 · v0.48 批）。
 *
 * ⚠️ **为什么抽出来**：曾经有**两个**入口——产品视图里那块面板（视图层，
 * 走 `riscv:command` 信号）与**页头**那个入口（app 层）⇒ ⇒ 两者必须
 * **共用这一个函数**⇒ ⇒ **一条装载路径**（`loadProjectIntoKernel`）⇒ ⇒
 * 重订流与清残留**只有一处**（规格 §3 裁决 G）。
 *
 * ⚠️ **v0.58 批**：入口**收敛成一个**（页头那个）⇒ ⇒ 这个函数是**唯一**
 * 的装载实现（脚本口 `loadFromUrl` 也走它）⇒ ⇒ **一条装载路径**这条规矩
 * **更严**了（不是两条入口凑成一条，而是**只剩一条**）。
 *
 * ⚠️ **与脚本口也共用**：`window.riscv_image.loadFromUrl` 最终走的也是
 * `loadImageFromBytes` ⇒ ⇒ **不写第二套**。
 *
 * ⚠️ **装完就停**（STAGE-32 裁决 D）⇒ ⇒ 这里⛔ **不**发 `ContinueRun`
 * ⇒ ⇒ 用户自己点「继续」。
 *
 * @param {{params?: {bytes?: number[], name?: string, kind?: string}}} detail
 * @param {object} services
 * @returns {Promise<void>}
 */
export async function handleLoadImageCommand(detail, services) {
    const bytes = detail?.params?.bytes;
    if (!Array.isArray(bytes) || bytes.length === 0) {
        setImageLoadState({
            status: "error",
            error: "没有镜像字节 —— 请先用文件选择器选一个固件",
            pending: null,
        });
        publishProductState();
        setNote("加载固件失败：没有镜像字节");
        return;
    }
    // ⚠️ **先写「装载中」再发命令**（v0.58 批 · 幂等的关键）：
    // 这一段是**同步**的（async 函数体在第一个 `await` 之前同步执行）⇒ ⇒
    // 连点两下时，第二下看到的 `status` 已经是 `loading` ⇒ ⇒ `imageLoadDecision`
    // 判成「忽略」⇒ ⇒ **不会**发第二条 `LoadProject`（内核被连着换两次工程）。
    setImageLoadState({ status: "loading", error: null });
    publishProductState();
    try {
        const result = await loadImageFromBytes(services, bytes, {
            name: detail?.params?.name,
            kind: detail?.params?.kind,
        });
        // ⚠️ **产品树**也要换成新工程的（否则还显示上一份工程的树）。
        viewStore.setState("product", (prev) => ({
            ...prev,
            tree: result.project.tree,
        }));
        setImageLoadState({
            status: "ok",
            name: detail?.params?.name ?? "image.bin",
            byteLength: result.byteLength,
            error: null,
            // ⚠️ **装完就把 pending 清掉** ⇒ ⇒ 那份字节**不会**被又发一遍。
            // ⚠️ **但「再点一次」不再报失败**（v0.58 批）：`pending` 没有了
            // ⇒ ⇒ 判定落到「已装载」那一格 ⇒ ⇒ 如实说「已装载，无需重复
            // 加载」⇒ ⇒ ⛔ **不再**报「装载失败：没有待加载的文件」。
            pending: null,
        });
        publishProductState();
        const streamErrors = [
            result.probeError ? `探针流 ${result.probeError.message}` : null,
            result.consoleError ? `console 流 ${result.consoleError.message}` : null,
            result.memoryMapError ? `内存图流 ${result.memoryMapError.message}` : null,
        ].filter(Boolean);
        setNote(streamErrors.length > 0
            ? `已加载固件「${detail?.params?.name ?? "image.bin"}」，但流订阅不可用：${streamErrors.join("；")}`
            : `已加载固件「${detail?.params?.name ?? "image.bin"}」（${result.byteLength} 字节）—— 点「继续」开始运行`);
    } catch (err) {
        // ⚠️ **失败显示内核原文**（STAGE-32 裁决 J）⇒ ⇒ 不吞。
        //
        // ⚠️ **失败时 pending 保留**（v0.58 批）⇒ ⇒ 用户点一下就能**重试**
        // 同一份文件 ⇒ ⇒ 而不是「再点一次 ⇒ 没有待加载的文件」。
        //
        // ⚠️ **⛔ 这里刻意**不写** `pending: null`：`setImageLoadState` 是
        // **合并**语义 ⇒ ⇒ 不写 ⇒ ⇒ 那份 `pending` **原样留着**。
        setImageLoadState({
            status: "error",
            error: err?.message ?? String(err),
        });
        publishProductState();
        setNote(`加载固件失败：${err?.message ?? err}`);
    }
}

// ---------------------------------------------------------------------------
// 装载**仓外**的镜像（真实 RTOS 的 ELF 不进仓库）
// ---------------------------------------------------------------------------
/**
 * 「按 URL 取镜像字节 → 发 `LoadProject`」的入口。
 *
 * ⚠️ **装载之后必须强制重订订阅**：`LoadProject` 会清掉内核侧的主题集合
 * ⇒ 订阅 `sim.console` 必须在装载**之后**重订，否则 console 一个字节都
 * 收不到（`loadProjectIntoKernel` 把这件事绑在装载流程里，调用点不会漏）。
 *
 * @param {{commandRouter: object, registerMemory: object, waveform: object, debug?: object, console: object, memoryMap?: object}} services
 * @param {string} url
 * @param {object} [options] — 传给 {@link buildImageProject}
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null, byteLength: number, project: object}>}
 */
export async function loadImageFromUrl(services, url, options = {}) {
    const bytes = await fetchImageBytes(url);
    const project = buildImageProject(url, bytes, options);
    const result = await loadProjectIntoKernel(services, project);
    return { ...result, byteLength: bytes.length, project };
}

/**
 * 「按**已在手上**的字节 → 发 `LoadProject`」的入口（STAGE-32 · v0.47 批）。
 *
 * ⚠️ **这是 UI 那条路的落点**：用户在**文件选择器**里选了固件 ⇒ ⇒ 浏览器
 * 把字节读出来 ⇒ ⇒ 视图层把它们放进 `riscv:command` 信号 ⇒ ⇒ app 层
 * （**这里**）把字节变成工程并装载。
 *
 * ⚠️ **与 `loadImageFromUrl` 共用同一条装载路径**（`loadProjectIntoKernel`）
 * ⇒ ⇒ **不写第二套**（规格 §5 的台账边界）⇒ ⇒ 重订流、清残留这些语义
 * **只有一处**。
 *
 * ⚠️ **装完就停**（规格 §3 裁决 D）⇒ ⇒ 这里⛔ **不**发 `ContinueRun`
 * ⇒ ⇒ 跑不跑由用户自己点「继续」。
 *
 * ⚠️ **内核不联网**（`ImageSource::Url` 在宿主里报 `E_LDR-01`）⇒ ⇒ 字节
 * 必须是**真的字节**（不是 base64、不是字符串）⇒ ⇒ 契约层要的形状是
 * `{"Bytes":[...]}`。
 *
 * @param {{commandRouter: object, registerMemory: object, waveform: object, debug?: object, console: object, memoryMap?: object}} services
 * @param {number[]|Uint8Array} bytes — 镜像字节
 * @param {{name?: string, kind?: string, id?: string, projectName?: string, boardName?: string}} [options]
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null, byteLength: number, project: object}>}
 */
export async function loadImageFromBytes(services, bytes, options = {}) {
    const project = buildImageProject(
        options.name ? `/local/${options.name}` : "/local/image.bin",
        bytes,
        {
            imageName: options.name ?? "image.bin",
            kind: options.kind ?? "Elf",
            id: options.id ?? "proj-user-image",
            name: options.projectName ?? options.name ?? "image.bin",
            boardName: options.boardName ?? "MainBoard",
            toolchain: "user file (browser)",
        },
    );
    const result = await loadProjectIntoKernel(services, project);
    return { ...result, byteLength: Array.from(bytes ?? []).length, project };
}

/**
 * 反复发 `Continue`，直到 console 里出现 `needle`（或到点）。
 *
 * ⚠️ **必须一条一条地发**：M-07 的 Worker 是**串行**处理帧的（一条链），
 * 而 M-05 的命令超时只有 500 ms ⇒ 一次并发发上百条的话，最后几条一定
 * 超时（内核照样会跑，但前端会记一串 `E_CTX-06`，且判定不可信）。
 *
 * ⚠️ **判据是 console 里的具体字面量**（如 `"OSZ$ "`），不是「非空」——
 * 「非空」抓不到「跑的是 demo 固件而不是真实 RTOS」这种错。
 *
 * @param {{commandRouter: object}} services
 * @param {string} needle — 期待的 console 片段
 * @param {{timeoutMs?: number, maxBatches?: number, commandTimeoutMs?: number}} [options]
 * @returns {Promise<{matched: boolean, batches: number, text: string}>}
 */
export async function runUntilConsole(services, needle, options = {}) {
    const { commandRouter } = services;
    const timeoutMs = Number(options.timeoutMs ?? 60000);
    const commandTimeoutMs = Number(options.commandTimeoutMs ?? 5000);
    const deadline = Date.now() + timeoutMs;
    let batches = 0;
    let text = "";
    while (Date.now() < deadline) {
        await commandRouter.dispatchCommand(
            Command.continueRun(),
            { timeoutMs: commandTimeoutMs },
        );
        batches += 1;
        text = viewStore.getState("terminal")?.term?.bytes ?? "";
        if (needle && text.includes(needle)) {
            return { matched: true, batches, text };
        }
        if (options.maxBatches && batches >= Number(options.maxBatches)) break;
    }
    return { matched: Boolean(needle) && text.includes(needle), batches, text };
}

/**
 * 把「装载仓外镜像 + 跑到某个 console 前缀」暴露给页面上的脚本。
 *
 * ⚠️ 这条链**没有 UI 入口**：真实 RTOS 的镜像**在仓外**（不进仓库），
 * 浏览器要拿到它就得**自己**把字节取回来——只能由脚本
 * （`tools/browser-verify.mjs`）驱动。
 *
 * @param {object} services
 */
export function exposeImageLoader(services) {
    window.riscv_image = {
        loadFromUrl: (url, options) => loadImageFromUrl(services, url, options),
        runUntilConsole: (needle, options) => runUntilConsole(services, needle, options),
    };
}
