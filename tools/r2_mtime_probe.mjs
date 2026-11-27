#!/usr/bin/env node
// R-2 复现：wasm 后端 CLINT `mtime` 的 64 位读是否单调。
//
// 完全按 guest 的读法来（`hal_clint_timer_get_time_stamp` 反汇编出来的是
// `lui a5,0x200c; lw a6,-8(a5); lw a7,-4(a5)` 两条 32 位 load）：
//
//   1. 每条 guest 指令 = 一次 `StepN{1}`（推进虚拟时钟 1000 ps = 1 ns，
//      10 MHz CLINT ⇒ 0.01 cycle/条）；
//   2. 读两次 CLINT：低半字 `0x200BFF8`、高半字 `0x200BFFC`；
//   3. 拼成 64 位，检查「单调不减」且「相邻差 < 1000 cycles」。
//
// 任何一次不满足就打印**原始两个半字**（这是判因的关键：是「半字错位」还是
// 「时钟跳变」），退出码 1。
//
// 用法（验收就是 100 万次）：
//     node tools/r2_mtime_probe.mjs 1000000
// 前置：`front/assets/pkg/` 里有 wasm 产物（`tools/build-wasm.sh`），否则 127。

import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");
const pkgGlue = resolve(repoRoot, "front/assets/pkg/riscv_sim_kernel.js");
const pkgWasm = resolve(repoRoot, "front/assets/pkg/riscv_sim_kernel_bg.wasm");
if (!existsSync(pkgGlue) || !existsSync(pkgWasm)) {
    console.error("wasm 产物缺失：先跑 tools/build-wasm.sh");
    process.exit(127);
}

const MTIME_LO = 0x200bff8;
const MTIME_HI = 0x200bffc;
const SAMPLES = Number(process.argv[2] ?? 20000);

const mod = await import(pathToFileURL(pkgGlue).href);
// Node 的 fetch 不认 file:// ⇒ 直接交字节（新版 wasm-bindgen 收对象参数）
await mod.default({ module_or_path: readFileSync(pkgWasm) });

let schema = 20;
try {
    schema = mod.protocol_schema_version();
} catch {
    // 老产物没有这个导出：用契约层当前版本兜底（R-2 与版本无关）
}
let seq = 1;

function send(payload) {
    const message = {
        schema_version: schema,
        direction: "Command",
        seq: seq++,
        payload: { Command: payload },
    };
    const out = mod.process_message(JSON.stringify(message));
    const events = JSON.parse(out);
    const errors = events
        .map((e) => e.payload?.Event?.Error)
        .filter(Boolean);
    return { events, errors };
}

function find(events, variant) {
    return events.map((e) => e.payload?.Event).find((p) => p && variant in p);
}

// -- 配板 + 装载自跳固件（4 字节 `jal x0, 0`）--------------------------------
{
    const listed = send("ListPlatformTemplates");
    const templates = find(listed.events, "PlatformTemplates")?.PlatformTemplates?.templates;
    if (!templates?.length) {
        console.error("拿不到模板库：", JSON.stringify(listed.events).slice(0, 400));
        process.exit(1);
    }
    // qemu-virt 优先（CLINT 语义与 OSZ 板上一致），拿不到就退 default
    const id = templates.some((t) => t.id === "qemu-virt") ? "qemu-virt" : templates[0].id;
    const got = send({ GetPlatformTemplate: { id } });
    const description = find(got.events, "PlatformTemplate")?.PlatformTemplate?.description;
    const mapped = send({ SetMemoryMap: { description, expected_revision: null } });
    if (!find(mapped.events, "MemoryMap")) {
        console.error("配板失败：", JSON.stringify(mapped.events).slice(0, 400));
        process.exit(1);
    }
    console.log(`板：${id}`);
}

{
    const spin = [0x6f, 0x00, 0x00, 0x00]; // jal x0, 0
    const project = {
        id: "proj-r2",
        name: "r2 mtime probe",
        tree: { root: { name: "MainBoard", component_id: null, children: [] } },
        images: [{ name: "spin.bin", kind: "RawHex", source: { Bytes: spin } }],
        version: { schema: 1, isa: "RV32IMC", toolchain: "r2-probe" },
    };
    const loaded = send({ LoadProject: project });
    if (loaded.errors.length) {
        console.error("装载失败：", JSON.stringify(loaded.errors));
        process.exit(1);
    }
}

function readWord(addr) {
    const out = send({ ReadMemory: { addr, size: "Word" } });
    const mem = find(out.events, "MemoryRead")?.MemoryRead;
    if (!mem) {
        const why = out.errors[0];
        throw new Error(`ReadMemory(0x${addr.toString(16)}) 没有回体：${JSON.stringify(why ?? out.events).slice(0, 300)}`);
    }
    return mem.value >>> 0;
}

// -- 百万次 64 位读 ---------------------------------------------------------
let prev = null;
let min = null;
let max = null;
let maxJump = 0;
const anomalies = [];

for (let i = 0; i < SAMPLES; i += 1) {
    send({ StepN: { n: 1 } });
    const lo = readWord(MTIME_LO);
    send({ StepN: { n: 1 } });
    const hi = readWord(MTIME_HI);
    const value = (BigInt(hi) << 32n) | BigInt(lo);

    if (prev !== null) {
        const jump = value - prev;
        if (jump < 0n || jump > 1000n) {
            if (anomalies.length < 5) {
                anomalies.push({ i, prev: prev.toString(), value: value.toString(), lo, hi });
            }
        } else if (jump > maxJump) {
            maxJump = Number(jump);
        }
    }
    if (min === null || value < min) min = value;
    if (max === null || value > max) max = value;
    prev = value;
}

console.log(`采样 ${SAMPLES} 次 64 位读（间隔 2 条指令）`);
console.log(`  首值 ${min} / 末值 ${max} / 最大单步差 ${maxJump} cycles`);
if (anomalies.length) {
    console.error(`  异常 ${anomalies.length} 处（最多列 5 处）：`);
    for (const a of anomalies) {
        console.error(`   #${a.i}: ${a.prev} → ${a.value}（lo=0x${a.lo.toString(16)} hi=0x${a.hi.toString(16)}）`);
    }
    process.exit(1);
}
console.log("  ✅ 单调不减、单步差 < 1000 cycles");
