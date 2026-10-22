// M-11 调试面（契约 v3）的前端门禁：反汇编面板 + trace 导出入口。
//
// 这个套件盯住 STAGE-5 §5.2 的验收 #19 / #20，以及 §2 裁决 C / H：
//
//   1. mock 内核对四条新命令都有**形状正确**的回复（否则前端根本测不了）；
//   2. `Disasm` 事件的条目进得了面板：地址 / 十六进制机器码 / 助记符 / 宽度；
//   3. **PC 所在的那一行**带高亮标记（V-12）——PC 只能来自 M-10 推进的
//      `sim.pc`，前端不许自己造状态源；
//   4. 压缩指令**原样显示**（`c.addi sp,-16` 不许被改写成 `addi sp,sp,-16`，
//      裁决 C：伪指令别名是汇编器语义，平台不产生它）；
//   5. 导出后展示 `chunk`；`next_offset < total` 时有「继续导出」按钮，
//      点它发出的命令的 `offset` **等于**上一次的 `next_offset`；
//   6. `TraceState.enabled === true` ⇒ 显示降速提示（评审 R9）；`false` ⇒
//      提示消失。前端**不**自己发明「检测到性能下降」这类判断；
//   7. 内核回 `Error`（`E_CTX-03`）⇒ 前端显示该错误（走 M-05 的错误路径，
//      不另造一套错误展示）。
//
// ⚠️ **断言必须同步**：这里的传输是**进程内**的——mock 内核在
// `sendCommand` 里**同步**回帧，于是「事件 → store → 重渲染」全都在
// 同一个 tick 内完成。断言与事件处理之间没有 `await`，中间不可能插入
// 别的动作。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（D-46 检视 V-B · probe D2）：`app.js` 是
// 引导入口——**导入即启动**，它的 `bootstrap()` 是异步的，会在任意两个
// 用例之间动 M-03 的模块级单例。这里显式关掉，让套件**不依赖时序运气**。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const m10 = await importFront("services/m10_registermemory.js");
const { Command, Event: EventPayload, eventMessage, TRACE_FORMAT, TRACE_SOURCE } =
    await importFront("lib/message.js");
const { createMockKernel } = await importFront("dev/mock-kernel.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

/**
 * An **in-process** transport: the mock kernel answers synchronously, so
 * the whole chain (M-05 → mock kernel → M-05 → 视图) runs inside one
 * tick.
 *
 * ⚠️ This is **not** the M-07 worker transport — the worker path is
 * covered by `mock-kernel.test.mjs`. What this buys the debug panel is
 * **determinism**: the reply is delivered before `dispatchCommand`
 * returns, so the assertions never have to wait for a timer.
 *
 * @param {object} kernel — the mock kernel
 */
function inProcessTransport(kernel) {
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
            const frames = kernel.handle({
                type: "command",
                seq: message.seq,
                sessionId: "sess-test",
                role: "embedded",
                reqId,
                payload: message,
            });
            for (const frame of frames) {
                for (const fn of [...handlers]) fn(frame);
            }
        },
        /** Deliver a pushed event (no request id) to the router. */
        emit(payload) {
            const frame = {
                type: "event",
                seq: 1,
                sessionId: "sess-test",
                role: "embedded",
                reqId: null,
                payload: eventMessage(1, payload),
            };
            for (const fn of [...handlers]) fn(frame);
        },
    };
    return transport;
}

/**
 * Build a clean wiring: mock kernel → transport → M-05 → 调试面接线，
 * plus the mounted + rendered debug view.
 *
 * ⚠️ 视图状态是**模块级单例**（M-03），用例之间共享 ⇒ 每个用例都要
 * 先 `reset()`。
 *
 * @returns {Promise<{kernel: object, transport: object, router: object, debug: object}>}
 */
async function setup() {
    m03.reset();
    const kernel = createMockKernel();
    const transport = inProcessTransport(kernel);
    const router = await m05.mountCommandRouter({ transport });
    const debug = app.wireDebug(router);
    // 面板要先挂载 + 渲染，DOM 里才有 `[data-view-body="debug"]`。
    m01.assembleWorkspace("embedded");
    m01.route("#/debug");
    return { kernel, transport, router, debug };
}

