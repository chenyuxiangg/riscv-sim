// 顶部渐隐 toast 的前端门禁：
//
// ① 单元：弹出 → 显示中 → 停留结束渐隐 → 自动关闭；文案写进唯一落点；
// ② 接入：单字读内存 / 整段转储被内核拒绝时，顶部 toast 报出错误码与原文，
//    既有错误位 / 页脚 note 行为不变（错误照旧向上抛）。
//
// ⚠️ 状态断言走 `data-toast-state` 属性（与 alert.js 同约定），不依赖
//    `style` / `hidden` ⇒ ⇒ Node 侧零浏览器可验。

import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const toast = await importFront("lib/toast.js");
const m02 = await importFront("views/m02_viewcomponents.js");

await m02.mountViewComponents();

const tick = (ms) => new Promise((r) => setTimeout(r, ms));

/** 读失败的内存服务替身（带错误码）。 */
function failingRegmem(code, message) {
    return {
        async readMemory() {
            const err = new Error(`${code}: ${message}`);
            err.code = code;
            throw err;
        },
    };
}

/** 读失败的命令路由替身（带错误码）。 */
function failingRouter(code, message) {
    return {
        async dispatchCommand() {
            const err = new Error(`${code}: ${message}`);
            err.code = code;
            throw err;
        },
    };
}

describe("顶部渐隐 toast · 单元", () => {
    beforeEach(() => toast.hideToast());

    test("弹出后状态为 open，文案写进唯一落点", () => {
        const state = toast.showToast("E_BUS-01：read unmapped addr=0x10000008");
        assert.equal(state, "open");
        assert.equal(toast.toastState(), "open");
        const text = globalThis.document.querySelector("[data-toast-text]");
        assert.ok(text, "toast 文案落点不存在");
        assert.match(text.textContent, /E_BUS-01/);
        assert.match(text.textContent, /0x10000008/);
    });

    test("停留结束 → fading → 自动关闭", async () => {
        toast.showToast("越限测试", { holdMs: 1, fadeMs: 1 });
        assert.equal(toast.toastState(), "open");
        await tick(30);
        assert.equal(toast.toastState(), "", "渐隐结束后必须彻底关闭");
    });

    test("反复弹出重置计时，不被上一次提前关掉", () => {
        toast.showToast("第一次", { holdMs: 1, fadeMs: 1 });
        toast.showToast("第二次", { holdMs: 5000, fadeMs: 300 });
        assert.equal(toast.toastState(), "open");
        const text = globalThis.document.querySelector("[data-toast-text]");
        assert.equal(text.textContent, "第二次");
        toast.hideToast();
    });
});

describe("顶部渐隐 toast · 内存读失败接入", () => {
    beforeEach(() => toast.hideToast());

    test("单字读内存被拒 ⇒ ⇒ toast 报错误码与原文（外层 catch 照旧写页脚）", async () => {
        const services = {
            commandRouter: failingRouter("E_BUS-01", "read unmapped addr=0x10000008"),
            registerMemory: failingRegmem("E_BUS-01", "read unmapped addr=0x10000008"),
        };
        await app.handleCommand(
            { action: "read-mem", params: { addr: 0x10000008, size: "Byte" } },
            services,
        );
        assert.equal(toast.toastState(), "open", "读失败必须弹顶部 toast");
        const text = globalThis.document.querySelector("[data-toast-text]").textContent;
        assert.match(text, /E_BUS-01/);
        assert.match(text, /0x10000008/);
    });

    test("整段转储越限 ⇒ ⇒ toast 报错误码与原文（外层 catch 照旧写页脚）", async () => {
        const services = {
            commandRouter: failingRouter("E_DBG-03", "dump range exceeds window"),
            registerMemory: failingRegmem("E_DBG-03", "dump range exceeds window"),
        };
        await app.handleCommand(
            { action: "dump-memory", params: { addr: 0x10000000, len: 256 } },
            services,
        );
        assert.equal(toast.toastState(), "open", "转储越限必须弹顶部 toast");
        const text = globalThis.document.querySelector("[data-toast-text]").textContent;
        assert.match(text, /E_DBG-03/);
        assert.match(text, /dump range exceeds window/);
    });
});
