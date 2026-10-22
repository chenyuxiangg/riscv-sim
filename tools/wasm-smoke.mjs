#!/usr/bin/env node
// WASM 桥冒烟测试（第 1 阶段）。
//
// 在 **Node** 里直接驱动 `front/assets/pkg/riscv_sim_kernel.js`——也就是
// 浏览器 Worker 加载的那**同一个**产物——把 `process_message` 这条桥从头
// 走一遍：
//
//   Nop → LoadProject → Subscribe → StepN → ReadMemory → SetBreakpoint
//       → Continue → 畸形输入
//
// 它验的是**桥的形状**（契约消息进 / 契约事件数组出）与**真实执行**
// （demo 固件真的跑起来了、寄存器值对、断点真的命中）。
//
// ## 前置
//
// 先编 WASM：
//
//     tools/build-wasm.sh            # 或 --release
//
// 产物不在（`front/assets/pkg/` 被 gitignore）时脚本**跳过**并退出 127
// （与 `tools/browser-verify.mjs` 同一口径：127 = 前置条件缺失，不是
// 「验证通过」——CI 只看退出码时，「没跑」绝不能被当成「过了」）。
//
// ## 用法
//
//     node tools/wasm-smoke.mjs

import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { DEMO_FIRMWARE_BYTES } from "../front/dev/demo-firmware.js";
import { SCHEMA_VERSION } from "../front/lib/message.js";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");
const pkgDir = resolve(repoRoot, "front/assets/pkg");
const glue = resolve(pkgDir, "riscv_sim_kernel.js");
const wasm = resolve(pkgDir, "riscv_sim_kernel_bg.wasm");

if (!existsSync(glue) || !existsSync(wasm)) {
    console.error([
        "SKIP: WASM 产物不存在（先跑 tools/build-wasm.sh）",
        `  期望：${glue}`,
        `  期望：${wasm}`,
    ].join("\n"));
    process.exit(127);
}

/**
 * demo 固件（40 字节，RV32I 手写编码，小端）。
 *
 * ⚠️ **直接 import 前端那份**，不复制常量：两份字节一旦漂移，这个脚本
 * 就会绿着测一个前端根本没发的程序，而浏览器复验走的是前端那份——
 * 两个「真机证据」指向不同的固件。（V-B 检视发现 · 中危 2）
 */
const DEMO_BYTES = DEMO_FIRMWARE_BYTES;

/** demo 固件的断点靶点。 */
const DEMO_BREAKPOINT_ADDR = 0x20;

/** demo 固件写进内存的目标地址。 */
const DEMO_STORE_ADDR = 0x100;

const mod = await import(pathToFileURL(glue).href);
// Node 的 `fetch` 不支持 `file://`，所以把 wasm 字节直接交给 init
// （浏览器里 `init()` 无参即可——它按 `import.meta.url` 解析）。
// 新版 wasm-bindgen 的 init 收一个对象参数，位置参数已废弃。
await mod.default({ module_or_path: readFileSync(wasm) });

/** 内核自己报的协议版本 —— 发命令时用它（见 `send()` 的注释）。 */
const kernelSchemaVersion = mod.protocol_schema_version();

/**
 * 契约层（Rust）的 `SCHEMA_VERSION` —— **直接读源码**。
 *
 * ⚠️ **为什么不能在本脚本里再抄一份**：v0.20 之前，这里硬编码着 `2`。
 * 契约升到 v3 时，那条断言**静默失配**（门禁红着，却没人发现它早该改）⇒
 * 门禁脚本里的「协议版本」必须是**推导出来的**，不是**抄下来的**。
 *
 * ⚠️ 解析失败 ⇒ 直接抛错：读不到版本号时**不能**当成「跳过」——
 * 那会让这条门禁**悄悄消失**。
 */
