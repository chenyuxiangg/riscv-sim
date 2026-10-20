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

const { DEMO_FIRMWARE_BYTES, DEMO_FIRMWARE_BREAKPOINT_ADDR } = await importFront("dev/demo-firmware.js");
const { buildMockProject } = await importFront("dev/mock-project.js");

/** The byte list the stage-1 brief froze, byte for byte. */
const EXPECTED_BYTES = [
    0x93, 0x00, 0xa0, 0x02,
    0x13, 0x01, 0x70, 0x00,
    0xb3, 0x81, 0x20, 0x00,
    0x23, 0x20, 0x30, 0x10,
    0x6f, 0x00, 0x00, 0x01,
    0x13, 0x00, 0x00, 0x00,
    0x13, 0x00, 0x00, 0x00,
    0x13, 0x00, 0x00, 0x00,
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
