// End-to-end gate: M-07 transport frame → **real WASM host worker file**
// → WASM bridge → contract message → transport frame.
//
// The file under test is `front/kernel-worker.js` — the very file the
// browser loads. It runs inside a `node:worker_threads` Worker with the
// browser Worker API shimmed on (`node-worker-host-wasm.mjs`), and the
// WASM module is the fake double (`fake-wasm-module.mjs`) injected
// through the worker's `self.__RISCV_WASM_URL__` test seam: the real
// wasm-pack output is gitignored, so the gate must not depend on it.
//
// What is asserted here is the **bridge** (spec §4): the frame →
// message → frame mapping, the reqId placement (first event answers the
// command, the rest are pushes) and the failure paths (a throwing
// kernel, a non-JSON answer, a dead init) — all of which must come
// back as `Error` events instead of taking the worker down.

import { test, describe, after } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Worker as NodeWorker } from "node:worker_threads";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const workers = [];

/**
 * A browser-shaped Worker backed by `node:worker_threads`, running the
 * real `front/kernel-worker.js` against the fake wasm module.
 *
 * @param {string} mode — the fake module's behaviour mode
 */
function createWasmWorker(mode) {
    const wasmUrl = new URL(`./fake-wasm-module.mjs?mode=${mode}`, import.meta.url).href;
    const worker = new NodeWorker(new URL("./node-worker-host-wasm.mjs", import.meta.url), {
        workerData: { wasmUrl },
    });
    workers.push(worker);

    /** @type {object[]} */
    const frames = [];
    /** @type {Error[]} */
    const errors = [];
    /** @type {Function[]} */
    const waiters = [];

    function notify() {
        for (const waiter of waiters.splice(0)) waiter();
    }

    worker.on("message", (data) => {
        frames.push(data);
        notify();
    });
    // The worker throws when the wasm init fails (spec §4). Without a
    // listener Node would turn that into an uncaught exception in the
    // test process, so the error is captured here.
    worker.on("error", (err) => {
        errors.push(err);
        notify();
    });

    return {
        /** @param {object} frame */
        send(frame) {
            worker.postMessage(frame);
        },
        frames: () => frames.slice(),
        errors: () => errors.slice(),
        /**
         * @param {() => boolean} predicate
         * @param {number} [timeoutMs]
         */
        async waitFor(predicate, timeoutMs = 3000) {
            const start = Date.now();
            while (!predicate()) {
                if (Date.now() - start > timeoutMs) {
                    throw new Error("waitFor 超时：条件一直不成立");
                }
                await new Promise((r) => setTimeout(r, 5));
            }
        },
    };
}

/**
 * A command frame carrying one `CommandPayload` value.
 *
 * @param {object|string} payload — the inner `CommandPayload`
 * @param {{reqId?: number|null, sessionId?: string|null, role?: string|null}} [opts]
 * @returns {object}
 */
function commandFrame(payload, opts = {}) {
    return {
        type: "command",
        seq: 1,
        sessionId: opts.sessionId ?? "sess-1",
        role: opts.role ?? "embedded",
        reqId: opts.reqId ?? 7,
        payload: {
            schema_version: 2,
            direction: "Command",
            seq: 1,
            payload: { Command: payload },
        },
    };
}

after(async () => {
    for (const worker of workers) await worker.terminate();
});

