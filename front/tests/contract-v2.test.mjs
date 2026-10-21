// Contract v2 gate — numeric policy (§2), transport-frame crc (§7.2) and
// the new command / event variants (§4).
//
// Three things this file exists to **pin**, because each of them is a
// behaviour that a plausible-looking implementation gets *wrong*:
//
//   1. `psToNumber()` must **throw** above 2^53 — silently rounding a
//      virtual time is exactly the failure the §2 rule exists to kill.
//      `Number("9007199254740993")` rounds to ...992, so a
//      `Number()`-based implementation passes every "does it return a
//      number" test and still loses the picosecond.
//   2. The frame `crc` covers the **canonical** JSON — keys sorted
//      **recursively**. Sorting only the top level, or not sorting at
//      all, both produce a different checksum for the same payload.
//   3. A frame **without** a `crc` is accepted (old producers), a frame
//      **with a wrong** `crc` is rejected with E_PROTO-06. Treating
//      "wrong" as "missing" silently accepts corrupted payloads.
//
// The assertions land on **behaviour** (the returned value, the thrown
// code, the checksum), never on "it did not throw".

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
    SCHEMA_VERSION,
    MAX_EXACT_PS,
    ps,
    psToNumber,
    Command,
    Event,
    commandMessage,
    eventMessage,
    eventTopic,
    decodeMessage,
    waveform,
} from "../lib/message.js";
import {
    crc32,
    canonicalJson,
    frameCrc,
    encodeFrame,
    decodeFrame,
    FRAME_TYPE,
} from "../lib/frame.js";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const m05 = await importFront("services/m05_commandrouter.js");
const m10 = await importFront("services/m10_registermemory.js");
const { createMockKernel, MOCK_CAP_BITS } = await importFront("dev/mock-kernel.js");

// ---------------------------------------------------------------------------
// §2 · 虚拟时间的线上表示
// ---------------------------------------------------------------------------