/** The debug view's body element. */
function debugBody() {
    const body = dom.document.querySelector('[data-view-body="debug"]');
    assert.ok(body, "调试视图体未挂载");
    return body;
}

/**
 * Click a button and let the **production signal path** run: the view
 * emits `riscv:command`, the listener turns it into an M-05 command.
 *
 * @param {object} router
 * @param {object} button
 */
function clickAndDispatch(router, button) {
    /** @type {object[]} */
    const signals = [];
    const listener = (ev) => {
        signals.push(ev.detail);
        void app.handleCommand(ev.detail, { commandRouter: router });
    };
    dom.window.addEventListener("riscv:command", listener);
    button.dispatchEvent(new Event("click"));
    dom.window.removeEventListener("riscv:command", listener);
    return signals;
}

// ---------------------------------------------------------------------------

describe("mock 内核 · M-11 的四条新命令", () => {
    test("Disasm 返回形状正确的条目（含压缩指令与两种宽度）", async () => {
        const { router } = await setup();
        const event = await router.dispatchCommand(Command.disasm(0, 8));
        assert.equal(event.variant, "Disasm");
        const entries = event.value.entries;
        assert.equal(entries.length, 6, "条目数不对");
        for (const entry of entries) {
            assert.deepEqual(
                Object.keys(entry).sort(),
                ["addr", "code", "mnemonic", "width"],
                "条目字段必须与 DisasmEntryDto 一致",
            );
        }
        for (let i = 1; i < entries.length; i += 1) {
            assert.ok(entries[i].addr > entries[i - 1].addr, "地址必须递增");
        }
        assert.ok(entries.some((e) => e.width === 2), "缺少 2 字节（压缩）指令");
        assert.ok(entries.some((e) => e.width === 4), "缺少 4 字节指令");
        // 裁决 C：压缩指令保留 `c.` 前缀。
        assert.ok(
            entries.some((e) => e.mnemonic.startsWith("c.")),
            "压缩指令没有保留 c. 前缀",
        );
        router.close();
    });

    test("Disasm 的序列可以短于请求条数（截断，不是错误）", async () => {
        const { router } = await setup();
        const event = await router.dispatchCommand(Command.disasm(0, 99));
        assert.ok(event.value.entries.length < 99, "序列不该被凭空补齐");
        router.close();
    });

    test("ResolveLine 返回源码位置", async () => {
        const { router } = await setup();
        const event = await router.dispatchCommand(Command.resolveLine(0x10));
        assert.equal(event.variant, "SourceLocation");
        assert.equal(event.value.addr, 0x10);
        assert.equal(event.value.file, "demo.c");
        assert.equal(event.value.line, 7);
        router.close();
    });

    test("ExportTrace 的 total / next_offset 自洽，且可续导", async () => {
        const { router } = await setup();
        const first = await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, 0, 0),
        );
        assert.equal(first.variant, "TraceExport");
        const v = first.value;
        assert.ok(v.chunk.length > 0, "chunk 不能为空");
        assert.ok(v.next_offset > v.offset, "还有条目没给完");
        assert.ok(v.next_offset <= v.total, "next_offset 不能越过 total");
        assert.equal(v.bytes, v.chunk.length);

        // 续导：把 `next_offset` 当作下次的 `offset`。
        const second = await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, v.next_offset, 0),
        );
        assert.equal(second.value.offset, v.next_offset);
        assert.ok(second.value.next_offset <= second.value.total);
        router.close();
    });

    test("非法的 source/format 组合被内核拒绝（E_CTX-03）", async () => {
        const { router } = await setup();
        await assert.rejects(
            () => router.dispatchCommand(
                Command.exportTrace(TRACE_SOURCE.Waveform, TRACE_FORMAT.Json, 0, 0),
            ),
            /E_CTX-03/,
            "波形 + JSON 必须被拒绝（M-19 没有 JSON 波形导出）",
        );
        await assert.rejects(
            () => router.dispatchCommand(
                Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Vcd, 0, 0),
            ),
            /E_CTX-03/,
            "执行轨迹 + VCD 必须被拒绝（轨迹不是信号波形）",
        );
        router.close();
    });

    test("SetTraceEnabled 回显 enabled（幂等）", async () => {
        const { router } = await setup();
        const on = await router.dispatchCommand(Command.setTraceEnabled(true));
        assert.equal(on.variant, "TraceState");
        assert.equal(on.value.enabled, true);
        const again = await router.dispatchCommand(Command.setTraceEnabled(true));
        assert.equal(again.value.enabled, true);
        const off = await router.dispatchCommand(Command.setTraceEnabled(false));
        assert.equal(off.value.enabled, false);
        router.close();
    });
});

