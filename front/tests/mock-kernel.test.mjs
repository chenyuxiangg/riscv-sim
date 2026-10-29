// End-to-end gate: M-07 → **real worker file** → mock kernel → M-05.
//
// This is the test that proves the whole front-end service chain is
// really wired: the browser worker file (`front/dev/mock-kernel-worker.js`)
// is loaded into a `node:worker_threads` Worker, the transport talks to
// it with the M-07 transport frame, the mock kernel answers with the
// frozen contract envelope, and the command router matches the
// response back to the command.

import { test, describe, after } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Worker as NodeWorker } from "node:worker_threads";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));
void dom;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

/**
 * A command frame carrying one `CommandPayload` value.
 *
 * @param {object|string} payload — the inner `CommandPayload`
 * @param {number} [reqId]
 * @returns {object}
 */
function commandFrame(payload, reqId = 0) {
    return {
        type: "command",
        seq: 1,
        sessionId: null,
        role: null,
        reqId,
        payload: {
            schema_version: SCHEMA_VERSION,
            direction: "Command",
            seq: 1,
            payload: { Command: payload },
        },
    };
}

const m05 = await importFront("services/m05_commandrouter.js");
const m07 = await importFront("services/m07_transport.js");
const m10 = await importFront("services/m10_registermemory.js");
const { Command, SCHEMA_VERSION } = await importFront("lib/message.js");
const { createMockKernel } = await importFront("dev/mock-kernel.js");

const workers = [];

/**
 * A browser-shaped Worker backed by `node:worker_threads`.
 *
 * @returns {object}
 */
function createNodeWorker() {
    const worker = new NodeWorker(new URL("./node-worker-host.mjs", import.meta.url));
    workers.push(worker);
    return {
        postMessage: (data) => worker.postMessage(data),
        addEventListener(type, handler) {
            if (type === "message") worker.on("message", (data) => handler({ data }));
            else if (type === "error") worker.on("error", (err) => handler(err));
        },
        terminate: () => worker.terminate(),
    };
}

after(async () => {
    for (const worker of workers) await worker.terminate();
});

/**
 * Wait until a predicate holds. The worker answers with a *batch* of
 * frames (response + pushed events), and each frame arrives as its own
 * message — so the pushed events land one macrotask after the response
 * that resolved the command promise.
 *
 * @param {() => boolean} predicate
 * @param {number} [timeoutMs]
 */
async function waitFor(predicate, timeoutMs = 2000) {
    const start = Date.now();
    while (!predicate()) {
        if (Date.now() - start > timeoutMs) {
            throw new Error("waitFor 超时：条件一直不成立");
        }
        await new Promise((r) => setTimeout(r, 5));
    }
}

/**
 * Boot the whole chain: transport (worker mode) → router → regmem.
 */
async function bootChain() {
    const status = await m07.mountTransport({
        mode: "worker",
        workerUrl: "./dev/mock-kernel-worker.js",
        createWorker: createNodeWorker,
    });
    assert.equal(status.connected, true, "传输层没有连上 mock worker");
    const router = await m05.mountCommandRouter({ transport: m07 });
    const regmem = await m10.mountRegisterMemory({ commandRouter: router, transport: m07 });
    return { router, regmem };
}