describe("契约 v2 · 虚拟时间走十进制字符串（§2）", () => {
    test("ps() 产出十进制字符串", () => {
        assert.equal(ps(0), "0");
        assert.equal(ps(1000), "1000");
        assert.equal(ps(9007199254740992), "9007199254740992");
        assert.equal(typeof ps(42), "string");
    });

    test("psToNumber() 把线上表示读回**精确**的值", () => {
        assert.equal(psToNumber("0"), 0);
        assert.equal(psToNumber("1000"), 1000);
        assert.equal(psToNumber("9007199254740992"), 9007199254740992);
    });

    test("psToNumber() 超过 2^53 **抛错**，不是截断（E_PROTO-04）", () => {
        // 2^53 + 1 —— `Number()` 会把它悄悄变成 2^53。
        //
        // ⚠️ 错误码必须是 **E_PROTO-04**（输入超限：数值的大小超过了消费
        // 者能精确表示的上限），**不是** E_PROTO-02——后者的原义是「未知
        // 字段」，在契约里是**不抛错**的（只增不改，仅记一条日志）。拿一
        // 个「仅日志」的码去抛错，语义是反的。
        //
        // 断言钉到 **code**（不是「抛了就行」）：把 04 写成 02，用例照样
        // 全绿——除非它真的区分这两个码。
        assert.throws(() => psToNumber("9007199254740993"), /E_PROTO-04/);
        assert.throws(() => psToNumber("9007199254740993"), (err) => {
            assert.equal(err.code, "E_PROTO-04");
            return true;
        });
        // 更远的值同样拒绝。
        assert.throws(() => psToNumber("18446744073709551615"), /E_PROTO-04/);
        assert.throws(() => psToNumber("18446744073709551615"), (err) => {
            assert.equal(err.code, "E_PROTO-04");
            return true;
        });
    });

    test("psToNumber() 超过 2^53 时**不**返回被四舍五入的数", () => {
        let caught = null;
        try {
            psToNumber("9007199254740993");
        } catch (err) {
            caught = err;
        }
        assert.ok(caught, "必须抛错，而不是返回一个被舍入的数");
        assert.equal(caught.code, "E_PROTO-04");
        // 错误信息里不能出现那个被舍入的值 —— 它会让人以为读到了真值。
        assert.ok(!caught.message.includes("9007199254740992"), "错误信息泄露了被舍入的值");
    });

    test("psToNumber() 接受数字形态（旧生产者兼容）", () => {
        assert.equal(psToNumber(0), 0);
        assert.equal(psToNumber(1000), 1000);
        assert.equal(psToNumber(MAX_EXACT_PS), MAX_EXACT_PS);
    });

    test("psToNumber() 的数字形态超过 2^53 也**抛错**", () => {
        // 数字形态的精度可能早已丢失，但**太大**的值仍然要拒——
        // 放行它等于把「读不出来的时刻」当成「读出来的时刻」。
        assert.throws(() => psToNumber(1e300), /E_PROTO-04/);
        assert.throws(() => psToNumber(1e300), (err) => {
            assert.equal(err.code, "E_PROTO-04");
            return true;
        });
    });

    test("psToNumber() 对畸形输入抛 E_PROTO-08（不是静默当成 0）", () => {
        assert.throws(() => psToNumber("abc"), /E_PROTO-08/);
        assert.throws(() => psToNumber(""), /E_PROTO-08/);
        assert.throws(() => psToNumber("-1"), /E_PROTO-08/);
        assert.throws(() => psToNumber(undefined), /E_PROTO-08/);
        assert.throws(() => psToNumber(null), /E_PROTO-08/);
        assert.throws(() => psToNumber(1.5), /E_PROTO-08/);
    });

    test("ps() 对极大值产出**纯十进制**（不是指数记法）", () => {
        // `String(1e21)` = "1e+21" —— 那**不是**十进制整数字符串，
        // `psToNumber` 会把它判成「形态非法」（E_PROTO-08）。但 1e21 > 2^53，
        // 按规格 §2.4 正确的分类是「输入超限」（E_PROTO-04）——分类错了，
        // 「超限」与「畸形」就混成了同一件事。
        const text = ps(1e21);
        assert.match(text, /^\d+$/, "ps() 必须产出纯十进制字符串");
        assert.equal(text, "1000000000000000000000");
        assert.throws(() => psToNumber(text), (err) => {
            assert.equal(err.code, "E_PROTO-04");
            return true;
        });
        // 常规路径（普通整数）行为不变。
        assert.equal(ps(0), "0");
        assert.equal(ps(1000), "1000");
        assert.equal(ps(9007199254740992), "9007199254740992");
    });

    test("Tick 事件的 elapsed_ps 是字符串", () => {
        const evt = Event.tick(1234, 0x40);
        assert.equal(typeof evt.Tick.elapsed_ps, "string");
        assert.equal(evt.Tick.elapsed_ps, "1234");
        // 信封里也是字符串。
        const decoded = decodeMessage(eventMessage(1, evt));
        assert.equal(decoded.value.elapsed_ps, "1234");
    });

    test("SimStateDelta 的 t_ps 是字符串，pc 没有变化时是 null", () => {
        const delta = Event.simStateDelta([{ index: 1, value: 42 }], null, 5000);
        assert.equal(delta.SimStateDelta.t_ps, "5000");
        assert.equal(delta.SimStateDelta.pc, null);
        assert.deepEqual(delta.SimStateDelta.registers, [{ index: 1, value: 42 }]);
        const withPc = Event.simStateDelta([], 0x10, 0);
        assert.equal(withPc.SimStateDelta.pc, 0x10);
    });

    test("波形边沿的 t_ps 是字符串", () => {
        const w = waveform([{ id: "cpu.gpr.x1", label: "x1", edges: [{ t_ps: 5, value: 42 }] }]);
        assert.equal(typeof w.streams[0].edges[0].t_ps, "string");
        assert.equal(w.streams[0].edges[0].t_ps, "5");
    });
});

// ---------------------------------------------------------------------------
// §4 · 新命令 / 新事件
// ---------------------------------------------------------------------------

