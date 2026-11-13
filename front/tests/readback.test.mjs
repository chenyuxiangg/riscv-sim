// 「**读回**」（read-back）的前端门禁（v0.67 批 · R-1 ~ R-16）。
//
// ⚠️ **这一批修的是用户报的两个现象**：
//
//   ① 寄存器窗口写着「**变化高亮 = 与上一次读回相比**」，但用户**不知道什么时候
//      会高亮**（规则不可预期、时机不透明）；
//   ② 填好地址「读内存」之后，**运行再暂停**，内存单元表**不联动刷新**，仍停在
//      运行前的旧值。
//
// ⇒ ⇒ 两条用例线：**R-1 ~ R-9** 钉**纯函数**（`front/lib/readback.js` · 零 DOM）；
//   **R-10 ~ R-16** 钉**app 层编排**（基准什么时候抓、什么时候读回、读回什么）。
//
// ⚠️ **判别力清单**（每一条都要能**变红**）：
//   - R-7 钉「`EndOfExecution` **没有**暂停在等 ⇒ ⇒ **不**读回」⇒ ⇒ 删掉那个
//     前提，这条**必红**（否则「永远读回」的实现也会绿 ⇒ ⇒ 空转）；
//   - R-13 钉「真停 ⇒ ⇒ 读回三样」⇒ ⇒ 删掉任何一样，这条**必红**；
//   - R-14 钉「**宽度**必须按原来那份重读」⇒ ⇒ 宽度写死成 `Word` 这条**必红**；
//   - R-15 钉「连续运行的窗口结束**不**触发读回」⇒ ⇒ 触发了就**必红**；
//   - R-16 钉「用户暂停后的那一次停下**要**触发读回」⇒ ⇒ 不触发就**必红**。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（与其它套件同一条规矩）⇒ ⇒ 套件不依赖时序运气。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const rb = await importFront("lib/readback.js");
const m03 = await importFront("views/m03_viewstore.js");

/** 让微任务与定时器都跑完（读回是异步的）。 */
async function settle(times = 4) {
    for (let i = 0; i < times; i += 1) {
        await new Promise((r) => setTimeout(r, 0));
    }
}

/**
 * 一个**记调用**的假 M-10（寄存器 / 内存那一份）。
 *
 * @param {{gpr?: number[], memoryReads?: {addr:number,size:string}[]}} [init]
 */
function fakeRegisterMemory(init = {}) {
    const calls = { readRegisters: 0, readMemory: [], dumpMemory: [], flushRender: 0 };
    const state = {
        gpr: Array.isArray(init.gpr) ? init.gpr.slice() : Array(32).fill(0),
        memoryReads: Array.isArray(init.memoryReads) ? init.memoryReads.map((r) => ({ ...r })) : [],
    };
    return {
        calls,
        state,
        getSnapshot: () => ({
            gpr: state.gpr.slice(),
            memoryReads: state.memoryReads.map((r) => ({ ...r })),
        }),
        readRegisters: async () => {
            calls.readRegisters += 1;
        },
        readMemory: async (addr, size) => {
            calls.readMemory.push({ addr, size });
            return { addr, size, value: 0 };
        },
        flushRender: () => {
            calls.flushRender += 1;
        },
    };
}

/** 一个**能手动触发停下报告**的假 commandRouter。 */
function fakeRouter() {
    const handlers = new Map();
    const dispatched = [];
    return {
        handlers,
        dispatched,
        subscribe: (topic, fn) => {
            handlers.set(topic, fn);
            return () => handlers.delete(topic);
        },
        dispatchCommand: async (payload) => {
            dispatched.push(payload);
            return { value: null };
        },
    };
}

// ---------------------------------------------------------------------------

