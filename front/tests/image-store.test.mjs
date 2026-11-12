// M-23 镜像持久化（刷新后恢复“已选择”状态）的前端门禁。
//
// 不碰真实 IndexedDB（node 环境没有）；这里盯的是**记录形状**与
// **backend 契约**，真实 IndexedDB 由浏览器真机走一遍。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const storeModule = await importFront("services/m23_image_store.js");

function memoryBackend() {
    const rows = new Map();
    return {
        rows,
        async get(key) {
            return rows.get(key) ?? null;
        },
        async put(record) {
            rows.set(record.key, record);
        },
        async delete(key) {
            rows.delete(key);
        },
    };
}

describe("镜像持久化记录", () => {
    test("记录只含 last、文件名、类型、字节，字节是普通数组", () => {
        const record = storeModule.imageRecord({
            name: "demo.elf",
            kind: "Elf",
            bytes: [0x7f, 0x45, 0x4c, 0x46],
        });
        assert.deepEqual(record, {
            key: "last",
            name: "demo.elf",
            kind: "Elf",
            byteLength: 4,
            bytes: [0x7f, 0x45, 0x4c, 0x46],
        });
    });

    test("没有字节 / 空字节不算可持久化镜像", () => {
        assert.equal(storeModule.imageRecord(null), null);
        assert.equal(storeModule.imageRecord({ name: "x.bin", bytes: [] }), null);
        assert.equal(storeModule.imageRecord({ name: "x.bin" }), null);
    });

    test("缺 kind / 非法 kind 不落库（避免刷新后按错误类型装载）", () => {
        assert.equal(
            storeModule.imageRecord({ name: "x.elf", bytes: [1, 2] }),
            null,
        );
        assert.equal(
            storeModule.imageRecord({ name: "x.bin", kind: "Hex", bytes: [1, 2] }),
            null,
        );
    });

    test("save / load / clear 走 backend，且恢复时保留字节", async () => {
        const backend = memoryBackend();
        const store = storeModule.createImageStore(backend);
        assert.equal(store.supported, true);

        await store.save({ name: "demo.bin", kind: "RawHex", bytes: [0xde, 0xad] });
        const loaded = await store.load();
        assert.equal(loaded.name, "demo.bin");
        assert.equal(loaded.kind, "RawHex");
        assert.deepEqual(loaded.bytes, [0xde, 0xad]);

        await store.clear();
        assert.equal(await store.load(), null);
    });

    test("没有 backend 时显式降级，不抛", async () => {
        const store = storeModule.createImageStore(null);
        assert.equal(store.supported, false);
        assert.equal(await store.load(), null);
        assert.equal(await store.save({ name: "x.bin", bytes: [1] }), false);
    });
});