describe("契约 v2 · 新命令与新事件（§4 / §4.5）", () => {
    test("三个新命令的变体名与字段名", () => {
        assert.deepEqual(Command.readRegisters(1, 4), { ReadRegisters: { index: 1, count: 4 } });
        assert.deepEqual(Command.writeRegister(3, 0x2a), { WriteRegister: { index: 3, value: 0x2a } });
        assert.deepEqual(Command.negotiate(2, 0x0f), {
            Negotiate: { min_schema_version: 2, cap_bits: 0x0f },
        });
    });

    test("三个新事件的变体名与字段名", () => {
        assert.deepEqual(Event.registers(2, [7, 8]), { Registers: { index: 2, values: [7, 8] } });
        assert.deepEqual(Event.negotiated(2, 0x0f, "k"), {
            Negotiated: { schema_version: 2, cap_bits: 0x0f, kernel: "k" },
        });
        assert.deepEqual(Event.simStateDelta([{ index: 1, value: 9 }], 0x20, 100), {
            SimStateDelta: { registers: [{ index: 1, value: 9 }], pc: 0x20, t_ps: "100" },
        });
    });

    test("topic 映射逐字符合规格 §4.5", () => {
        assert.equal(eventTopic("Registers"), "sim.registers");
        assert.equal(eventTopic("Negotiated"), "sim.negotiated");
        assert.equal(eventTopic("SimStateDelta"), "sim.state.delta");
        assert.equal(eventTopic("WaveformHistory"), "waveform.history");
    });

    test("D-34 改名之后旧名字不再有 topic（不能静默兼容两个名字）", () => {
        assert.equal(eventTopic("WaveformDelta"), "unknown");
    });

    test("M-05 把新事件路由到新 topic", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const seen = [];
        for (const topic of ["sim.registers", "sim.negotiated", "sim.state.delta", "waveform.history"]) {
            router.subscribe(topic, (event) => seen.push([topic, event.variant]));
        }
        transport.emit(eventFrame(null, Event.registers(0, [1])));
        transport.emit(eventFrame(null, Event.negotiated(2, 0, "k")));
        transport.emit(eventFrame(null, Event.simStateDelta([], null, 0)));
        transport.emit(eventFrame(null, Event.waveformHistory({ streams: [] })));
        assert.deepEqual(seen, [
            ["sim.registers", "Registers"],
            ["sim.negotiated", "Negotiated"],
            ["sim.state.delta", "SimStateDelta"],
            ["waveform.history", "WaveformHistory"],
        ]);
        router.close();
    });
});

// ---------------------------------------------------------------------------
// §7.2 · 传输帧的 crc
// ---------------------------------------------------------------------------