describe("v0.67 · 「读回」的纯函数", () => {
    test("R-1 · captureBaseline 拷一份（改源不改基准）", () => {
        const live = [1, 2, 3];
        const baseline = rb.captureBaseline(live);
        live[0] = 99;
        assert.deepEqual(baseline, [1, 2, 3], "基准引用了活值 ⇒ ⇒ 跑完之后基准会跟着变");
    });

    test("R-2 · captureBaseline 对非法输入给 null / 归零", () => {
        assert.equal(rb.captureBaseline(null), null);
        assert.equal(rb.captureBaseline(undefined), null);
        assert.equal(rb.captureBaseline("不是数组"), null);
        assert.deepEqual(rb.captureBaseline([1, "2", null]), [1, 2, 0]);
    });

    test("R-3 · diffRegisters 给出变了的下标（没变的**不在**里面）", () => {
        assert.deepEqual(rb.diffRegisters([1, 2, 3], [1, 2, 3]), []);
        assert.deepEqual(rb.diffRegisters([1, 2, 3], [1, 9, 3]), [1]);
        assert.deepEqual(rb.diffRegisters([1, 2, 3], [0, 2, 3]), [0]);
    });

    test("R-4 · diffRegisters 对没有基准给空数组（不编造）", () => {
        assert.deepEqual(rb.diffRegisters(null, [1, 2]), []);
        assert.deepEqual(rb.diffRegisters([1, 2], null), []);
    });

    test("R-5 · diffRegisters 长度不齐也照样比（缺的那位按 0）", () => {
        // ⚠️ 基准比当前值**短** ⇒ ⇒ 缺的那位**按 0**参与 ⇒ ⇒ 高一位的变化
        //    **看得见**（不是「跳过」）。
        assert.deepEqual(rb.diffRegisters([1], [1, 5]), [1]);
        assert.deepEqual(rb.diffRegisters([1, 5], [1]), [1]);
    });

    test("R-6 · readbackReasonOf 认得四种真停原因", () => {
        assert.equal(rb.readbackReasonOf("Breakpoint"), rb.READBACK_REASON.BREAKPOINT);
        assert.equal(rb.readbackReasonOf("StepOver"), rb.READBACK_REASON.STEP);
        assert.equal(rb.readbackReasonOf("Watchpoint"), rb.READBACK_REASON.WATCHPOINT);
        assert.equal(rb.readbackReasonOf("Trap"), rb.READBACK_REASON.TRAP);
        assert.equal(rb.readbackReasonOf("UserPause"), rb.READBACK_REASON.USER_PAUSE);
    });

    test("R-7 · readbackReasonOf 对不认识的原因给 null（**不猜**）", () => {
        assert.equal(rb.readbackReasonOf("EndOfExecution"), null);
        assert.equal(rb.readbackReasonOf("SomethingElse"), null);
        assert.equal(rb.readbackReasonOf(null), null);
        assert.equal(rb.readbackReasonOf(undefined), null);
    });

    test("R-8 · shouldAutoReadback：四种真停 ⇒ ⇒ 读回", () => {
        for (const reason of ["Breakpoint", "Watchpoint", "Trap", "UserPause", "StepOver"]) {
            assert.equal(
                rb.shouldAutoReadback({ haltReason: reason }),
                true,
                `${reason} 是真停 ⇒ ⇒ 停下之后该读回`,
            );
        }
    });

    test("R-9 · shouldAutoReadback：`EndOfExecution` **没有**暂停在等 ⇒ ⇒ **不**读回（防空转）", () => {
        // ⚠️ **这一条最容易写成空转** —— 连续运行里**每个窗口**结束都会来一次
        //   `EndOfExecution` ⇒ ⇒ 若这里也返回 true ⇒ ⇒ 真机门禁的脚本口每个
        //   批次都多发两条命令 ⇒ ⇒ **慢到不可用** ⇒ ⇒ 必须是 false。
        assert.equal(rb.shouldAutoReadback({ haltReason: "EndOfExecution" }), false);
        assert.equal(
            rb.shouldAutoReadback({ haltReason: "EndOfExecution", pausePending: false }),
            false,
        );
        // ⚠️ **有**暂停在等 ⇒ ⇒ 那一次停下**就是**用户要的那次停 ⇒ ⇒ 该读回。
        assert.equal(
            rb.shouldAutoReadback({ haltReason: "EndOfExecution", pausePending: true }),
            true,
        );
        // ⚠️ 不认识的原因 ⇒ ⇒ **不猜** ⇒ ⇒ 不读回。
        assert.equal(rb.shouldAutoReadback({ haltReason: "Whatever" }), false);
        assert.equal(rb.shouldAutoReadback({ haltReason: null }), false);
        assert.equal(rb.shouldAutoReadback({}), false);
    });
});

// ---------------------------------------------------------------------------