describe("mock kernel（纯逻辑，进程内）", () => {
    test("Nop 得到一个 Tick 响应", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle({
            type: "command",
            seq: 1,
            sessionId: "sess-1",
            role: "embedded",
            reqId: 5,
            payload: {
                schema_version: SCHEMA_VERSION,
                direction: "Command",
                seq: 1,
                payload: { Command: "Nop" },
            },
        });
        assert.equal(frames.length, 1);
        assert.equal(frames[0].type, "event");
        assert.equal(frames[0].reqId, 5);
        assert.deepEqual(frames[0].payload.payload, { Event: { Tick: { elapsed_ps: "0", pc: 0 } } });
    });

    test("单步推进 PC 并把寄存器推成 Registers 事件", () => {
        const kernel = createMockKernel();
        // The kernel only pushes to subscribers of the register topic.
        kernel.handle({
            type: "command",
            seq: 1,
            sessionId: null,
            role: null,
            reqId: 0,
            payload: {
                schema_version: SCHEMA_VERSION,
                direction: "Command",
                seq: 1,
                payload: { Command: { Subscribe: { topic: "sim.registers" } } },
            },
        });
        const frames = kernel.handle({
            type: "command",
            seq: 1,
            sessionId: null,
            role: null,
            reqId: 1,
            payload: {
                schema_version: SCHEMA_VERSION,
                direction: "Command",
                seq: 1,
                payload: { Command: { StepN: { n: 1 } } },
            },
        });
        // The response is a Tick; the register delta follows.
        assert.deepEqual(frames[0].payload.payload, { Event: { Tick: { elapsed_ps: "1000", pc: 4 } } });
        const pushed = frames.find((f) => f.payload.payload.Event.Registers);
        assert.ok(pushed, "没有推送寄存器增量");
        const values = pushed.payload.payload.Event.Registers.values;
        assert.equal(values[1], 42, "x1 没有被写成 42");
        assert.equal(kernel.snapshot().pc, 4);
    });

    test("未注册的命令得到 E_CTX-05", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle({
            type: "command",
            seq: 1,
            sessionId: null,
            role: null,
            reqId: 1,
            payload: {
                schema_version: SCHEMA_VERSION,
                direction: "Command",
                seq: 1,
                payload: { Command: { NotACommand: {} } },
            },
        });
        assert.equal(frames.length, 1);
        assert.equal(frames[0].payload.payload.Event.Error.code, "E_CTX-05");
    });

    test("未映射地址的读失败报 E_BUS-01", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle({
            type: "command",
            seq: 1,
            sessionId: null,
            role: null,
            reqId: 1,
            payload: {
                schema_version: SCHEMA_VERSION,
                direction: "Command",
                seq: 1,
                payload: { Command: { ReadMemory: { addr: 0x90000000, size: "Word" } } },
            },
        });
        assert.equal(frames[0].payload.payload.Event.Error.code, "E_BUS-01");
    });
});

describe("mock 内核 · 断点契约（F1）", () => {
    test("ClearBreakpoint 按 id 删除断点（契约里没有 addr 字段）", () => {
        const kernel = createMockKernel();
        kernel.handle(commandFrame({ SetBreakpoint: { id: 7, addr: 0x40 } }));
        assert.deepEqual(kernel.snapshot().breakpoints, [[0x40, 7]]);

        // The frozen contract's `ClearBreakpoint { id }` carries no
        // address — the old implementation deleted address 0 and left the
        // breakpoint in place.
        const frames = kernel.handle(commandFrame({ ClearBreakpoint: { id: 7 } }));
        assert.deepEqual(kernel.snapshot().breakpoints, [], "断点没有被删掉");
        assert.equal(frames[0].payload.payload.Event.Tick.elapsed_ps, "0");
    });

    test("清掉断点后步进越过 0x40（不再在断点处停下）", () => {
        const kernel = createMockKernel();
        kernel.handle(commandFrame({ SetBreakpoint: { id: 7, addr: 0x40 } }));
        // With the breakpoint set, the run stops *before* executing the
        // instruction at 0x40 — five instructions, 5 × 1000 ps.
        kernel.handle(commandFrame("Continue"));
        assert.equal(kernel.snapshot().timeVirt, 5000, "断点没有在 0x40 生效");

        const cleared = createMockKernel();
        cleared.handle(commandFrame({ SetBreakpoint: { id: 7, addr: 0x40 } }));
        cleared.handle(commandFrame({ ClearBreakpoint: { id: 7 } }));
        cleared.handle(commandFrame("Continue"));
        // Without the breakpoint the instruction at 0x40 runs as well.
        assert.equal(cleared.snapshot().timeVirt, 6000, "断点被清掉后仍然停下了");
    });

    test("clear 一个不存在的 id 不会误删其它断点", () => {
        const kernel = createMockKernel();
        kernel.handle(commandFrame({ SetBreakpoint: { id: 7, addr: 0x40 } }));
        kernel.handle(commandFrame({ SetBreakpoint: { id: 8, addr: 0x20 } }));

        const frames = kernel.handle(commandFrame({ ClearBreakpoint: { id: 99 } }));
        assert.deepEqual(
            kernel.snapshot().breakpoints,
            [[0x40, 7], [0x20, 8]],
            "不存在的 id 误删了断点",
        );
        // An unknown id is not an error — the contract has no error code
        // for it, and the command still gets its Tick answer.
        assert.equal(frames[0].payload.payload.Event.Tick.elapsed_ps, "0");
    });
});