describe("契约 v2 · 传输帧 crc（§7.2）", () => {
    test("CRC-32 算法用已知测试向量钉住", () => {
        // CRC-32/ISO-HDLC 的标准测试向量。
        assert.equal(crc32("123456789"), 0xcbf43926);
        assert.equal(crc32(""), 0);
        assert.equal(crc32("a"), 0xe8b7be43);
    });

    test("crc 是 8 位小写十六进制（无 0x 前缀）", () => {
        const crc = frameCrc({ schema_version: 2, direction: "Command" });
        assert.match(crc, /^[0-9a-f]{8}$/);
    });

    test("encodeFrame 写入 crc，decodeFrame 对正确的 crc 放行", () => {
        const frame = encodeFrame({
            type: FRAME_TYPE.Command,
            seq: 1,
            sessionId: "s",
            role: "r",
            reqId: 1,
            payload: commandMessage(1, Command.nop()),
        });
        assert.equal(typeof frame.crc, "string");
        assert.equal(frame.crc, frameCrc(frame.payload));
        const decoded = decodeFrame(JSON.parse(JSON.stringify(frame)));
        assert.equal(decoded.crc, frame.crc);
        assert.equal(decodeMessage(decoded.payload).variant, "Nop");
    });

    test("decodeFrame 对**算错**的 crc 抛 E_PROTO-06", () => {
        const frame = encodeFrame({
            type: FRAME_TYPE.Event,
            seq: 1,
            sessionId: null,
            role: null,
            reqId: null,
            payload: eventMessage(1, Event.tick(1, 2)),
        });
        const broken = { ...frame, crc: "deadbeef" };
        assert.throws(() => decodeFrame(broken), /E_PROTO-06/);
        assert.throws(() => decodeFrame(broken), (err) => {
            assert.equal(err.code, "E_PROTO-06");
            return true;
        });
        // 载荷被改过而 crc 没改 —— 同样必须拒绝。
        const tampered = {
            ...frame,
            payload: eventMessage(1, Event.tick(999, 2)),
        };
        assert.throws(() => decodeFrame(tampered), /E_PROTO-06/);
    });

    test("decodeFrame 对**缺失**的 crc 放行（旧生产者兼容）", () => {
        const frame = encodeFrame({
            type: FRAME_TYPE.Event,
            seq: 1,
            sessionId: null,
            role: null,
            reqId: null,
            payload: eventMessage(1, Event.tick(1, 2)),
        });
        const { crc, ...withoutCrc } = frame;
        assert.match(crc, /^[0-9a-f]{8}$/);
        assert.equal("crc" in withoutCrc, false, "crc 必须真的被拿掉");
        const decoded = decodeFrame(withoutCrc);
        assert.equal(decoded.crc, null);
        assert.equal(decoded.type, "event");
    });

    test("crc 与键的插入顺序无关（规范 JSON 递归排序键）", () => {
        const a = {
            schema_version: 2,
            direction: "Command",
            seq: 1,
            payload: { Command: { StepN: { n: 3 } } },
        };
        // 同样的内容，键的插入顺序完全不同（顶层 + 嵌套都换）。
        const b = {
            payload: { Command: { StepN: { n: 3 } } },
            seq: 1,
            direction: "Command",
            schema_version: 2,
        };
        assert.notDeepEqual(Object.keys(a), Object.keys(b), "两边的插入顺序必须不同，否则这条用例没有判别力");
        assert.equal(frameCrc(a), frameCrc(b));
    });

    test("嵌套对象的键序也不影响 crc（只排顶层是不够的）", () => {
        const a = { outer: { alpha: 1, beta: 2 } };
        const b = { outer: { beta: 2, alpha: 1 } };
        assert.notDeepEqual(Object.keys(a.outer), Object.keys(b.outer));
        assert.equal(frameCrc(a), frameCrc(b));
    });

    test("规范 JSON 递归排序键，数组保持原序", () => {
        const text = canonicalJson({ b: 1, a: { d: 2, c: 3 }, list: [3, 1, 2] });
        assert.equal(text, '{"a":{"c":3,"d":2},"b":1,"list":[3,1,2]}');
    });

    test("crc 只覆盖 payload：会话信息变了不影响它", () => {
        const payload = commandMessage(1, Command.nop());
        const one = encodeFrame({ type: FRAME_TYPE.Command, seq: 1, sessionId: "a", role: "r", reqId: 1, payload });
        const two = encodeFrame({ type: FRAME_TYPE.Command, seq: 9, sessionId: "b", role: "r2", reqId: 2, payload });
        assert.equal(one.crc, two.crc);
    });
});

// ---------------------------------------------------------------------------
// M-10 · 消费新事件
// ---------------------------------------------------------------------------

