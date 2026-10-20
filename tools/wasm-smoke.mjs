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
        schema_version: 1,
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

console.log("WASM 桥冒烟测试");
console.log(`  kernel_version          = ${mod.kernel_version()}`);
console.log(`  protocol_schema_version = ${mod.protocol_schema_version()}`);
console.log(`  ping                    = ${mod.ping("hello")}`);

check("ping 回声正确", mod.ping("hello") === "pong:hello");
check("schema 版本是 1", mod.protocol_schema_version() === 1);

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
    const wave = find(events, "WaveformDelta");
    check("Subscribe 推全量寄存器快照", wave !== undefined);
    const streams = wave?.WaveformDelta?.streams ?? [];
    check("全量快照 = 32 条 GPR + pc", streams.length === 33, `实际 ${streams.length}`);
}

// -- StepN ----------------------------------------------------------------
{
    const events = send({ StepN: { n: 4 } });
    const wave = find(events, "WaveformDelta");
    const streams = wave?.WaveformDelta?.streams ?? [];
    const value = (id) => {
        const s = streams.find((x) => x.id === id);
        return s ? s.edges[s.edges.length - 1].value : undefined;
    };
    check("StepN 后 x1 = 42", value("cpu.gpr.x1") === 42, `实际 ${value("cpu.gpr.x1")}`);
    check("StepN 后 x2 = 7", value("cpu.gpr.x2") === 7, `实际 ${value("cpu.gpr.x2")}`);
    check("StepN 后 x3 = 49", value("cpu.gpr.x3") === 49, `实际 ${value("cpu.gpr.x3")}`);
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
