// STAGE-43 补丁批 —— 器件文档默认表的**偏移播种**门禁。
//
// 不变式：
//   ① 器件实例详情表的默认**偏移**以内核类型表（`device.types`）为准 ⇒ ⇒
//      前端内置表只提供人读的名字 / 含义 / 位宽说明；
//   ② 内核类型表还没推过来时，退回内置表（订阅前界面不空）；
//   ③ 文档独有的半字拆分行（如 CLINT `MTIMECMP0_HI`）保留文档偏移，不被
//      基础寄存器覆盖；内核独有的寄存器追加一行。
//
// ⚠️ 断言值是写死的字面量（对照 `crates/kernel/src/m15_peripheral/uart.rs`
// 的寄存器常量），不从被测实现反推。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const docs = await importFront("views/components/device-docs.js");
const workspace = await importFront("views/components/product-workspace.js");

/** 内核类型表 fixture —— 对照 `uart.rs:178-207` 的字节偏移 0..7。 */
const UART_TYPES = Object.freeze([
    {
        kind: "uart16550",
        window_bytes: 8,
        registers: [
            { name: "THR", offset: 0, width_bits: 32 },
            { name: "IER", offset: 1, width_bits: 32 },
            { name: "FCR", offset: 2, width_bits: 32 },
            { name: "LCR", offset: 3, width_bits: 32 },
            { name: "MCR", offset: 4, width_bits: 32 },
            { name: "LSR", offset: 5, width_bits: 32 },
            { name: "MSR", offset: 6, width_bits: 32 },
            { name: "SCR", offset: 7, width_bits: 32 },
        ],
    },
]);

/** 内核 CLINT 类型表 fixture（半字拆分只在文档层）。 */
const CLINT_TYPES = Object.freeze([
    {
        kind: "clint",
        window_bytes: 0x10000,
        registers: [
            { name: "MSIP", offset: 0, width_bits: 32 },
            { name: "MTIMECMP", offset: 0x4000, width_bits: 64 },
            { name: "MTIME", offset: 0xbff8, width_bits: 64 },
        ],
    },
]);

describe("器件文档 · 默认偏移由内核类型表播种", () => {
    test("UART：偏移覆盖成内核字节偏移 0..7，名字 / 含义保留文档说明", () => {
        const doc = docs.seedRegisterDoc("uart16550", UART_TYPES);
        const offsets = doc.regs.map((r) => r.offset);
        assert.deepEqual(offsets, [
            "0x000", "0x001", "0x002", "0x003",
            "0x004", "0x005", "0x006", "0x007",
        ]);
        // 文档友好名保留：RBR / THR；IIR / FCR。
        assert.equal(doc.regs[0].name, "RBR / THR");
        assert.equal(doc.regs[2].name, "IIR / FCR");
        assert.match(doc.regs[2].meaning, /FIFO/);
        // 位宽仍取文档层（内核 width_bits 对字节寄存器不可信）。
        assert.equal(doc.regs[0].width, 1);
    });

    test("内核类型表未到达（null）：退回内置表，回退偏移也是 0..7", () => {
        const doc = docs.seedRegisterDoc("uart16550", null);
        assert.deepEqual(
            doc.regs.map((r) => r.offset),
            ["0x000", "0x001", "0x002", "0x003", "0x004", "0x005", "0x006", "0x007"]
        );
    });

    test("CLINT：半字拆分行 `MTIMECMP0_HI` 保留文档偏移 0x4004", () => {
        const doc = docs.seedRegisterDoc("clint", CLINT_TYPES);
        const byName = Object.fromEntries(doc.regs.map((r) => [r.name, r.offset]));
        const off = (name) => parseInt(byName[name], 16);
        assert.equal(off("MSIP0"), 0x0);
        assert.equal(off("MTIMECMP0"), 0x4000);
        assert.equal(off("MTIMECMP0_HI"), 0x4004);
        assert.equal(off("MTIME"), 0xbff8);
    });

    test("内核独有的寄存器：追加一行，含义留空", () => {
        const types = [
            {
                kind: "gpio",
                window_bytes: 0x1c,
                registers: [
                    { name: "DIR", offset: 0, width_bits: 32 },
                    { name: "EXTRA", offset: 0x1c, width_bits: 32 },
                ],
            },
        ];
        const doc = docs.seedRegisterDoc("gpio", types);
        const extra = doc.regs.find((r) => r.name === "EXTRA");
        assert.ok(extra, "内核独有寄存器没有追加");
        assert.equal(extra.offset, "0x01c");
        assert.equal(extra.meaning, "");
    });

    test("product 视图 docFor：从 state.deviceTypes 播种默认偏移", () => {
        const state = { deviceTypes: UART_TYPES, docs: {} };
        const node = { key: "r1", deviceKind: "uart16550" };
        const doc = workspace.docFor(state, node);
        assert.deepEqual(
            doc.regs.map((r) => r.offset),
            ["0x000", "0x001", "0x002", "0x003", "0x004", "0x005", "0x006", "0x007"]
        );
    });

    test("内置 UART 回退表本身 == 内核字节偏移常量（0..7）", () => {
        const builtin = docs.DEVICE_DOCS.uart16550.regs;
        assert.deepEqual(
            builtin.map((r) => r.offset),
            ["0x000", "0x001", "0x002", "0x003", "0x004", "0x005", "0x006", "0x007"]
        );
    });
});