function rustSchemaVersion() {
    const src = readFileSync(resolve(repoRoot, "crates/contract/src/lib.rs"), "utf8");
    const m = src.match(/pub const SCHEMA_VERSION:\s*u32\s*=\s*(\d+)/);
    if (!m) {
        throw new Error("读不到契约层的 SCHEMA_VERSION（crates/contract/src/lib.rs）");
    }
    return Number(m[1]);
}

/** 契约层（Rust）的协议版本。 */
const contractSchemaVersion = rustSchemaVersion();

let failures = 0;

/**
 * 断言。
 *
 * @param {string} what
 * @param {boolean} ok
 * @param {string} [detail]
 */
function check(what, ok, detail = "") {
    if (ok) {
        console.log(`  ok   ${what}`);
    } else {
        failures += 1;
        console.error(`  FAIL ${what}${detail ? ` — ${detail}` : ""}`);
    }
}

/**
 * 发一条契约命令，返回内核给的事件数组。
 *
 * @param {object|string} payload
 * @returns {object[]}
 */
function send(payload) {
    const message = {
        // 用**内核自己报的**版本发命令：契约规定「低于支持版本 → 拒」，
        // 所以发一个内核不认的版本会让每个断言都变成「命令被拒」，
        // 掩盖真正的被测行为。版本**对不对**由下面那条显式断言管。
        schema_version: kernelSchemaVersion,
        direction: "Command",
        seq: 1,
        payload: { Command: payload },
    };
    const out = mod.process_message(JSON.stringify(message));
    const events = JSON.parse(out);
    if (!Array.isArray(events) || events.length === 0) {
        throw new Error(`内核没有回事件：${out}`);
    }
    for (const e of events) {
        if (e.direction !== "Event") throw new Error("出参方向不是 Event");
        if (typeof e.seq !== "number") throw new Error("出参缺少 seq");
    }
    return events;
}

/**
 * 在事件数组里找第一个变体。
 *
 * @param {object[]} events
 * @param {string} variant
 * @returns {object|undefined}
 */
function find(events, variant) {
    return events.map((e) => e.payload.Event).find((p) => variant in p);
}

/**
 * 在事件数组里找一条**探针**波形流。
 *
 * ⚠️ 不能直接用 `find(events, "WaveformHistory")`：一次运行会推**两条**
 * 波形事件——寄存器增量 + 探针边沿——`find` 只会拿到第一条（寄存器那条）。
 * 所以这里按流 id 找。
 *
 * @param {object[]} events
 * @param {string} id — e.g. `"GPIO0.OUT"`
 * @returns {{id: string, label: string, edges: Array<{t_ps: string, value: number}>}|undefined}
 */
function findProbeStream(events, id) {
    for (const e of events) {
        const payload = e.payload.Event;
        if (!payload || !("WaveformHistory" in payload)) continue;
        const streams = payload.WaveformHistory.streams ?? [];
        const hit = streams.find((s) => s.id === id);
        if (hit) return hit;
    }
    return undefined;
}

/** 探针通道 id —— M-18 的 `"{外设 id}.{信号名}"`。 */
const GPIO_STREAM_ID = "GPIO0.OUT";

console.log("WASM 桥冒烟测试");
console.log(`  kernel_version          = ${mod.kernel_version()}`);
console.log(`  protocol_schema_version = ${mod.protocol_schema_version()}`);
console.log(`  ping                    = ${mod.ping("hello")}`);

check("ping 回声正确", mod.ping("hello") === "pong:hello");
check(
    "契约层（Rust）的 SCHEMA_VERSION 与前端 JS 镜像一致",
    contractSchemaVersion === SCHEMA_VERSION,
    `Rust ${contractSchemaVersion} vs JS ${SCHEMA_VERSION}`,
);
check(
    "内核的协议版本与契约层一致",
    kernelSchemaVersion === contractSchemaVersion,
    `内核 ${kernelSchemaVersion} vs 契约 ${contractSchemaVersion}`,
);

