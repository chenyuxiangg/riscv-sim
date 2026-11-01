// STAGE-45 · v0.69 批 —— **陈旧面收尾**（台账 **D-176** 做法 C + B）的
// 前端门禁（N 组 · **N-3 ~ N-12**）。
//
// 这个套件盯**一条用户能自己走通的路**：
//
//   程序跑过之后 ⇒ ⇒ 调试视图那张「反汇编」表**仍是旧 PC 那一段** ⇒ ⇒
//   界面**在骗人**（用户看不出来它已经过期）⇒ ⇒ 本批把它治掉：
//   **做法 C**（如实标出「这段是哪个 PC 时的」+「当前 PC 是多少」）＋
//   **做法 B**（加一个「**跟随 PC**」开关，**默认关**；打开时停下后按新 PC
//   自动重取一段）。
//
// ⚠️ **四条不变式**：
//   ① **段起点 = `entries[0].addr`**（**内核给的推导量**）⇒ ⇒ 前端**不**
//      记住「我请求了哪个地址」⇒ ⇒ **契约层零升版**；
//   ② **当前 PC** 来自 **M-10 的快照**⇒ ⇒ 与顶栏那份**同一个源**⇒ ⇒ 不是
//      第二个真相；
//   ③ **跟随 PC 默认关**⇒ ⇒ 用户**不主动要求**就不动他手动选的那一段
//      ⇒ ⇒ 「用户选的那段被顶掉」这个副作用**默认不会发生**；
//   ④ **重取的唯一落点 = 读回那条链路**⇒ ⇒ 「停下」这个时机**只有一处
//      定义**⇒ ⇒ 真停与单步**同一处**。
//
// ⚠️ **判别力清单**（每一条都要能**变红**）：
//   - N-3 钉「段起点 = 第一条的地址」⇒ ⇒ 改成「请求地址」这条**必红**；
//   - N-5 钉「跟随 PC 关闭 ⇒ ⇒ **不**跟随」⇒ ⇒ **必要对照**⇒ ⇒ 把它写成
//     恒真 ⇒ ⇒ 这条**必红**；
//   - N-8 钉「`data-disasm-stale` 跟着「段起点 ≠ 当前 PC」」⇒ ⇒ 写死成
//     `0` 这条**必红**；
//   - N-11 钉「跟随打开 ⇒ ⇒ 停下后**恰好一条** `Disasm`」⇒ ⇒ 多发或
//     少发都**必红**；
//   - N-12 钉「跟随关闭 ⇒ ⇒ **一条都不发**」⇒ ⇒ **必要对照**。
//
// ⚠️ **D-173 的那两条判据**在 `front/tests/embedded-landing.test.mjs`
//   （**N-1 / N-2**）—— 落地视图那份**重复的**虚拟时间 KPI **删掉了**⇒ ⇒
//   判据落在**被改的那个对象旁边**⇒ ⇒ 后人一眼看得见。
//
// ⚠️ **真交互**（真点复选框、真跑、真停下）由 `tools/browser-verify.mjs`
//   的 `staleSurfaces` 段（**B-130 ~ B-135**）负责 ⇒ ⇒ 两边合起来才是完整
//   的判据。

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
const df = await importFront("lib/disasm-follow.js");
const rb = await importFront("lib/readback.js");
const m02 = await importFront("views/m02_viewcomponents.js");
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
 * ⚠️ **必须带 `pc`** ⇒ ⇒ 「跟随 PC」重取用的就是它 ⇒ ⇒ 没有它 ⇒ ⇒
 *   「重取用的 PC 是哪个」那条判据**恒为 0** ⇒ ⇒ 空转。
 *
 * @param {{gpr?: number[], pc?: number, memoryReads?: {addr:number,size:string}[]}} [init]
 */