// ---------------------------------------------------------------------------

describe("反汇编面板 (M-11)", () => {
    test("Disasm 事件的条目进得了面板（地址 / 十六进制机器码 / 助记符 / 宽度）", async () => {
        const { router } = await setup();
        await router.dispatchCommand(Command.disasm(0, 8));

        const body = debugBody();
        const rows = body.querySelectorAll("tr.disasm-row");
        assert.equal(rows.length, 6, "面板没有渲染全部条目");

        const first = body.querySelector('[data-disasm-addr="0x00000000"]');
        assert.ok(first, "第一条指令没有渲染");
        assert.ok(first.textContent.includes("0x00000000"), "地址没显示");
        assert.ok(first.textContent.includes("addi x1, x0, 42"), "助记符没显示");
        assert.ok(first.textContent.includes("0x02a00093"), "机器码不是十六进制形态");
        assert.ok(first.textContent.includes("4"), "宽度没显示");
        router.close();
    });

    test("机器码按指令宽度对齐（16 位 → 4 位十六进制）", async () => {
        const { router } = await setup();
        await router.dispatchCommand(Command.disasm(0, 8));
        const body = debugBody();
        const compressed = body.querySelector('[data-disasm-addr="0x0000000a"]');
        assert.ok(compressed, "压缩指令没渲染");
        assert.ok(
            compressed.textContent.includes("0x2a00"),
            `16 位机器码应当是 4 位十六进制：${compressed.textContent}`,
        );
        router.close();
    });

    test("PC 所在的那一行高亮（V-12），别的行不高亮", async () => {
        const { router, transport } = await setup();
        const service = await m10.mountRegisterMemory({
            commandRouter: router,
            transport,
            viewStore: m03,
        });
        await router.dispatchCommand(Command.disasm(0, 8));
        // PC 只能来自 M-10 推进的 `sim.pc`（`sim.registers` / `sim.state.delta`）。
        transport.emit(EventPayload.simStateDelta([], 0x0000_0004, 0));
        service.flushRender();

        const body = debugBody();
        const pcRow = body.querySelector('[data-disasm-addr="0x00000004"]');
        assert.ok(pcRow, "PC 行没渲染");
        assert.equal(pcRow.classList.contains("is-pc"), true, "PC 行没有高亮标记");
        assert.equal(pcRow.getAttribute("data-pc"), "1", "PC 行没有 data-pc 标记");

        const other = body.querySelector('[data-disasm-addr="0x00000000"]');
        assert.ok(other, "非 PC 行没渲染");
        assert.equal(other.classList.contains("is-pc"), false, "非 PC 行被高亮了");
        router.close();
        service.close();
    });

    test("压缩指令原样显示（不被改写成伪指令别名）", async () => {
        const { router } = await setup();
        await router.dispatchCommand(Command.disasm(0, 8));
        const body = debugBody();
        const row = body.querySelector('[data-disasm-addr="0x0000000a"]');
        assert.ok(row, "压缩指令没渲染");
        const mnemonic = row.querySelector(".disasm-mnemonic");
        assert.ok(mnemonic, "助记符单元没渲染");
        assert.equal(mnemonic.textContent, "c.addi sp,-16");
        assert.equal(
            body.textContent.includes("addi sp,sp,-16"),
            false,
            "助记符被改写成了伪指令别名（裁决 C）",
        );
        router.close();
    });

    test("装载新工程后，旧程序的反汇编被清掉（D-45 的坑）", async () => {
        const { router, debug } = await setup();
        await router.dispatchCommand(Command.disasm(0, 8));
        assert.ok(
            m03.getState("debug").sim.disasm.length > 0,
            "先得有一条反汇编",
        );
        await app.loadProjectIntoKernel(
            {
                commandRouter: router,
                registerMemory: { async loadProject() {} },
                waveform: { reset() {}, async subscribeProbeStream() {} },
                debug,
            },
            { id: "p" },
        );
        assert.deepEqual(
            m03.getState("debug").sim.disasm,
            [],
            "旧程序的反汇编还挂在面板上",
        );
        router.close();
    });

    test("「反汇编」按钮发出 Disasm 命令（地址取当前 PC）", async () => {        const { router, transport } = await setup();
        const service = await m10.mountRegisterMemory({
            commandRouter: router,
            transport,
            viewStore: m03,
        });
        transport.emit(EventPayload.simStateDelta([], 0x0000_0004, 0));
        service.flushRender();

        const button = debugBody().querySelector('[data-action="disasm"]');
        assert.ok(button, "没有「反汇编」按钮");
        const signals = clickAndDispatch(router, button);
        assert.equal(signals.length, 1, "点击没有发出 riscv:command 信号");
        assert.equal(signals[0].action, "disasm");

        assert.equal(transport.sent.length, 1, "命令没有发到 M-05");
        const payload = transport.sent[0].message.payload.Command;
        assert.deepEqual(payload, { Disasm: { addr: 0x0000_0004, count: 16 } });
        router.close();
        service.close();
    });
});