// -- Nop ------------------------------------------------------------------
{
    const events = send("Nop");
    check("Nop 回一个 Tick", find(events, "Tick") !== undefined);
}

// -- LoadProject ----------------------------------------------------------
{
    const project = {
        id: "proj-smoke",
        name: "wasm smoke",
        tree: { root: { name: "MainBoard", component_id: null, children: [] } },
        images: [
            { name: "demo.bin", kind: "RawHex", source: { Bytes: DEMO_BYTES } },
        ],
        version: { schema: 1, isa: "RV32IMC", toolchain: "wasm-smoke" },
    };
    const events = send({ LoadProject: project });
    check("LoadProject 回一个 Tick", find(events, "Tick") !== undefined);
}

// -- Subscribe ------------------------------------------------------------
{
    const events = send({ Subscribe: { topic: "sim.registers" } });
    const wave = find(events, "WaveformHistory");
    check("Subscribe 推全量寄存器快照", wave !== undefined);
    const streams = wave?.WaveformHistory?.streams ?? [];
    check("全量快照 = 32 条 GPR + pc", streams.length === 33, `实际 ${streams.length}`);
}

// -- StepN ----------------------------------------------------------------
{
    const events = send({ StepN: { n: 4 } });
    const wave = find(events, "WaveformHistory");
    const streams = wave?.WaveformHistory?.streams ?? [];
    const value = (id) => {
        const s = streams.find((x) => x.id === id);
        return s ? s.edges[s.edges.length - 1].value : undefined;
    };
    check("StepN 后 x1 = 42", value("cpu.gpr.x1") === 42, `实际 ${value("cpu.gpr.x1")}`);
    check("StepN 后 x2 = 7", value("cpu.gpr.x2") === 7, `实际 ${value("cpu.gpr.x2")}`);
    check("StepN 后 x3 = 49", value("cpu.gpr.x3") === 49, `实际 ${value("cpu.gpr.x3")}`);
}

