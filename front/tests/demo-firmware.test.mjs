// Gate for the demo firmware + the project that carries it.
//
// The firmware is **hand-assembled RV32I** (no toolchain involved), so
// the only thing that can keep it honest is an assertion on the exact
// bytes: a silent edit of a single byte would change what the real
// kernel executes, and nothing else in the front end would notice.

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const {
    DEMO_FIRMWARE_BYTES,
    DEMO_FIRMWARE_BREAKPOINT_ADDR,
    DEMO_FIRMWARE_GPIO_BASE,
    DEMO_FIRMWARE_GPIO_OUT_OFFSET,
} = await importFront("dev/demo-firmware.js");
const { buildMockProject } = await importFront("dev/mock-project.js");

/**
 * The byte list the demo firmware is frozen at (byte for byte).
 *
 * ⚠️ 2026-10-21（D-46）：0x10..0x1c 从「跳到 0x20 + 三条 nop」改成
 * 「跳到 0x14 + 三条 GPIO 设置指令」——demo 固件必须**真的动一下引脚**，
 * 否则探针管道接通了也没有边沿可推，波形视图仍然是空的。
 */
const EXPECTED_BYTES = [
    0x93, 0x00, 0xa0, 0x02,
    0x13, 0x01, 0x70, 0x00,
    0xb3, 0x81, 0x20, 0x00,
    0x23, 0x20, 0x30, 0x10,
    0x6f, 0x00, 0x40, 0x00,
    0xb7, 0x02, 0x00, 0x40,
    0x13, 0x03, 0x10, 0x00,
    0x23, 0xa2, 0x62, 0x00,
    0x13, 0x02, 0x10, 0x00,
    0x6f, 0x00, 0x00, 0x00,
];

/**
 * Decode a little-endian 32-bit word out of the firmware bytes.
 *
 * @param {number} addr — byte offset inside the image
 * @returns {number}
 */
function wordAt(addr) {
    return (
        (DEMO_FIRMWARE_BYTES[addr] |
            (DEMO_FIRMWARE_BYTES[addr + 1] << 8) |
            (DEMO_FIRMWARE_BYTES[addr + 2] << 16) |
            (DEMO_FIRMWARE_BYTES[addr + 3] << 24)) >>> 0
    );
}

/**
 * Decode the S-type immediate of a `sw` / `sb` / `sh` word.
 *
 * imm[11:5] lives in bit[31:25], imm[4:0] in bit[11:7].
 *
 * @param {number} word
 * @returns {number}
 */
function decodeSTypeImm(word) {
    const immHi = (word >>> 25) & 0x7f;
    const immLo = (word >>> 7) & 0x1f;
    return (immHi << 5) | immLo;
}