// ---------------------------------------------------------------------------

describe("trace 导出入口", () => {
    test("导出后展示 chunk 的内容", async () => {
        const { router } = await setup();
        await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, 0, 0),
        );
        const chunk = dom.document.querySelector("[data-trace-chunk]");
        assert.ok(chunk, "没有导出内容区");
        assert.ok(chunk.textContent.includes("kind,addr,value"), "chunk 内容没显示");
        assert.ok(chunk.textContent.includes("instr,00000000,02a00093"), "chunk 内容不完整");
        router.close();
    });

    /**
     * The exported window is **not the complete history** when the
     * source ring overflowed. The front end has to say so — presenting
     * a truncated export as if it were the whole session is exactly the
     * "the data exists but nobody is told it is incomplete" failure the
     * D-50 / D-51 ledger entries are about.
     *
     * ⚠️ The state is injected **directly** into the store: the mock
     * kernel never overflows its ring, so there is no way to reach this
     * state through the command path. What is under test here is the
     * **view's** job (render the flag), not the mock's.
     */
    test("导出窗口不完整时给出提示（overflow_marked）", async () => {
        const { router } = await setup();
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                export: {
                    source: TRACE_SOURCE.Execution,
                    format: TRACE_FORMAT.Csv,
                    offset: 0,
                    next_offset: 4,
                    total: 4,
                    bytes: 2,
                    chunk: "a,b",
                    overflow_marked: true,
                },
            },
        }));
        // ⚠️ M-02 的渲染事件**必须带 `state`**（`applyRenderEvent` 读的是
        // `detail.state`，不是去 store 里读）。
        window.dispatchEvent(new CustomEvent("riscv:render", {
            detail: { viewId: "debug", state: m03.getState("debug") },
        }));

        const notice = dom.document.querySelector("[data-trace-overflow]");
        assert.ok(notice, "没有「导出窗口不完整」的提示");
        assert.ok(
            notice.textContent.includes("不是完整历史"),
            `提示没有说清窗口不完整：${notice.textContent}`,
        );
        router.close();
    });

    test("导出窗口完整时**不**给出「不完整」提示", async () => {
        const { router } = await setup();
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                export: {
                    source: TRACE_SOURCE.Execution,
                    format: TRACE_FORMAT.Csv,
                    offset: 0,
                    next_offset: 4,
                    total: 4,
                    bytes: 2,
                    chunk: "a,b",
                    overflow_marked: false,
                },
            },
        }));
        window.dispatchEvent(new CustomEvent("riscv:render", {
            detail: { viewId: "debug", state: m03.getState("debug") },
        }));

        assert.equal(
            dom.document.querySelector("[data-trace-overflow]"),
            null,
            "窗口完整却显示了「不完整」提示",
        );
        router.close();
    });

    test("next_offset < total 时有「继续导出」按钮", async () => {
        const { router } = await setup();
        await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, 0, 0),
        );
        const button = dom.document.querySelector('[data-action="export-trace"][data-continue="1"]');
        assert.ok(button, "没有「继续导出」按钮");
        const nextOffset = Number(button.dataset.offset);
        const total = Number(
            dom.document.querySelector("[data-trace-progress]").textContent.match(/\/\s*(\d+)/)[1],
        );
        assert.ok(nextOffset > 0 && nextOffset < total, "续导偏移不自洽");
        router.close();
    });

    test("点「继续导出」发出的命令的 offset 等于上一次的 next_offset", async () => {
        const { router, transport } = await setup();
        const first = await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, 0, 0),
        );
        const nextOffset = first.value.next_offset;
        assert.ok(nextOffset < first.value.total, "这个用例需要一个可续导的回复");

        const button = dom.document.querySelector('[data-action="export-trace"][data-continue="1"]');
        assert.ok(button, "没有「继续导出」按钮");
        clickAndDispatch(router, button);

        assert.equal(transport.sent.length, 2, "续导命令没有发出去");
        const payload = transport.sent[1].message.payload.Command;
        assert.equal(payload.ExportTrace.offset, nextOffset, "续导的 offset 不是上一次的 next_offset");
        assert.equal(payload.ExportTrace.source, TRACE_SOURCE.Execution);
        assert.equal(payload.ExportTrace.format, TRACE_FORMAT.Csv);
        router.close();
    });

    test("导出到末尾后不再有「继续导出」按钮", async () => {
        const { router } = await setup();
        await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, 0, 0),
        );
        const button = dom.document.querySelector('[data-action="export-trace"][data-continue="1"]');
        assert.ok(button);
        const nextOffset = Number(button.dataset.offset);
        await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, nextOffset, 0),
        );
        assert.equal(
            dom.document.querySelector('[data-action="export-trace"][data-continue="1"]'),
            null,
            "已经导完了还提示可以继续",
        );
        router.close();
    });
});