// -- 契约 v2 的新命令（真 wasm 产物）-------------------------------------
//
// 这一段验的是**契约 v2 新增的三条命令**在**真内核**上的行为：
// 协商、读寄存器、写寄存器。规格 §5 的语义逐条落地：
//
//   - `Negotiate`：宿主要求的**最低**版本高于内核支持的 → 拒（E_PROTO-01）；
//   - `ReadRegisters`：读是**钳位**，回复带**实际起点**；
//   - `WriteRegister`：写 x0 被**丢弃**但**仍回复**；写越界是**拒绝**
//     （E_DBG-01）而不是钳位——写钳位会静默写到别的寄存器。
{
    // 协商：正常路径
    // ⚠️ 宿主要求的**最低**版本 = 当前契约版本 ⇒ 内核的回包应当正好是它
    //    （版本是**推导出来的**，不是抄在这个脚本里的常量）。
    const neg = find(
        send({ Negotiate: { min_schema_version: SCHEMA_VERSION, cap_bits: 0 } }),
        "Negotiated",
    );
    check("Negotiate 回一个 Negotiated 事件", neg !== undefined);
    check(
        "协商的协议版本 = 当前契约版本",
        neg?.Negotiated?.schema_version === SCHEMA_VERSION,
        `实际 ${neg?.Negotiated?.schema_version}`,
    );
    check(
        "协商回了内核描述串（版本 + ISA）",
        typeof neg?.Negotiated?.kernel === "string" && /RV32IMC/.test(neg.Negotiated.kernel),
        `实际 ${JSON.stringify(neg?.Negotiated?.kernel)}`,
    );

    // 协商：宿主要求的版本高于内核 → 拒
    const negBad = send({ Negotiate: { min_schema_version: 99, cap_bits: 0 } });
    const negErr = find(negBad, "Error");
    check(
        "Negotiate 版本太高被拒（E_PROTO-01）",
        negErr?.Error?.code === "E_PROTO-01",
        `实际 ${negErr?.Error?.code}`,
    );

    // 读寄存器：钳位 + 回报实际起点
    const read = find(send({ ReadRegisters: { index: 1, count: 3 } }), "Registers");
    check("ReadRegisters 回一个 Registers 事件", read !== undefined);
    check(
        "读到的 x1/x2/x3 是固件真的算出来的值",
        JSON.stringify(read?.Registers?.values) === JSON.stringify([42, 7, 49]),
        `实际 ${JSON.stringify(read?.Registers?.values)}`,
    );

    // 读寄存器：区间越过寄存器表末尾 → 钳到末尾
    const clamped = find(send({ ReadRegisters: { index: 30, count: 10 } }), "Registers");
    check(
        "读的区间越界时钳到寄存器表末尾",
        clamped?.Registers?.index === 30 && clamped?.Registers?.values?.length === 2,
        `实际 index=${clamped?.Registers?.index} len=${clamped?.Registers?.values?.length}`,
    );

    // 读寄存器：count = 0 → **回复空区间**（不是不回复——不回复与消息
    // 丢失不可区分，M-05 会超时拒绝）
    const empty = find(send({ ReadRegisters: { index: 0, count: 0 } }), "Registers");
    check(
        "count = 0 回复空区间（而不是不回复）",
        empty !== undefined && Array.isArray(empty?.Registers?.values) &&
            empty.Registers.values.length === 0,
        `实际 ${JSON.stringify(empty?.Registers)}`,
    );

    // 写寄存器：写 x0 被丢弃但仍回复
    const w0 = find(send({ WriteRegister: { index: 0, value: 0xDEAD } }), "Registers");
    check(
        "写 x0 被丢弃但仍回复（值恒为 0）",
        JSON.stringify(w0?.Registers?.values) === JSON.stringify([0]),
        `实际 ${JSON.stringify(w0?.Registers?.values)}`,
    );

    // 写寄存器：越界是**拒绝**（E_DBG-01），不是钳位
    const wBad = find(send({ WriteRegister: { index: 99, value: 1 } }), "Error");
    check(
        "写越界被拒（E_DBG-01），不是钳位",
        wBad?.Error?.code === "E_DBG-01",
        `实际 ${wBad?.Error?.code}`,
    );

    // 写寄存器：正常写要真的写进去（能被读回来）
    //
    // ⚠️ 写的是 x5，而 demo 固件在 0x1c 用 x5 作 GPIO 基址
    // （`sw x6, 4(x5)`）。**这不影响后面的探针断言**：固件在 0x14
    // 有 `lui x5, 0x40000`，探针段的 `StepN(4)` 正好执行 0x10→0x14→
    // 0x18→0x1c，x5 在那之前就被**重新设回** 0x4000_0000 了。
    send({ WriteRegister: { index: 5, value: 0x1234 } });
    const back = find(send({ ReadRegisters: { index: 5, count: 1 } }), "Registers");
    check(
        "写进 x5 的值能被读回来",
        back?.Registers?.values?.[0] === 0x1234,
        `实际 ${JSON.stringify(back?.Registers?.values)}`,
    );
}