describe("demo 固件", () => {
    test("40 个字节与 brief 逐字节相同", () => {
        assert.equal(DEMO_FIRMWARE_BYTES.length, 40, "固件长度必须是 40 字节");
        assert.deepEqual([...DEMO_FIRMWARE_BYTES], EXPECTED_BYTES, "固件字节与 brief 不一致");
    });

    test("字节是小端序（每个 32 位字的低字节在前）", () => {
        // 0x00 处是 `addi x1, x0, 42` = 0x02A00093。
        assert.equal(DEMO_FIRMWARE_BYTES[0], 0x93);
        assert.equal(DEMO_FIRMWARE_BYTES[3], 0x02);
        // 0x20 处是 `addi x4, x0, 1` = 0x00100213。
        assert.equal(DEMO_FIRMWARE_BYTES[0x20], 0x13);
        assert.equal(DEMO_FIRMWARE_BYTES[0x23], 0x00);
    });

    test("sw 的立即数**解码**回 0x100（不只是比对字面值）", () => {
        // 2026-10-21 订正：brief 最初给的 0x00302023 的 S 型立即数解出来是
        // 0（imm[11:5] 漏了），会写成 `sw x3, 0(x0)`；前端「读 0x100」就
        // 读不到东西。这条断言解**立即数**，所以同类错误不会再靠人眼发现。
        const word = wordAt(0x0c);
        assert.equal(word, 0x10302023, "0x0c 处的机器码不对");
        assert.equal(decodeSTypeImm(word), 0x100, "sw 的偏移量必须解回 0x100");
        // opcode / funct3 / rs2 / rs1 也要对得上，否则「立即数对了」也
        // 可能是一条别的指令。
        assert.equal(word & 0x7f, 0x23, "opcode 必须是 STORE");
        assert.equal((word >>> 12) & 0x7, 0x2, "funct3 必须是 word");
        assert.equal((word >>> 20) & 0x1f, 3, "rs2 必须是 x3");
        assert.equal((word >>> 15) & 0x1f, 0, "rs1 必须是 x0");
    });

    test("断点靶点在 0x20", () => {
        assert.equal(DEMO_FIRMWARE_BREAKPOINT_ADDR, 0x20);
        // 靶点处必须是 `addi x4, x0, 1`（0x00100213）。
        assert.deepEqual(
            [...DEMO_FIRMWARE_BYTES.slice(0x20, 0x24)],
            [0x13, 0x02, 0x10, 0x00],
            "0x20 处不是断点靶点指令",
        );
    });

    test("GPIO 设置段：0x14..0x1c 真的在驱动引脚（D-46）", () => {
        // 0x14: `lui x5, 0x40000` → x5 = GPIO 基址 0x4000_0000。
        const lui = wordAt(0x14);
        assert.equal(lui & 0x7f, 0x37, "opcode 必须是 LUI");
        assert.equal((lui >>> 7) & 0x1f, 5, "rd 必须是 x5");
        const imm20 = lui >>> 12; // LUI 的立即数就在 bit[31:12]
        assert.equal((imm20 << 12) >>> 0, DEMO_FIRMWARE_GPIO_BASE, "x5 必须是 GPIO 基址");

        // 0x18: `addi x6, x0, 1` → x6 = 1。
        const addi = wordAt(0x18);
        assert.equal(addi & 0x7f, 0x13, "opcode 必须是 ADDI");
        assert.equal((addi >>> 7) & 0x1f, 6, "rd 必须是 x6");
        assert.equal((addi >>> 15) & 0x1f, 0, "rs1 必须是 x0");
        assert.equal(addi >> 20, 1, "立即数必须是 1");

        // 0x1c: `sw x6, 4(x5)` → GPIO0.OUT = 1。
        const sw = wordAt(0x1c);
        assert.equal(sw & 0x7f, 0x23, "opcode 必须是 STORE");
        assert.equal((sw >>> 12) & 0x7, 0x2, "funct3 必须是 word");
        assert.equal((sw >>> 20) & 0x1f, 6, "rs2 必须是 x6");
        assert.equal((sw >>> 15) & 0x1f, 5, "rs1 必须是 x5");
        assert.equal(
            decodeSTypeImm(sw),
            DEMO_FIRMWARE_GPIO_OUT_OFFSET,
            "sw 的偏移量必须解回 GPIO OUT 的偏移",
        );
    });

    test("0x10 的跳转目标是 0x14（不是 0x20）", () => {
        // J 型立即数以**半字**为单位，解回字节偏移再加回 PC。
        const jal = wordAt(0x10);
        assert.equal(jal & 0x7f, 0x6f, "opcode 必须是 JAL");
        assert.equal((jal >>> 7) & 0x1f, 0, "rd 必须是 x0（无条件跳转）");
        const imm =
            ((jal >>> 31) & 0x1) * 0x100000 +
            ((jal >>> 21) & 0x3ff) * 2 +
            ((jal >>> 20) & 0x1) * 0x800 +
            ((jal >>> 12) & 0xff) * 0x1000;
        const sign = imm & 0x100000 ? imm - 0x200000 : imm;
        assert.equal(0x10 + sign, 0x14, "跳转必须落到 GPIO 设置段");
    });

    test("字节数组被冻结（调用方改不动）", () => {
        assert.throws(() => {
            "use strict";
            DEMO_FIRMWARE_BYTES[0] = 0xff;
        }, TypeError);
    });
});

describe("mock 工程 · 挂上 demo 固件镜像", () => {
    test("镜像以 RawHex + Bytes 的契约形状挂载", () => {
        const project = buildMockProject();
        assert.equal(project.images.length, 1, "工程必须挂一个镜像");
        const image = project.images[0];
        assert.equal(image.name, "demo-firmware.bin");
        assert.equal(image.kind, "RawHex");
        // `model::ImageSource::Bytes(Vec<u8>)` — externally tagged.
        assert.deepEqual(Object.keys(image.source), ["Bytes"]);
        assert.deepEqual(image.source.Bytes, [...DEMO_FIRMWARE_BYTES]);
    });

    test("工程的顶层字段没有变（契约里 Project 的字段名不能漂）", () => {
        const project = buildMockProject();
        assert.deepEqual(
            Object.keys(project).sort(),
            ["id", "images", "name", "tree", "version"],
        );
    });
});