// ---------------------------------------------------------------------------

describe("trace 开关 · 降速提示与错误路径", () => {
    test("TraceState.enabled=true 显示降速提示，false 时提示消失", async () => {
        const { router } = await setup();
        assert.equal(
            dom.document.querySelector("[data-trace-notice]"),
            null,
            "初始状态不该有降速提示",
        );
        await router.dispatchCommand(Command.setTraceEnabled(true));
        const notice = dom.document.querySelector("[data-trace-notice]");
        assert.ok(notice, "开启 trace 后没有降速提示");
        assert.ok(notice.textContent.includes("降速"), "提示没有说明会降速");

        await router.dispatchCommand(Command.setTraceEnabled(false));
        assert.equal(
            dom.document.querySelector("[data-trace-notice]"),
            null,
            "关闭 trace 后降速提示还在",
        );
        router.close();
    });

    test("内核的 Error 回复（E_CTX-03）被显示出来", async () => {
        const { router } = await setup();
        await assert.rejects(
            () => router.dispatchCommand(
                Command.exportTrace(TRACE_SOURCE.Waveform, TRACE_FORMAT.Json, 0, 0),
            ),
            /E_CTX-03/,
        );
        const shown = dom.document.querySelector("[data-debug-error]");
        assert.ok(shown, "内核的错误没有被显示");
        assert.ok(shown.textContent.includes("E_CTX-03"), "错误码没有显示");
        router.close();
    });
});