describe("v0.67 · 「读回」的 app 层编排", () => {
    test("R-10 · 「运行」开始前抓基准（住 `debug.readback.baseline`）", async () => {
        const live = Array.from({ length: 32 }, (_, i) => i + 1);
        const memory = fakeRegisterMemory({ gpr: live });
        const router = fakeRouter();
        await app.handleCommand(
            { action: "continue" },
            { commandRouter: router, registerMemory: memory },
        );
        assert.deepEqual(
            m03.getState("debug")?.debug?.readback?.baseline,
            live,
            "「运行」开始前没有抓基准 ⇒ ⇒ 高亮没有基准可比",
        );
    });

    test("R-11 · 「读寄存器」**不**动基准（读回 ≠ 重新取基准）", async () => {
        const live = Array.from({ length: 32 }, (_, i) => i + 1);
        const memory = fakeRegisterMemory({ gpr: live });
        const router = fakeRouter();
        const services = { commandRouter: router, registerMemory: memory };
        await app.handleCommand({ action: "continue" }, services);
        const baseline = m03.getState("debug")?.debug?.readback?.baseline;
        await app.handleCommand({ action: "read-regs" }, services);
        assert.deepEqual(
            m03.getState("debug")?.debug?.readback?.baseline,
            baseline,
            "「读寄存器」把基准冲掉了 ⇒ ⇒ 用户正在查看的那趟对比被抹掉",
        );
        assert.equal(memory.calls.readRegisters, 1, "「读寄存器」没有真的去读寄存器");
    });

    test("R-12 · 真停（断点）⇒ ⇒ 自动读回**三样**：寄存器 + 内存单元 + 转储", async () => {
        const memory = fakeRegisterMemory({
            gpr: Array(32).fill(0),
            memoryReads: [{ addr: 0x1000, size: "Word" }],
        });
        const router = fakeRouter();
        app.wireDebug(router, { registerMemory: memory });
        // 先制造一份「读这一段」的转储结果 ⇒ ⇒ 自动读回要**重读**它。
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), dump: { addr: 0x2000, bytes: [1, 2, 3, 4] } },
        }));
        router.handlers.get("halted")({ value: { reason: "Breakpoint", pc: 0 } });
        await settle();
        assert.equal(memory.calls.readRegisters, 1, "停下之后没有自动读回寄存器");
        assert.equal(memory.calls.readMemory.length, 1, "停下之后没有自动重读内存单元");
        assert.deepEqual(
            memory.calls.readMemory[0],
            { addr: 0x1000, size: "Word" },
            "内存单元重读的地址 / 宽度不对",
        );
        const dumps = router.dispatched.filter((p) => p?.DumpMemory !== undefined);
        assert.equal(dumps.length, 1, "停下之后没有自动重读「读这一段」的转储结果");
        assert.equal(dumps[0].DumpMemory.addr, 0x2000, "转储重读的地址不对");
        assert.equal(dumps[0].DumpMemory.len, 4, "转储重读的长度不对");
        assert.equal(
            m03.getState("debug")?.debug?.readback?.reason,
            rb.READBACK_REASON.BREAKPOINT,
            "读回原因没有落进视图状态",
        );
        assert.equal(
            m03.getState("debug")?.debug?.readback?.auto,
            true,
            "自动读回没有被标成「自动」",
        );
    });

    test("R-13 · 内存单元按**原来那份宽度**重读（宽度写死就红）", async () => {
        const memory = fakeRegisterMemory({
            gpr: Array(32).fill(0),
            memoryReads: [
                { addr: 0x1000, size: "Byte" },
                { addr: 0x2000, size: "Half" },
            ],
        });
        const router = fakeRouter();
        app.wireDebug(router, { registerMemory: memory });
        router.handlers.get("halted")({ value: { reason: "Trap", pc: 0 } });
        await settle();
        assert.equal(memory.calls.readMemory.length, 2, "两个单元都要重读");
        assert.deepEqual(
            memory.calls.readMemory.map((c) => c.size),
            ["Byte", "Half"],
            "重读的宽度与用户当初读的那份不一致 ⇒ ⇒ 会显示一个**错的值**",
        );
    });

    test("R-14 · 连续运行的窗口结束（`EndOfExecution`）**不**触发读回", async () => {
        const memory = fakeRegisterMemory({ gpr: Array(32).fill(0) });
        const router = fakeRouter();
        app.wireDebug(router, { registerMemory: memory });
        router.handlers.get("halted")({ value: { reason: "EndOfExecution", pc: 0 } });
        await settle();
        assert.equal(
            memory.calls.readRegisters,
            0,
            "连续运行的每个窗口都触发了一次读回 ⇒ ⇒ 脚本口会慢到不可用",
        );
    });

    test("R-15 · 用户点「暂停」后的那一次停下**要**触发读回", async () => {
        const memory = fakeRegisterMemory({ gpr: Array(32).fill(0) });
        const router = fakeRouter();
        app.wireDebug(router, { registerMemory: memory });
        const services = { commandRouter: router, registerMemory: memory };
        // ⚠️ **先让它真的在跑** ⇒ ⇒ 否则「暂停」那一格不会置位。
        await app.handleCommand({ action: "continue" }, services);
        await app.handleCommand({ action: "pause" }, services);
        router.handlers.get("halted")({ value: { reason: "EndOfExecution", pc: 0 } });
        await settle();
        assert.equal(
            memory.calls.readRegisters,
            1,
            "用户暂停之后的那次停下没有读回 ⇒ ⇒ 内存单元表不联动刷新（用户报的现象）",
        );
        assert.equal(
            m03.getState("debug")?.debug?.readback?.reason,
            rb.READBACK_REASON.USER_PAUSE,
            "读回原因没有落成「暂停」",
        );
    });

    test("R-16 · 「单步」走完 ⇒ ⇒ 自动读回（内核此刻静止）", async () => {
        const memory = fakeRegisterMemory({ gpr: Array(32).fill(0) });
        const router = fakeRouter();
        await app.handleCommand(
            { action: "step" },
            { commandRouter: router, registerMemory: memory },
        );
        assert.equal(memory.calls.readRegisters, 1, "单步走完没有自动读回");
        assert.equal(
            m03.getState("debug")?.debug?.readback?.reason,
            rb.READBACK_REASON.STEP,
            "读回原因没有落成「单步」",
        );
    });
});