describe("契约 v2 · M-10 消费新事件与新命令", () => {
    test("readRegisters 发的是 ReadRegisters 命令（不再靠订阅搭车）", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const promise = service.readRegisters(0, 4);
        assert.deepEqual(transport.sent[0].message.payload, {
            Command: { ReadRegisters: { index: 0, count: 4 } },
        });
        transport.emit(eventFrame(transport.sent[0].reqId, Event.registers(0, [1, 2, 3, 4])));
        await promise;
        assert.deepEqual(service.getSnapshot().gpr.slice(0, 4), [1, 2, 3, 4]);
        router.close();
        service.close();
    });

    test("寄存器事件按**内核回报的实际起点**落缓存", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        // 内核把请求钳到了 30，回复带的是实际起点 30 —— 前端不能假设
        // 回复的起点等于请求的起点。
        const promise = service.readRegisters(30, 4);
        transport.emit(eventFrame(transport.sent[0].reqId, Event.registers(30, [11, 12])));
        await promise;
        const gpr = service.getSnapshot().gpr;
        assert.equal(gpr[30], 11);
        assert.equal(gpr[31], 12);
        assert.equal(gpr[0], 0, "值落错了位置");
        router.close();
        service.close();
    });

    test("sim.registers 主题的推送落到寄存器缓存", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        transport.emit(eventFrame(null, Event.registers(1, [42, 43])));
        const gpr = service.getSnapshot().gpr;
        assert.equal(gpr[1], 42);
        assert.equal(gpr[2], 43);
        router.close();
        service.close();
    });

    test("sim.state.delta 主题的推送落到寄存器缓存与 pc", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        transport.emit(eventFrame(null, Event.simStateDelta([{ index: 5, value: 99 }], 0x20, 1000)));
        const snapshot = service.getSnapshot();
        assert.equal(snapshot.gpr[5], 99);
        assert.equal(snapshot.pc, 0x20);
        router.close();
        service.close();
    });

    test("writeRegister 发的是 WriteRegister 命令", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const promise = service.writeRegister(3, 0x2a);
        assert.deepEqual(transport.sent[0].message.payload, {
            Command: { WriteRegister: { index: 3, value: 0x2a } },
        });
        transport.emit(eventFrame(transport.sent[0].reqId, Event.registers(3, [0x2a])));
        await promise;
        assert.equal(service.getSnapshot().gpr[3], 0x2a);
        router.close();
        service.close();
    });

    test("readRegisters(0, 0) 正常 resolve（空区间也要有回复，不超时）", async () => {
        const transport = mockKernelTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        // ⚠️ 判别式落在**服务层**：M-05 按 `reqId` 配对请求与回复，
        // 「没有回复」与「消息丢了」在协议上不可区分。内核要是真的不回复，
        // 这里会 500 ms 之后以 E_CTX-06（命令超时）拒绝——一个**合法**
        // 请求被报成「内核没应答」。
        const event = await service.readRegisters(0, 0);
        assert.equal(event.variant, "Registers");
        assert.deepEqual(event.value.values, [], "空区间的 values 是空数组");
        assert.equal(event.value.index, 0, "index 是钳位后的实际起点");
        router.close();
        service.close();
    });

    test("虚拟时间从字符串读回（超过 2^53 时报错而不是舍入）", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        transport.emit(eventFrame(null, Event.tick("1000", 0x10)));
        assert.equal(service.getSnapshot().timeVirt, 1000);

        // 超过 2^53 —— 必须**报错**，不能悄悄变成一个舍入过的值。
        transport.emit(eventFrame(null, Event.tick("9007199254740993", 0x10)));
        // 错误码钉到 **E_PROTO-04**（输入超限），不是 E_PROTO-02（未知
        // 字段 · 仅日志）。
        assert.match(service.getSnapshot().lastError, /^E_PROTO-04:/);
        assert.equal(
            service.getSnapshot().timeVirt,
            1000,
            "时间被静默改写成了别的值",
        );
        router.close();
        service.close();
    });

    test("Tick 缺 elapsed_ps 时保留旧时间并报 E_PROTO-08（时间轴不倒退）", async () => {
        const transport = fakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        transport.emit(eventFrame(null, Event.tick("1000", 0x10)));
        assert.equal(service.getSnapshot().timeVirt, 1000);

        // 缺 `elapsed_ps` 的 tick —— 这是**畸形**，不是 0。
        //
        // ⚠️ 判别式落在**时间轴**上：把缺失当成 0 会让虚拟时间从 1000
        // **倒退**到 0（时间倒流），而且**不报任何错**。
        transport.emit(eventFrame(null, { Tick: { pc: 0x14 } }));
        assert.equal(
            service.getSnapshot().timeVirt,
            1000,
            "缺失 elapsed_ps 让虚拟时间倒退了",
        );
        assert.match(
            service.getSnapshot().lastError ?? "",
            /^E_PROTO-08:/,
            "缺失 elapsed_ps 没有被报成畸形",
        );
        router.close();
        service.close();
    });
});

// ---------------------------------------------------------------------------
// mock 内核 · §5 的接线语义
// ---------------------------------------------------------------------------