function fakeRegisterMemory(init = {}) {
    const calls = { readRegisters: 0, readMemory: [], flushRender: 0 };
    const state = {
        gpr: Array.isArray(init.gpr) ? init.gpr.slice() : Array(32).fill(0),
        pc: Number.isFinite(init.pc) ? init.pc : 0,
        memoryReads: Array.isArray(init.memoryReads) ? init.memoryReads.map((r) => ({ ...r })) : [],
    };
    return {
        calls,
        state,
        getSnapshot: () => ({
            gpr: state.gpr.slice(),
            pc: state.pc,
            timeVirt: 0,
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

/** 一个**记调用**的假 commandRouter。 */
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

/** 视图状态里那一份「跟随 PC」。 */
function followPcState() {
    return m03.getState("debug")?.debug?.followPc === true;
}

/** 把视图状态里的「跟随 PC」**写死**（不经过 app 层 ⇒ ⇒ 隔离测试）。 */
function setFollowPc(on) {
    m03.setState("debug", (prev) => ({
        ...prev,
        debug: { ...(prev.debug ?? {}), followPc: on === true },
    }));
}

/** 假的一条反汇编条目。 */
function entry(addr) {
    return {
        addr,
        code: 0x00000013,
        mnemonic: "addi x1, x0, 1",
        width: 4,
        file: null,
        line: null,
    };
}

// ---------------------------------------------------------------------------

describe("v0.69 · 「跟随 PC / 陈旧标注」的纯函数", () => {
    test("N-3 · 段起点 = 第一条的地址（**内核给的推导量** · 前端不记住请求地址）", () => {
        assert.equal(
            df.segmentBaseAddr([entry(0x1000), entry(0x1004)]),
            0x1000,
            "段起点不是第一条的地址 ⇒ ⇒ 那就得前端另存「我请求了哪个地址」",
        );
        assert.equal(df.segmentBaseAddr([]), null, "空表**不该**有段起点");
        assert.equal(df.segmentBaseAddr(null), null, "空表**不该**有段起点");
        assert.equal(
            df.segmentBaseAddr([{ addr: "不是数" }]),
            null,
            "地址不是数时**不猜**一个起点",
        );
    });

    test("N-4 · 陈旧判据：段起点 ≠ 当前 PC ⇒ ⇒ 陈旧；相等 ⇒ ⇒ 不陈旧；无内容 ⇒ ⇒ 不陈旧", () => {
        const entries = [entry(0x1000)];
        assert.equal(df.isStaleSegment(entries, 0x2000), true, "段起点 ≠ 当前 PC 却说不陈旧");
        assert.equal(df.isStaleSegment(entries, 0x1000), false, "段起点 == 当前 PC 却说陈旧");
        assert.equal(df.isStaleSegment([], 0x2000), false, "**没有东西**不算陈旧");
        assert.equal(df.isStaleSegment(entries, null), false, "读不到当前 PC 时**不猜**");
    });

    test("N-5 · 「跟随 PC」关闭 ⇒ ⇒ **一律不跟随**（**必要对照**）", () => {
        for (const reason of [
            rb.READBACK_REASON.BREAKPOINT,
            rb.READBACK_REASON.WATCHPOINT,
            rb.READBACK_REASON.TRAP,
            rb.READBACK_REASON.USER_PAUSE,
            rb.READBACK_REASON.STEP,
        ]) {
            assert.equal(
                df.shouldFollowDisasm({ followPc: false, reason }),
                false,
                `跟随 PC 关着（reason=${reason}）却要跟随 ⇒ ⇒ 用户没要求就被顶掉了`,
            );
        }
    });

    test("N-6 · 「跟随 PC」打开 ⇒ ⇒ 只在**真的停下**时跟随（手动 / 未知 ⇒ ⇒ 不跟随）", () => {
        for (const reason of [
            rb.READBACK_REASON.BREAKPOINT,
            rb.READBACK_REASON.WATCHPOINT,
            rb.READBACK_REASON.TRAP,
            rb.READBACK_REASON.USER_PAUSE,
            rb.READBACK_REASON.STEP,
        ]) {
            assert.equal(
                df.shouldFollowDisasm({ followPc: true, reason }),
                true,
                `停下（reason=${reason}）却没有跟随 ⇒ ⇒ 停下后那段还是旧的`,
            );
        }
        assert.equal(
            df.shouldFollowDisasm({ followPc: true, reason: rb.READBACK_REASON.MANUAL }),
            false,
            "手动读回**不是**「程序停下了」⇒ ⇒ 不该重取",
        );
        assert.equal(
            df.shouldFollowDisasm({ followPc: true, reason: "没见过的原因" }),
            false,
            "不认识的原因**不猜** ⇒ ⇒ 不跟随",
        );
        assert.equal(
            df.shouldFollowDisasm({ followPc: true, reason: null }),
            false,
            "没有原因**不猜** ⇒ ⇒ 不跟随",
        );
    });

    test("N-7 · 状态行三态分得开（「陈旧」两个字**只**出现在陈旧那一态）", () => {
        const none = df.disasmStatusText({ entries: [], pc: 0x2000 });
        assert.equal(none.stale, false, "没有内容却标成陈旧");
        assert.ok(none.text.includes("尚无反汇编"), `空态文案不对：${none.text}`);

        const fresh = df.disasmStatusText({ entries: [entry(0x1000)], pc: 0x1000 });
        assert.equal(fresh.stale, false, "段起点 == 当前 PC 却标成陈旧");
        assert.ok(!fresh.text.includes("陈旧"), `不该出现「陈旧」二字：${fresh.text}`);
        assert.ok(fresh.text.includes("0x00001000"), `没说是哪一段：${fresh.text}`);
        assert.ok(fresh.text.includes("0x00001000"), `没说是当前 PC：${fresh.text}`);

        const stale = df.disasmStatusText({ entries: [entry(0x1000)], pc: 0x2000 });
        assert.equal(stale.stale, true, "段起点 ≠ 当前 PC 却没标成陈旧");
        assert.ok(stale.text.includes("陈旧"), `陈旧那一态必须**如实标出**：${stale.text}`);
        assert.ok(stale.text.includes("0x00002000"), `没说是当前 PC：${stale.text}`);
    });

    test("N-7b · 「跟随 PC」开关的标签**两个都写**（状态 + 点了会发生什么）", () => {
        const on = df.followPcLabel({ followPc: true });
        assert.ok(on.includes("已开启"), `开着的标签没说开：${on}`);
        assert.ok(on.includes("点击关闭"), `开着的标签没说点了会怎样：${on}`);
        const off = df.followPcLabel({ followPc: false });
        assert.ok(off.includes("已关闭"), `关着的标签没说关：${off}`);
        assert.ok(off.includes("点击开启"), `关着的标签没说点了会怎样：${off}`);
    });
});

describe("v0.69 · 调试视图的反汇编卡（做法 C 的状态行 + 做法 B 的开关）", () => {
    /** 渲染调试视图。 */
    function renderDebug(extra = {}) {
        const view = m02.get("debug");
        assert.ok(view, "调试视图组件必须注册着");
        return view.render({
            sim: { gpr: Array(32).fill(0), pc: 0x2000, disasm: [] },
            debug: {},
            timeVirt: 0,
            ...extra,
        });
    }

    test("N-8 · 调试视图渲染出状态行与「跟随 PC」开关", () => {
        const html = renderDebug();
        assert.ok(html.includes("data-disasm-status"), "反汇编卡里没有状态行");
        assert.ok(html.includes("data-disasm-follow"), "反汇编卡里没有「跟随 PC」开关");
    });

    test("N-8b · 开关的勾选状态**跟着** `debug.followPc`（默认关 ⇒ ⇒ 不勾）", () => {
        const off = renderDebug({ debug: { followPc: false } });
        assert.ok(
            /data-disasm-follow(?![^>]*checked)/.test(off),
            "默认**不许**勾上（「跟随 PC」默认关）",
        );
        const on = renderDebug({ debug: { followPc: true } });
        assert.ok(
            /data-disasm-follow[^>]*checked/.test(on),
            "开着的时候复选框**必须**是勾上的",
        );
    });

    test("N-8c · `data-disasm-stale` 跟着「段起点 ≠ 当前 PC」（0 / 1）", () => {
        const fresh = renderDebug({
            sim: { gpr: Array(32).fill(0), pc: 0x1000, disasm: [entry(0x1000)] },
        });
        assert.ok(
            fresh.includes('data-disasm-stale="0"'),
            "段起点 == 当前 PC 时**不该**标陈旧",
        );
        const stale = renderDebug({
            sim: { gpr: Array(32).fill(0), pc: 0x2000, disasm: [entry(0x1000)] },
        });
        assert.ok(
            stale.includes('data-disasm-stale="1"'),
            "段起点 ≠ 当前 PC 时**必须**标陈旧（不骗人）",
        );
    });
});

describe("v0.69 · app 层编排（开关 + 停下后的重取）", () => {
    test("N-9 · 勾上「跟随 PC」⇒ ⇒ 写进视图状态**且立刻**重取一段（当前 PC）", async () => {
        setFollowPc(false);
        const memory = fakeRegisterMemory({ pc: 0x1234 });
        const router = fakeRouter();
        await app.handleCommand(
            { action: "disasm-follow", params: { follow: true } },
            { commandRouter: router, registerMemory: memory },
        );
        assert.equal(followPcState(), true, "「跟随 PC」没写进视图状态");
        const disasms = router.dispatched.filter((p) => p?.Disasm !== undefined);
        assert.equal(disasms.length, 1, "勾上之后**没有**立刻重取 ⇒ ⇒ 用户看不到变化");
        assert.equal(
            disasms[0].Disasm.addr,
            0x1234,
            "重取的地址**不是当前 PC** ⇒ ⇒ 跟错了地方",
        );
    });

    test("N-10 · 取消勾 ⇒ ⇒ 写回「关」**且不发**任何命令", async () => {
        setFollowPc(true);
        const memory = fakeRegisterMemory({ pc: 0x1234 });
        const router = fakeRouter();
        await app.handleCommand(
            { action: "disasm-follow", params: { follow: false } },
            { commandRouter: router, registerMemory: memory },
        );
        assert.equal(followPcState(), false, "取消勾之后视图状态还是「开」");
        const disasms = router.dispatched.filter((p) => p?.Disasm !== undefined);
        assert.equal(disasms.length, 0, "取消勾**不该**再发命令");
    });

    test("N-11 · 「跟随 PC」打开 ⇒ ⇒ 真停之后**恰好**重取一条（addr == 新 PC）", async () => {
        setFollowPc(true);
        const memory = fakeRegisterMemory({ pc: 0x2000 });
        const router = fakeRouter();
        app.wireDebug(router, { registerMemory: memory });
        router.handlers.get("halted")({ value: { reason: "Breakpoint", pc: 0x2000 } });
        await settle();
        const disasms = router.dispatched.filter((p) => p?.Disasm !== undefined);
        assert.equal(disasms.length, 1, "停下之后没有（只）重取一条反汇编");
        assert.equal(
            disasms[0].Disasm.addr,
            0x2000,
            "重取的地址**不是停下时的 PC** ⇒ ⇒ 还是旧的那一段",
        );
    });

    test("N-12 · 「跟随 PC」关闭 ⇒ ⇒ 真停之后**一条都不发**（**必要对照**）", async () => {
        setFollowPc(false);
        const memory = fakeRegisterMemory({ pc: 0x2000 });
        const router = fakeRouter();
        app.wireDebug(router, { registerMemory: memory });
        router.handlers.get("halted")({ value: { reason: "Breakpoint", pc: 0x2000 } });
        await settle();
        const disasms = router.dispatched.filter((p) => p?.Disasm !== undefined);
        assert.equal(
            disasms.length,
            0,
            "跟随 PC 关着却发了 Disasm ⇒ ⇒ 用户手动选的那一段**被顶掉了**",
        );
    });

    test("N-12b · 单步走完 ⇒ ⇒ 也跟随（单步**也是**「停下」）", async () => {
        setFollowPc(true);
        const memory = fakeRegisterMemory({ pc: 0x3000 });
        const router = fakeRouter();
        await app.handleCommand(
            { action: "step" },
            { commandRouter: router, registerMemory: memory },
        );
        await settle();
        const disasms = router.dispatched.filter((p) => p?.Disasm !== undefined);
        assert.equal(disasms.length, 1, "单步走完没有跟随 ⇒ ⇒ 用户最常做的事反而不跟随");
        assert.equal(
            disasms[0].Disasm.addr,
            0x3000,
            "单步后重取的地址**不是走完那一步之后的 PC**",
        );
    });

    test("N-12c · 连续运行的窗口结束（`EndOfExecution`）⇒ ⇒ **不**跟随", async () => {
        setFollowPc(true);
        const memory = fakeRegisterMemory({ pc: 0x2000 });
        const router = fakeRouter();
        app.wireDebug(router, { registerMemory: memory });
        router.handlers.get("halted")({ value: { reason: "EndOfExecution", pc: 0x2000 } });
        await settle();
        const disasms = router.dispatched.filter((p) => p?.Disasm !== undefined);
        assert.equal(
            disasms.length,
            0,
            "连续运行的**每个窗口**都重取 ⇒ ⇒ 脚本口会慢到不可用",
        );
    });
});