describe("WASM 传输宿主 · 帧 → 消息 → 帧", () => {
    test("Nop 命令回一个事件帧（reqId / 会话 / 角色原样回传）", async () => {
        const host = createWasmWorker("nop");
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.type, "event", "回帧类型必须是 event");
        assert.equal(frame.reqId, 7, "首个事件必须带命令的 reqId");
        assert.equal(frame.sessionId, "sess-1", "sessionId 必须原样回传");
        assert.equal(frame.role, "embedded", "role 必须原样回传");
        assert.equal(frame.payload.direction, "Event", "payload 必须是契约事件消息");
        assert.deepEqual(
            frame.payload.payload,
            { Event: { Tick: { elapsed_ps: "0", pc: 0 } } },
            "事件负载不是内核给出的契约形状",
        );
        // ⚠️ **D-47**：帧 seq 是**传输层自己的**单调计数（传输层的序列
        // 空间），与内核 `Message.seq`（**载荷里的**一个字段）**解耦**。
        //
        // 此前这里断言的是「帧 seq == 事件的 seq」——那正是 D-47 说的
        // **两个序列空间**混用：同一个 Worker 内，宿主合成的错误帧用的是
        // 另一个计数器 ⇒ 帧 seq 可以从 11 跳回 1。
        assert.equal(typeof frame.seq, "number");
        assert.ok(Number.isInteger(frame.seq) && frame.seq > 0, "帧 seq 必须是正整数");
    });

    test("首个事件带 reqId，后续事件带 null（推送类）", async () => {
        const host = createWasmWorker("two");
        host.send(commandFrame("Nop", { reqId: 42 }));
        await host.waitFor(() => host.frames().length >= 2);

        const [first, second] = host.frames();
        assert.equal(first.reqId, 42, "首个事件必须带 reqId");
        assert.equal(second.reqId, null, "后续事件必须带 null");
        assert.equal(second.sessionId, "sess-1", "推送事件也要带会话");
        assert.equal(second.role, "embedded", "推送事件也要带角色");
        assert.equal(
            second.payload.payload.Event.Halted.reason,
            "Breakpoint",
            "第二个事件没有原样传回内核的事件负载",
        );
    });

    test("内核抛错 → 回一个 E_TRX-05 的 Error 事件", async () => {
        const host = createWasmWorker("throw");
        host.send(commandFrame("Nop", { reqId: 9 }));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.type, "event");
        assert.equal(frame.reqId, 9, "错误帧必须带命令的 reqId");
        assert.equal(frame.payload.direction, "Event");
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-05");
        assert.match(
            frame.payload.payload.Event.Error.message,
            /内核炸了/,
            "错误消息必须带上异常文本",
        );
    });

    test("内核返回非法 JSON → 回一个 E_TRX-05 的 Error 事件", async () => {
        const host = createWasmWorker("bad-json");
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-05");
        assert.match(frame.payload.payload.Event.Error.message, /JSON/i);
    });

    test("内核返回的不是数组 → 回一个 E_TRX-05 的 Error 事件", async () => {
        const host = createWasmWorker("not-array");
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-05");
        assert.match(frame.payload.payload.Event.Error.message, /数组/);
    });

    test("内核返回空数组 → 回一个 E_TRX-05 的 Error 事件", async () => {
        const host = createWasmWorker("empty");
        host.send(commandFrame("Nop", { reqId: 11 }));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.type, "event");
        assert.equal(frame.reqId, 11, "错误帧必须带命令的 reqId");
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-05");
        assert.match(
            frame.payload.payload.Event.Error.message,
            /没有返回任何事件/,
            "空数组必须被明确拒绝（否则命令会静默无回帧，只能等超时）",
        );
    });

    test("D-47：帧 seq 是传输层自己的单调序列（内核事件帧与宿主错误帧交错也不回退）", async () => {
        // ⚠️ **判别力**：这条用例守的是**帧 seq 只有一个序列空间**。
        //
        // 旧实现：内核事件帧用 `msg.seq`（内核的计数器）、宿主错误帧用
        // `hostSeq`（另一个从 1 起的计数器）⇒ 本场景下帧 seq 是
        // `[2, 3, 1]`——**从 3 跳回 1**。
        //
        // 新实现：全部帧走**传输层自己的**计数器 ⇒ `[1, 2, 3]`。
        const host = createWasmWorker("mix");
        host.send(commandFrame("Nop", { reqId: 1 }));
        await host.waitFor(() => host.frames().length >= 2);
        // 第二条命令让内核抛错 ⇒ 宿主回一帧**宿主侧错误**。
        host.send(commandFrame("Nop", { reqId: 2 }));
        await host.waitFor(() => host.frames().length >= 3);

        const frames = host.frames();
        assert.equal(frames.length, 3, "应当是 2 帧内核事件 + 1 帧宿主错误");
        // 前两帧是内核事件帧（Tick / Halted），第三帧是宿主错误帧。
        assert.ok(frames[0].payload.payload.Event.Tick, "第一帧应当是内核的 Tick 事件");
        assert.ok(frames[1].payload.payload.Event.Halted, "第二帧应当是内核的 Halted 事件");
        assert.equal(frames[2].payload.payload.Event.Error.code, "E_TRX-05");

        const seqs = frames.map((f) => f.seq);
        for (let i = 1; i < seqs.length; i += 1) {
            assert.ok(
                seqs[i] > seqs[i - 1],
                `帧 seq 必须严格递增：第 ${i} 帧是 ${seqs[i]}，前一帧是 ${seqs[i - 1]}`,
            );
        }
        // 帧 seq 与内核消息的 seq 是**两个空间**：第一帧的载荷 seq 是
        // 内核自己编的（2），帧 seq 是传输层的（1）。
        assert.notEqual(
            frames[0].seq,
            frames[0].payload.seq,
            "帧 seq 不能直接复用内核 Message.seq（D-47）",
        );
    });

    test("D-48：宿主侧错误是 E_TRX-05，**不是** E_TRX-04", async () => {
        // ⚠️ 语义变更（不是「为了让测试过而改断言」）：此前宿主侧与传输
        // 通道侧共用 `E_TRX-04` ⇒ 前端无法按码分派（「通道断了要重连」
        // 与「内核算错了要报错」是两种处置）。⇒ 宿主侧改用 `E_TRX-05`。
        const host = createWasmWorker("throw");
        host.send(commandFrame("Nop", { reqId: 9 }));
        await host.waitFor(() => host.frames().length >= 1);

        const code = host.frames()[0].payload.payload.Event.Error.code;
        assert.equal(code, "E_TRX-05", "宿主侧错误必须报 E_TRX-05");
        assert.notEqual(code, "E_TRX-04", "E_TRX-04 只表示传输通道错误");
    });

    test("非命令类型的帧被忽略（不产生回帧）", async () => {
        const host = createWasmWorker("nop");
        host.send({
            type: "event",
            seq: 3,
            sessionId: "sess-1",
            role: "embedded",
            reqId: null,
            payload: {
                schema_version: 2,
                direction: "Event",
                seq: 3,
                payload: { Event: { Tick: { elapsed_ps: "0", pc: 0 } } },
            },
        });
        await new Promise((r) => setTimeout(r, 200));
        assert.equal(host.frames().length, 0, "非命令帧不该产生回帧");
    });

    test("畸形帧被丢弃（不产生回帧，也不打挂 worker）", async () => {
        const host = createWasmWorker("nop");
        host.send({ nonsense: true });
        await new Promise((r) => setTimeout(r, 200));
        assert.equal(host.frames().length, 0, "畸形帧不该产生回帧");

        // The worker is still alive: a valid command still works.
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);
        assert.equal(host.frames()[0].type, "event");
    });

    test("WASM 初始化失败 → 回一个 E_TRX-05 的 Error 事件", async () => {
        const host = createWasmWorker("init-fail");
        // No command is sent: the failure must be reported on its own.
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-05");
        assert.match(
            frame.payload.payload.Event.Error.message,
            /初始化失败/,
            "初始化失败的错误消息必须说明原因",
        );
    });

    test("初始化失败时有命令在飞 → 只报一次，且带命令的 reqId", async () => {
        // The fake init fails **after** a delay, so the command below is
        // still waiting for the init when the failure hits.
        const host = createWasmWorker("init-fail-late");
        host.send(commandFrame("Nop", { reqId: 11 }));
        await host.waitFor(() => host.frames().length >= 1);
        await new Promise((r) => setTimeout(r, 150));

        const frames = host.frames();
        assert.equal(
            frames.length,
            1,
            "同一个初始化故障只该报一次（命令在飞时由命令帧承担，启动级帧不再重复）",
        );
        const frame = frames[0];
        assert.equal(frame.reqId, 11, "错误帧必须带命令的 reqId（M-05 要靠它拒命令）");
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-05");
        assert.match(frame.payload.payload.Event.Error.message, /初始化失败/);
    });

    test("同一个初始化故障在所有帧里只有一种文案", async () => {
        const host = createWasmWorker("init-fail");
        host.send(commandFrame("Nop", { reqId: 12 }));
        await host.waitFor(() => host.frames().length >= 1);
        await new Promise((r) => setTimeout(r, 150));

        const frames = host.frames();
        const messages = frames.map(
            (f) => f.payload?.payload?.Event?.Error?.message ?? "",
        );
        assert.ok(messages.length >= 1, "至少要有一帧报告初始化失败");
        assert.ok(
            messages.every((m) => m === messages[0]),
            `同一故障不应出现两种文案：${JSON.stringify(messages)}`,
        );
        assert.ok(
            frames.some((f) => f.reqId === 12),
            "在飞的命令必须拿到带 reqId 的错误帧",
        );
        assert.ok(
            frames.filter((f) => f.reqId === null).length <= 1,
            "启动级（无 reqId）错误帧最多一条",
        );
    });
});

describe("WASM 传输宿主 · import 安全性", () => {
    test("在 Node 主线程里 import 无副作用、不抛错", async () => {
        assert.equal(
            typeof globalThis.self,
            "undefined",
            "Node 主线程里没有 self 全局（守卫的前提）",
        );
        const mod = await import(pathToFileURL(resolve(frontDir, "kernel-worker.js")).href);
        assert.ok(mod, "kernel-worker.js 未能被 import");
        assert.equal(
            typeof globalThis.self,
            "undefined",
            "import 不该在全局作用域留下任何痕迹",
        );
    });
});