describe("契约 v2 · mock 内核的 §5 语义", () => {
    test("ReadRegisters 把 index / count 钳到 32 个寄存器并回报实际起点", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({ ReadRegisters: { index: 30, count: 10 } }));
        const reply = frames.find((f) => f.payload.payload.Event.Registers);
        assert.ok(reply, "没有回复 Registers");
        assert.equal(reply.payload.payload.Event.Registers.index, 30);
        assert.equal(reply.payload.payload.Event.Registers.values.length, 2);
    });

    test("ReadRegisters 的起点也钳位（不能读越界）", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({ ReadRegisters: { index: 99, count: 4 } }));
        const reply = frames.find((f) => f.payload.payload.Event.Registers);
        assert.equal(reply.payload.payload.Event.Registers.index, 31);
        assert.equal(reply.payload.payload.Event.Registers.values.length, 1);
    });

    test("ReadRegisters count=0 也要**回复**（空区间，不是「没有回复」）", () => {
        // 规格 §5 已订正：「不回复」不在 M-05 的回复模型里——它按 reqId
        // 配对请求与回复，所以「没有回复」与「消息丢了」不可区分，一个
        // 合法请求会变成一条**假超时**（E_CTX-06）。
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({ ReadRegisters: { index: 99, count: 0 } }));
        const reply = frames.find((f) => f.payload.payload.Event.Registers);
        assert.ok(reply, "count=0 也必须回复");
        assert.deepEqual(
            reply.payload.payload.Event.Registers.values,
            [],
            "空区间的 values 是空数组",
        );
        assert.equal(
            reply.payload.payload.Event.Registers.index,
            31,
            "index 仍是钳位后的实际起点",
        );
    });

    test("写 x0 被丢弃但**仍回复**（值恒为 0）", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({ WriteRegister: { index: 0, value: 42 } }));
        const reply = frames.find((f) => f.payload.payload.Event.Registers);
        assert.ok(reply, "写 x0 也必须回复");
        assert.deepEqual(reply.payload.payload.Event.Registers.values, [0]);
        assert.equal(kernel.snapshot().gpr[0], 0);
    });

    test("写越界是**拒绝**（E_DBG-01），不是钳位", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({ WriteRegister: { index: 40, value: 7 } }));
        const err = frames.find((f) => f.payload.payload.Event.Error);
        assert.ok(err, "越界写必须被拒绝");
        assert.equal(err.payload.payload.Event.Error.code, "E_DBG-01");
        assert.equal(
            frames.find((f) => f.payload.payload.Event.Registers),
            undefined,
            "越界写不能被悄悄钳到别的寄存器",
        );
    });

    test("Negotiate 回内核定下的版本与**内核授予的**能力位", () => {
        // 规格 §5：回复带的是**内核授予的** `cap_bits`，**不是**宿主
        // 提供的那个。宿主提供的是「报价」，规格没有定义取交集——回显
        // 报价等于凭空造出第二套协商语义。
        //
        // ⚠️ 判别式：宿主这里报 `0x0f`（与内核授予的那一组**不同**），
        // 所以「回显宿主值」的实现会在这里**红**。
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({ Negotiate: { min_schema_version: 2, cap_bits: 0x0f } }));
        const reply = frames.find((f) => f.payload.payload.Event.Negotiated);
        assert.ok(reply, "没有回复 Negotiated");
        assert.equal(reply.payload.payload.Event.Negotiated.schema_version, SCHEMA_VERSION);
        assert.equal(
            reply.payload.payload.Event.Negotiated.cap_bits,
            MOCK_CAP_BITS,
            "回复的能力位必须是内核授予的那一组",
        );
        assert.notEqual(
            reply.payload.payload.Event.Negotiated.cap_bits,
            0x0f,
            "回复回显了宿主提供的能力位",
        );
        assert.match(reply.payload.payload.Event.Negotiated.kernel, /RV32IMC/);
    });

    test("Negotiate 的能力位**不随宿主的报价变**（报价是 0 也不能回 0）", () => {
        // 宿主报 0（= 什么能力都不要）时，内核仍然回**自己授予的**那
        // 一组：能力位是内核的裁决，不是宿主的输入。
        //
        // 「空能力位」是内核侧明令禁止的（前端会按位降级），所以这条
        // 断言也钉住了「回复永远非 0」。
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({ Negotiate: { min_schema_version: 2, cap_bits: 0 } }));
        const reply = frames.find((f) => f.payload.payload.Event.Negotiated);
        assert.ok(reply, "没有回复 Negotiated");
        const capBits = reply.payload.payload.Event.Negotiated.cap_bits;
        assert.equal(capBits, MOCK_CAP_BITS, "回复的能力位必须是内核授予的那一组");
        assert.notEqual(capBits, 0, "能力位不能是空的（空 = 前端按位降级）");
    });

    test("Negotiate 在宿主要的版本比内核新时被拒（E_PROTO-01）", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame({
            Negotiate: { min_schema_version: SCHEMA_VERSION + 1, cap_bits: 0 },
        }));
        const err = frames.find((f) => f.payload.payload.Event.Error);
        assert.ok(err, "版本不兼容必须被拒");
        assert.equal(err.payload.payload.Event.Error.code, "E_PROTO-01");
    });

    test("mock 内核的帧带 crc，且能被 decodeFrame 收下", () => {
        const kernel = createMockKernel();
        const frames = kernel.handle(commandFrame("Nop"));
        assert.ok(frames.length > 0);
        for (const frame of frames) {
            assert.match(frame.crc, /^[0-9a-f]{8}$/);
            // 帧的两端都在 JS 里 —— 同一个 decodeFrame 必须收下它。
            assert.doesNotThrow(() => decodeFrame(JSON.parse(JSON.stringify(frame))));
        }
    });
});

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/** A transport double with the M-07 surface the router uses. */
function fakeTransport() {
    /** @type {Set<Function>} */
    const messageHandlers = new Set();
    /** @type {Set<Function>} */
    const statusHandlers = new Set();
    const transport = {
        sent: [],
        connected: true,
        onMessage(handler) {
            messageHandlers.add(handler);
            return () => messageHandlers.delete(handler);
        },
        onStatus(handler) {
            statusHandlers.add(handler);
            return () => statusHandlers.delete(handler);
        },
        sendCommand(message, options = {}) {
            transport.sent.push({ message, reqId: options.reqId ?? null });
        },
        getTransportStatus() {
            return { mode: "worker", connected: transport.connected, connState: "ready" };
        },
        emit(frame) {
            for (const fn of [...messageHandlers]) fn(frame);
        },
    };
    return transport;
}