describe("mock 内核 · 换工程即换会话（F12）", () => {
    /** A minimal `model::Project` shaped payload. */
    const project = {
        id: "p",
        name: "p",
        tree: { root: { name: "r", component_id: null, children: [] } },
        images: [],
        version: { schema: 1, isa: "RV32IMC", toolchain: "mock" },
    };

    test("LoadProject 清空旧订阅（旧 topic 不再收到推送）", () => {
        const kernel = createMockKernel();
        kernel.handle(commandFrame({ Subscribe: { topic: "sim.registers" } }));
        assert.deepEqual(kernel.snapshot().topics, ["sim.registers"]);

        const frames = kernel.handle(commandFrame({ LoadProject: project }));
        assert.deepEqual(kernel.snapshot().topics, [], "旧订阅没有随工程一起失效");
        // The old topic receives no push any more — not even the
        // register snapshot the `LoadProject` answer would carry.
        assert.equal(
            frames.find((f) => f.payload.payload.Event.Registers),
            undefined,
            "旧 topic 仍然收到了推送",
        );
    });

    test("重新订阅后推送恢复", () => {
        const kernel = createMockKernel();
        kernel.handle(commandFrame({ Subscribe: { topic: "sim.registers" } }));
        kernel.handle(commandFrame({ LoadProject: project }));

        kernel.handle(commandFrame({ Subscribe: { topic: "sim.registers" } }));
        const frames = kernel.handle(commandFrame({ StepN: { n: 1 } }));
        assert.ok(
            frames.find((f) => f.payload.payload.Event.Registers),
            "重新订阅后没有收到推送",
        );
    });
});

describe("端到端：M-07 → 真实 worker 文件 → mock kernel → M-05", () => {
    test("Nop 往返成功（链路真的通）", async () => {
        const { router } = await bootChain();
        const event = await router.dispatchCommand(Command.nop());
        assert.equal(event.variant, "Tick");
        router.close();
    });

    test("单步后 PC 与寄存器经 M-10 落到视图", async () => {
        const { router, regmem } = await bootChain();
        await regmem.readRegisters();
        await router.dispatchCommand(Command.stepN(3));
        // The register delta is pushed *after* the command response —
        // wait for the register value, not for the PC (the Tick already
        // carries the PC).
        await waitFor(() => regmem.getSnapshot().gpr[3] === 49);
        const snapshot = regmem.getSnapshot();
        assert.equal(snapshot.pc, 0x0c, "单步三拍后 PC 应该是 0x0c");
        assert.equal(snapshot.gpr[1], 42);
        assert.equal(snapshot.gpr[2], 7);
        assert.equal(snapshot.gpr[3], 49);
        router.close();
        regmem.close();
    });

    test("继续执行命中断点并收到停止原因", async () => {
        const { router } = await bootChain();
        const halts = [];
        router.subscribe("halted", (event) => halts.push(event));
        await router.dispatchCommand(Command.continueRun());
        await waitFor(() => halts.length === 1);
        assert.equal(halts[0].value.reason, "Breakpoint");
        assert.equal(halts[0].value.pc, 0x40);
        router.close();
    });

    test("程序写入的内存可以被读回", async () => {
        const { router, regmem } = await bootChain();
        await router.dispatchCommand(Command.stepN(4)); // sw x3, 0x100(x0)
        const value = await regmem.readMemory(0x100, "Word");
        assert.equal(value.value, 49, "0x100 处应当是 x3 = 49");
        assert.equal(regmem.getSnapshot().memory["0x00000100"], 49);
        router.close();
        regmem.close();
    });

    test("写内存后读回新值", async () => {
        const { router, regmem } = await bootChain();
        const value = await regmem.writeMemory(0x300, "Word", 0xbeef);
        assert.equal(value.value, 0xbeef);
        router.close();
        regmem.close();
    });

    test("加载项目会复位内核状态", async () => {
        const { router, regmem } = await bootChain();
        await router.dispatchCommand(Command.stepN(3));
        assert.equal(regmem.getSnapshot().pc, 0x0c);
        await router.dispatchCommand(Command.loadProject({
            id: "p",
            name: "p",
            tree: { root: { name: "r", component_id: null, children: [] } },
            images: [],
            version: { schema: 1, isa: "RV32IMC", toolchain: "mock" },
        }));
        await regmem.readRegisters();
        assert.equal(regmem.getSnapshot().pc, 0, "加载项目后 PC 没有复位");
        assert.equal(regmem.getSnapshot().gpr[1], 0, "加载项目后寄存器没有复位");
        router.close();
        regmem.close();
    });

    test("未注册命令的响应让命令被拒绝", async () => {
        const { router } = await bootChain();
        await assert.rejects(
            router.dispatchCommand({ NotACommand: {} }),
            /E_CTX-05/,
        );
        router.close();
    });
});