// -- 探针流（D-46）--------------------------------------------------------
//
// 波形管道是**真的**接上的：demo 固件写 GPIO0.OUT → M-15 报边沿 →
// M-18 判沿 → 宿主推成契约的 `WaveformHistory`。
//
// 这里特意**不用** mock：跑的是 `front/assets/pkg/` 里那个真 wasm 产物。
{
    // 订阅探针流：内核会**强制**推一次通道列表（哪怕还没有边沿）。
    const subEvents = send({ Subscribe: { topic: "sim.waveform" } });
    const stream = findProbeStream(subEvents, GPIO_STREAM_ID);
    check("订阅探针流后列出了 GPIO0.OUT 通道", stream !== undefined);
    check(
        "刚订阅时通道还没有边沿",
        (stream?.edges ?? []).length === 0,
        `实际 ${JSON.stringify(stream?.edges)}`,
    );

    // demo 固件的第 8 条指令写 GPIO0.OUT —— 跑到那里就该有边沿了。
    // （前 4 条指令上面已经跑过，这里再跑 4 条。）
    const runEvents = send({ StepN: { n: 4 } });
    const runStream = findProbeStream(runEvents, GPIO_STREAM_ID);
    check(
        "固件写 GPIO 后探针流带来一条边沿",
        (runStream?.edges ?? []).length === 1,
        `实际 ${JSON.stringify(runStream?.edges)}`,
    );
    check(
        "边沿的值是固件写进去的引脚位图（1）",
        runStream?.edges?.[0]?.value === 1,
        `实际 ${runStream?.edges?.[0]?.value}`,
    );
    check(
        "边沿带的是**真实**的虚拟时间戳（不是 0）",
        // 契约 v2：虚拟时间在线上是**十进制字符串**（§2.2）——u64 走
        // JSON 数字会落到 JS double，只精确到 2^53。所以这里既要看它
        // 是字符串形态，也要看它折算出来确实大于 0。
        typeof runStream?.edges?.[0]?.t_ps === "string" &&
            /^\d+$/.test(runStream.edges[0].t_ps) &&
            Number(runStream.edges[0].t_ps) > 0,
        `实际 ${JSON.stringify(runStream?.edges?.[0]?.t_ps)}`,
    );
}

// -- ReadMemory -----------------------------------------------------------
{
    const events = send({ ReadMemory: { addr: DEMO_STORE_ADDR, size: "Word" } });
    const read = find(events, "MemoryRead");
    check(
        "ReadMemory 读回 sw 写的值 49",
        read?.MemoryRead?.value === 49,
        `实际 ${JSON.stringify(read?.MemoryRead)}`,
    );
}