/**
 * A transport double backed by the **real** mock kernel.
 *
 * The kernel's frames go back through `decodeFrame()` — so the crc is
 * validated on the way back too, and the whole M-05 correlation path
 * (request → kernel → reply → pending command) runs for real. A command
 * the kernel does **not** answer therefore really does time out here,
 * which is exactly what the "empty range must still be answered" case
 * has to distinguish.
 */
function mockKernelTransport() {
    const kernel = createMockKernel();
    /** @type {Set<Function>} */
    const messageHandlers = new Set();
    return {
        sent: [],
        onMessage(handler) {
            messageHandlers.add(handler);
            return () => messageHandlers.delete(handler);
        },
        onStatus() {
            return () => {};
        },
        sendCommand(message, options = {}) {
            const frames = kernel.handle({
                type: "command",
                seq: 1,
                sessionId: null,
                role: null,
                reqId: options.reqId ?? null,
                payload: message,
            });
            for (const raw of frames) {
                const frame = decodeFrame(raw);
                for (const fn of [...messageHandlers]) fn(frame);
            }
        },
        getTransportStatus() {
            return { mode: "worker", connected: true, connState: "ready" };
        },
    };
}

/** An inbound event frame. */
function eventFrame(reqId, payload, seq = 1) {
    return {
        type: "event",
        seq,
        sessionId: "sess-1",
        role: "embedded",
        reqId,
        payload: eventMessage(seq, payload),
    };
}

/** A command frame carrying one `CommandPayload` value. */
function commandFrame(payload, reqId = 1) {
    return {
        type: "command",
        seq: 1,
        sessionId: null,
        role: null,
        reqId,
        payload: commandMessage(1, payload),
    };
}