// ---------------------------------------------------------------------------
// 契约 v9 · 调试器增强批（mock 内核）
//
// ⚠️ **mock 的数据是假的**（台账 D-55）⇒ ⇒ 这里验的是「**形状对不对**」与
//   「**契约语义对不对**」（精确命中 vs 前缀候选 · 原样拒不静默截断）⇒ ⇒
//   **不是**真内核的符号表。
//
// ⚠️ **两个上限取自契约镜像** ⇒ ⇒ 与契约层同源 ⇒ ⇒ 夹具一改就红。
// ---------------------------------------------------------------------------

describe("mock 内核 · 调试器增强批（契约 v9）", () => {
    test("`ResolveSymbol` 精确命中 ⇒ ⇒ `symbol` 有值、候选里**不含**它", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame(Command.resolveSymbol("main")));
        const evt = frames[0].payload.payload.Event.SymbolResolved;
        assert.ok(evt, "没有 SymbolResolved 事件");
        assert.equal(evt.name, "main", "必须**回显**请求里的名字");
        assert.ok(evt.symbol, "精确命中时 `symbol` 必须有值");
        assert.equal(evt.symbol.name, "main");
        assert.equal(typeof evt.symbol.addr, "number", "假符号也得带一个假地址");
        // ⚠️ **精确命中那条**不在候选里（裁决 B）⇒ ⇒ 但**前缀**命中的那些
        //   仍在（`main` 是 `main_loop` 的前缀 ⇒ ⇒ 它**是**一个候选）。
        assert.deepEqual(
            evt.candidates.map((c) => c.name),
            ["main_loop"],
            "候选必须**不含**精确命中那条",
        );
        assert.equal(evt.total, 3, "`total` 是这张假符号表的条数");
    });

    test("`ResolveSymbol` 只给前缀 ⇒ ⇒ `symbol` 为 null、候选按名字升序", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame(Command.resolveSymbol("ma")));
        const evt = frames[0].payload.payload.Event.SymbolResolved;
        assert.equal(evt.symbol, null, "只有前缀命中时 `symbol` 必须是 null");
        assert.deepEqual(
            evt.candidates.map((c) => c.name),
            ["main", "main_loop"],
            "前缀候选必须**只**含前缀命中的那些，且按**名字升序**",
        );
        assert.equal(
            evt.candidates.some((c) => c.name === "helper"),
            false,
            "不是前缀的符号**不许**进候选",
        );
    });

    test("`ResolveSymbol` 名字为空 ⇒ ⇒ `E_DBG-04`，**不发** `SymbolResolved`", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame(Command.resolveSymbol("   ")));
        assert.equal(frames.length, 1, "只回一条（错误）");
        assert.equal(frames[0].payload.payload.Event.Error.code, "E_DBG-04");
        assert.equal(
            frames[0].payload.payload.Event.SymbolResolved,
            undefined,
            "被拒时**不许**发 SymbolResolved",
        );
    });

    test("`DumpMemory` ⇒ ⇒ 回的字节数 == 请求的 `len`", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame(Command.dumpMemory(0x1000, 40)));
        const evt = frames[0].payload.payload.Event.MemoryDump;
        assert.ok(evt, "没有 MemoryDump 事件");
        assert.equal(evt.addr, 0x1000, "必须**回显**起始地址");
        assert.equal(evt.bytes.length, 40, "字节数必须 == 请求的 `len`");
        for (const b of evt.bytes) {
            assert.ok(Number.isInteger(b) && b >= 0 && b <= 255, "字节必须在 0..255");
        }
    });

    test("`DumpMemory` 长度越界 ⇒ ⇒ `E_DBG-03`（原样拒，不静默截断）", () => {
        const kernel = createMockKernel();
        for (const len of [0, 4097]) {
            const frames = kernel.handle(commandFrame(Command.dumpMemory(0x1000, len)));
            assert.equal(frames.length, 1, "只回一条（错误）");
            assert.equal(frames[0].payload.payload.Event.Error.code, "E_DBG-03");
            assert.equal(
                frames[0].payload.payload.Event.MemoryDump,
                undefined,
                "被拒时**不许**发 MemoryDump（不许静默截断）",
            );
        }
    });

    test("`DumpMemory` 正好在上限上 ⇒ ⇒ **放行**（4096 是合法长度）", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame(Command.dumpMemory(0, 4096)));
        const evt = frames[0].payload.payload.Event.MemoryDump;
        assert.ok(evt, "上限本身必须放行");
        assert.equal(evt.bytes.length, 4096);
    });
});
