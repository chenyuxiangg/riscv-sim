// v0.24 · Lane K —— 「可编程的源码树入口」的前端门禁（K-2）。
//
// 这个套件盯三件事：
//
//   1. `window.riscv_source.setTree(treeMap)` 这个**可编程**入口存在
//      （真机脚本没法操作文件选择器 ⇒ 需要一条可编程的路）；
//   2. 调它之后，源码树**进了 M-03 的视图状态**（源码视图的摘要行能
//      报出文件数）；
//   3. ⚠️ **内核侧一个字都没收到**——源码**不进内核、不进契约**
//      （STAGE-7 裁决 E 的老规矩：内核不联网、不碰文件系统）。
//
// ⚠️ 第 3 条是这条入口的**核心不变式**：源码树一旦漏进内核，就等于把
// 用户整棵源码树塞进仿真内核（还会让它进快照、进波形、进而进契约）。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（与 `console.test.mjs` 同一条理由）。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const { Event: EventPayload, eventMessage } = await importFront("lib/message.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** 一棵两文件的源码树（`Map` 形态 —— 任务书指定的形态）。 */
const TREE = new Map([
    ["src/main.c", "int main(void) { return 0; }\n"],
    ["src/util.h", "#pragma once\n"],
]);

/** 源码视图摘要行应当报出的**文件数**。 */
const TREE_FILE_COUNT = 2;

// ---------------------------------------------------------------------------

/**
 * 装一套干净的接线（视图状态是**模块级单例**，用例之间共享 ⇒ 每次都重置）。
 *
 * @returns {Promise<{router: object, transport: object}>}
 */
async function setup() {
    m03.reset();
    /** @type {Set<Function>} */
    const handlers = new Set();
    const transport = {
        /** @type {Array<{message: object, reqId: number|null}>} */
        sent: [],
        onMessage(handler) {
            handlers.add(handler);
            return () => handlers.delete(handler);
        },
        onStatus() { return () => {}; },
        getTransportStatus() { return { mode: "worker", connected: true }; },
        sendCommand(message, options = {}) {
            const reqId = options.reqId ?? null;
            transport.sent.push({ message, reqId });
            const frame = {
                type: "event",
                seq: message.seq,
                sessionId: "sess-test",
                role: "embedded",
                reqId,
                payload: eventMessage(message.seq, EventPayload.tick(0, 0)),
            };
            for (const fn of [...handlers]) fn(frame);
        },
    };
    const router = await m05.mountCommandRouter({ transport });
    m01.assembleWorkspace("embedded");
    m01.route("#/source");
    // 装上**可编程**入口（`bootstrap()` 装的是同一个函数）。
    app.exposeSourceLoader();
    return { router, transport };
}

// ---------------------------------------------------------------------------

describe("可编程的源码树入口（K-2）", () => {
    test("`window.riscv_source.setTree` 存在（可编程入口）", async () => {
        await setup();
        assert.equal(
            typeof dom.window.riscv_source?.setTree,
            "function",
            "可编程的源码树入口不存在",
        );
    });

    test("调它之后，源码树进了 M-03 的视图状态（摘要行报出文件数）", async () => {
        await setup();
        dom.window.riscv_source.setTree(TREE);

        const tree = m03.getState("source")?.source?.tree ?? {};
        assert.equal(
            Object.keys(tree).length,
            TREE_FILE_COUNT,
            "源码树没有进视图状态（或文件数不对）",
        );
        assert.equal(tree["src/main.c"], "int main(void) { return 0; }\n");
        assert.equal(tree["src/util.h"], "#pragma once\n");

        // 源码视图的摘要行如实报出文件数。
        const summary = dom.document.querySelector("[data-source-summary]");
        assert.ok(summary, "源码视图没有渲染摘要行");
        assert.ok(
            summary.textContent.includes(`源码树：${TREE_FILE_COUNT} 个文件`),
            `摘要行没有报出 ${TREE_FILE_COUNT} 个文件：${summary.textContent}`,
        );
    });

    test("⚠️ **内核侧一个字都没收到**（源码不进内核、不进契约）", async () => {
        const { transport } = await setup();
        dom.window.riscv_source.setTree(TREE);

        assert.equal(
            transport.sent.length,
            0,
            `装源码树不该发任何命令，却发了 ${JSON.stringify(transport.sent)}`,
        );
        // 再加一道：整条链路上**任何**帧里都不许出现源码内容。
        const wire = JSON.stringify(transport.sent);
        assert.ok(
            !wire.includes("int main(void)"),
            "源码内容漏进了发给内核的东西里",
        );
        assert.ok(
            !wire.includes("src/main.c"),
            "源码路径漏进了发给内核的东西里",
        );
    });

    test("普通对象与 `[key, value]` 数组也能喂（调用方不必改数据结构）", async () => {
        await setup();
        dom.window.riscv_source.setTree({ "a/b.c": "AAA" });
        dom.window.riscv_source.setTree([["d/e.f", "EEE"]]);
        const tree = m03.getState("source")?.source?.tree ?? {};
        assert.equal(tree["a/b.c"], "AAA");
        assert.equal(tree["d/e.f"], "EEE");
    });

    test("并入已有的树，不丢用户已经装好的文件", async () => {
        await setup();
        dom.window.riscv_source.setTree(TREE);
        dom.window.riscv_source.setTree(new Map([["src/extra.c", "XXX"]]));
        const tree = m03.getState("source")?.source?.tree ?? {};
        assert.equal(Object.keys(tree).length, TREE_FILE_COUNT + 1);
        assert.equal(tree["src/main.c"], "int main(void) { return 0; }\n");
        assert.equal(tree["src/extra.c"], "XXX");
    });
});