// -- 契约 v3 的调试面（M-11 · STAGE-5）-------------------------------------
//
// ⚠️ 这四条命令在**真 wasm 产物**上跑 ⇒ 证明「接缝被接上」，而不只是
//    「Rust 侧的单元能过」。
//    （浏览器里的生产路径就是这条：前端 → Worker → wasm → 内核。）
{
    // ① `Disasm` ⇒ 一条 `Disasm` 事件，条目带**真实**的助记符。
    const disasm = find(send({ Disasm: { addr: 0, count: 3 } }), "Disasm");
    const entries = disasm?.Disasm?.entries ?? [];
    check("Disasm 回一个 Disasm 事件", disasm !== undefined);
    check("Disasm 给出三条指令", entries.length === 3, `实际 ${entries.length}`);
    check(
        "Disasm 的第一条是 demo 固件的第一条指令（addi ra,zero,42）",
        entries[0]?.mnemonic === "addi ra,zero,42",
        `实际 ${JSON.stringify(entries[0])}`,
    );
    check(
        "Disasm 的机器码是固件里的真机器码",
        entries[0]?.code === 0x02a00093,
        `实际 ${JSON.stringify(entries[0]?.code)}`,
    );

    // ② `ResolveLine` ⇒ 一条 `SourceLocation` 事件（Wave 1 = 降级态）。
    const loc = find(send({ ResolveLine: { addr: 0 } }), "SourceLocation");
    check("ResolveLine 回一个 SourceLocation 事件", loc !== undefined);
    check(
        "ResolveLine 在无 DWARF 时给降级态（行号 0）",
        loc?.SourceLocation?.line === 0 && loc?.SourceLocation?.file === "",
        `实际 ${JSON.stringify(loc?.SourceLocation)}`,
    );

    // ③ `SetTraceEnabled` ⇒ 一条 `TraceState` 事件。
    const state = find(send({ SetTraceEnabled: { enabled: true } }), "TraceState");
    check("SetTraceEnabled 回一个 TraceState 事件", state !== undefined);
    check(
        "SetTraceEnabled 回显开关状态",
        state?.TraceState?.enabled === true,
        `实际 ${JSON.stringify(state?.TraceState)}`,
    );

    // ④ `ExportTrace`（执行轨迹 / CSV）⇒ 一条 `TraceExport` 事件。
    const exported = find(
        send({ ExportTrace: { source: "Execution", format: "Csv", offset: 0, limit: 0 } }),
        "TraceExport",
    );
    const chunk = exported?.TraceExport?.chunk ?? "";
    check("ExportTrace 回一个 TraceExport 事件", exported !== undefined);
    check(
        "执行轨迹导出带 CSV 表头",
        chunk.startsWith("time_ps,addr,kind,data,width"),
        `实际 ${JSON.stringify(chunk.slice(0, 80))}`,
    );
    check(
        "执行轨迹里能看到指令条目（kind = instruction）",
        chunk.includes(",instruction,"),
        `实际 ${JSON.stringify(chunk.slice(0, 200))}`,
    );

    // ⚠️ **时间轴必须真的动**：时间列**全是 0** ⇒ 导出看着有数据，却读不出
    //    「什么时候发生的」（M-14 的 `Bus::set_now_ps` 没接线的症状）。
    const stamps = chunk
        .split("\n")
        .slice(1)
        .filter((l) => l.trim().length > 0)
        .map((l) => Number(l.split(",")[0].replace(/"/g, "")));
    check("执行轨迹导出行数不为 0", stamps.length > 0, `实际 ${stamps.length}`);
    check(
        "时间戳不是恒 0（时间轴是活的）",
        stamps.some((t) => t > 0),
        `实际 ${JSON.stringify(stamps)}`,
    );

    // ⑤ 非法组合被**拒绝**（不「换一个格式凑合」）。
    const bad = send({ ExportTrace: { source: "Execution", format: "Vcd", offset: 0, limit: 0 } });
    const err = find(bad, "Error");
    check(
        "执行轨迹 + VCD 被拒（E_CTX-03）",
        err?.Error?.code === "E_CTX-03",
        `实际 ${JSON.stringify(err?.Error)}`,
    );
}

// -- 断点 -----------------------------------------------------------------
{
    send({ SetBreakpoint: { id: 1, addr: DEMO_BREAKPOINT_ADDR, source_hint: null, enabled: true } });
    const events = send("Continue");
    const halted = find(events, "Halted");
    check("Continue 命中断点", halted !== undefined);
    check(
        "断点原因 = Breakpoint",
        halted?.Halted?.reason === "Breakpoint",
        `实际 ${halted?.Halted?.reason}`,
    );
    check(
        "断点命中时 PC 停在断点地址",
        halted?.Halted?.pc === DEMO_BREAKPOINT_ADDR,
        `实际 ${halted?.Halted?.pc}`,
    );
}


// -- 畸形输入 -------------------------------------------------------------
{
    const out = JSON.parse(mod.process_message("{not json"));
    check("畸形输入回一个 Error 事件", out.length === 1);
    // 不只看数组长度：内核若回一个 `direction:"Command"` 的消息，长度
    // 仍然是 1，这条断言会绿。方向与 seq 都要断言。（V-B 检视 · 低危 2）
    check(
        "畸形输入回的是 Event 方向",
        out[0]?.direction === "Event",
        `实际 ${out[0]?.direction}`,
    );
    check(
        "畸形输入回的事件带合法 seq",
        typeof out[0]?.seq === "number" && Number.isFinite(out[0].seq),
        `实际 ${out[0]?.seq}`,
    );
    const err = out[0]?.payload?.Event?.Error;
    check("错误码是 E_PROTO-08", err?.code === "E_PROTO-08", `实际 ${err?.code}`);
}

if (failures > 0) {
    console.error(`\nFAIL: ${failures} 项不通过`);
    process.exit(1);
}
console.log("\nPASS: WASM 桥冒烟测试全过");
