// v0.30 · Lane B —— 板档案服务（M-23 · STAGE-15 §3.3）的前端门禁。
//
// 这个套件盯的是**规格 §5.2 的 B-1 ~ B-8**（Lane B · 前端服务层），
// 外加 `createBoardStore` 门面的四个方法。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。
// ⚠️ **用例必须能变红**（不许恒真断言）⇒ 每条断言都在钉一个具体行为。
//
// # 为什么存储层要拆成「纯逻辑 + backend」
//
// 前端**零依赖**（AGENTS.md）⇒ 不许引 `fake-indexeddb` ⇒ 真实
// IndexedDB 那一层在 node 里**测不了**（规格 §6.3 明写了这个覆盖缺口，
// 靠浏览器真机门禁 T-3 兜）⇒ 于是默认门禁覆盖的是**纯逻辑层 + 内存
// backend** ⇒ 这一层必须测扎实。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import {
    BOARD_DB_NAME,
    BOARD_DB_VERSION,
    BOARD_STORE_NAME,
    BOARD_STORE_KEY,
    BOARD_DESCRIPTION_SCHEMA_VERSION,
    boardId,
    boardEntry,
    mergeBoard,
    dropBoard,
    findBoard,
    isBoardIdTaken,
    checkBoardVersion,
    createMemoryBackend,
    createIndexedDbBackend,
    createBoardStore,
} from "../services/m23_boardstore.js";

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** 一块板 A 的描述。 */
const DESC_A = {
    schema_version: 1,
    id: "board-a",
    name: "板 A",
    regions: [],
};

/** 一块板 B 的描述。 */
const DESC_B = {
    schema_version: 1,
    id: "board-b",
    name: "板 B",
    regions: [],
};

/** 一块板 C 的描述（用来排「升序」）。 */
const DESC_C = {
    schema_version: 1,
    id: "board-c",
    name: "板 C",
    regions: [],
};

// ---------------------------------------------------------------------------

describe("板档案服务 · 纯逻辑层（B-1 ~ B-5 · B-8）", () => {
    test("B-1 `boardId` 取的是描述的 `id`（不是别的字段）", () => {
        assert.equal(boardId(DESC_A), "board-a");
        assert.equal(boardId(DESC_B), "board-b");
        // ⚠️ `id` 是**描述自己的**字段 ⇒ 换个字段名就不该拿到值
        assert.equal(boardId({ id: "x", name: "别的名字" }), "x");
    });

    test("B-1 `id` 为空 / 缺失 / 不是字符串 ⇒ 返回 `null`", () => {
        assert.equal(boardId({ id: "" }), null);
        assert.equal(boardId({}), null);
        assert.equal(boardId({ id: null }), null);
        assert.equal(boardId({ id: 123 }), null);
        assert.equal(boardId(null), null);
        assert.equal(boardId(undefined), null);
    });

    test("B-2 `mergeBoard` 同 `id` ⇒ **覆盖**（新值生效、旧值不留）", () => {
        const seed = mergeBoard([], boardEntry(DESC_A));
        assert.equal(seed.changed, true);
        assert.equal(seed.archive.length, 1);

        const edited = {
            schema_version: 1,
            id: "board-a",
            name: "板 A（改过）",
            regions: [],
        };
        const merged = mergeBoard(seed.archive, boardEntry(edited));
        assert.equal(merged.changed, true);
        assert.equal(merged.archive.length, 1, "同 id 覆盖不该多出一块板");
        assert.equal(merged.archive[0].id, "board-a");
        assert.equal(merged.archive[0].description.name, "板 A（改过）");
    });

    test("B-2 `mergeBoard` 新 `id` ⇒ **追加**", () => {
        const first = mergeBoard([], boardEntry(DESC_A));
        const second = mergeBoard(first.archive, boardEntry(DESC_B));
        assert.equal(second.changed, true);
        assert.equal(second.archive.length, 2);
        assert.deepEqual(
            second.archive.map((item) => item.id),
            ["board-a", "board-b"],
        );
    });

    test("B-2 `mergeBoard` 是**纯函数**：不改入参、返回新 archive", () => {
        const seed = mergeBoard([], boardEntry(DESC_A));
        const before = JSON.stringify(seed.archive);

        const merged = mergeBoard(seed.archive, boardEntry(DESC_B));
        const dropped = dropBoard(seed.archive, "board-a");

        assert.equal(JSON.stringify(seed.archive), before, "入参 archive 被改了");
        assert.equal(seed.archive.length, 1, "入参 archive 被改了");
        assert.notEqual(merged.archive, seed.archive, "返回的 archive 与入参是同一个对象");
        assert.notEqual(dropped.archive, seed.archive, "返回的 archive 与入参是同一个对象");
    });

    test("B-2 `mergeBoard(archive, null)` ⇒ `changed: false`", () => {
        const seed = mergeBoard([], boardEntry(DESC_A));
        const result = mergeBoard(seed.archive, null);
        assert.equal(result.changed, false);
        assert.equal(result.archive.length, 1);
    });

    test("B-3 `dropBoard` 删存在的 ⇒ `changed: true`（且真的没了）", () => {
        const seed = mergeBoard(mergeBoard([], boardEntry(DESC_A)).archive, boardEntry(DESC_B));
        const result = dropBoard(seed.archive, "board-a");
        assert.equal(result.changed, true);
        assert.equal(result.archive.length, 1);
        assert.equal(result.archive[0].id, "board-b");
    });

    test("B-3 `dropBoard` 删不存在的 ⇒ `changed: false`（且档案不变）", () => {
        const seed = mergeBoard([], boardEntry(DESC_A));
        const result = dropBoard(seed.archive, "board-zzz");
        assert.equal(result.changed, false);
        assert.equal(result.archive.length, 1);
        assert.equal(result.archive[0].id, "board-a");
    });

    test("B-4 `isBoardIdTaken` 对已占用的 `id` ⇒ `true`", () => {
        const seed = mergeBoard([], boardEntry(DESC_A));
        assert.equal(isBoardIdTaken(seed.archive, "board-a"), true);
    });

    test("B-4 `isBoardIdTaken` 对没占用的 `id` ⇒ `false`", () => {
        const seed = mergeBoard([], boardEntry(DESC_A));
        assert.equal(isBoardIdTaken(seed.archive, "board-b"), false);
        assert.equal(isBoardIdTaken(seed.archive, ""), false);
        assert.equal(isBoardIdTaken([], "board-a"), false);
    });

    test("B-5 `checkBoardVersion`：`schema_version` 不等于 1 ⇒ 明确地拒", () => {
        const result = checkBoardVersion({ ...DESC_A, schema_version: 2 });
        assert.equal(result.ok, false);
        assert.equal(result.reason, "version");
        assert.equal(result.expected, 1);
        assert.equal(result.actual, 2);
    });

    test("B-5 `checkBoardVersion`：`schema_version` 等于 1 ⇒ 通过", () => {
        const result = checkBoardVersion(DESC_A);
        assert.equal(result.ok, true);
        assert.equal("reason" in result, false);
    });

    test("B-5 版本对不上时**不返回任何「已修正的描述」**（不许静默按新版本解析）", () => {
        const result = checkBoardVersion({ ...DESC_A, schema_version: 7 });
        assert.equal(result.ok, false);
        assert.equal("description" in result, false, "失败结果里不该带一份「修正过的描述」");
        assert.equal("actual" in result, true);
        assert.equal(result.actual, 7);
    });

    test("B-5 描述缺 `schema_version` ⇒ 拒（如实报 `actual = null`）", () => {
        const result = checkBoardVersion({ id: "board-a", name: "板 A" });
        assert.equal(result.ok, false);
        assert.equal(result.reason, "version");
        assert.equal(result.expected, 1);
        assert.equal(result.actual, null);
    });

    test("B-8 板档案条目**不含时间戳**：字段集合恰为 `id` + `description`", () => {
        const entry = boardEntry(DESC_A);
        assert.deepEqual(Object.keys(entry).sort(), ["description", "id"]);
        // ⚠️ `name` 在描述里 ⇒ 条目里**不另存**一份（否则就是第二份真相）
        assert.equal("name" in entry, false);
        assert.equal("createdAt" in entry, false);
        assert.equal("updatedAt" in entry, false);
    });

    test("B-8 `boardEntry` 取不到 `id` ⇒ `null`（不产出一条没主键的条目）", () => {
        assert.equal(boardEntry({ id: "" }), null);
        assert.equal(boardEntry(null), null);
    });

    test("`findBoard` 找得到 ⇒ 返回条目；找不到 ⇒ `null`", () => {
        const seed = mergeBoard([], boardEntry(DESC_A));
        const found = findBoard(seed.archive, "board-a");
        assert.ok(found);
        assert.equal(found.id, "board-a");
        assert.equal(found.description.name, "板 A");
        assert.equal(findBoard(seed.archive, "board-zzz"), null);
    });
});

describe("板档案服务 · 存储适配层（B-6 · B-7）", () => {
    test("B-6 内存 backend 往返：写入 ⇒ `list()` 读回**逐字段相同**", async () => {
        const backend = createMemoryBackend();
        const entry = boardEntry(DESC_A);

        await backend.put(entry);
        const rows = await backend.list();

        assert.equal(rows.length, 1);
        assert.deepEqual(rows[0], entry, "读回的条目与写进去的逐字段不同");
        assert.equal(rows[0].id, "board-a");
        assert.deepEqual(rows[0].description, DESC_A);
    });

    test("B-6 内存 backend：`put` 同 `id` ⇒ **覆盖**（不多出一条）", async () => {
        const backend = createMemoryBackend();
        await backend.put(boardEntry(DESC_A));
        await backend.put(boardEntry({ ...DESC_A, name: "板 A（改过）" }));

        const rows = await backend.list();
        assert.equal(rows.length, 1);
        assert.equal(rows[0].description.name, "板 A（改过）");
    });

    test("B-6 内存 backend：`remove` ⇒ 读不到", async () => {
        const backend = createMemoryBackend();
        await backend.put(boardEntry(DESC_A));
        await backend.put(boardEntry(DESC_B));

        await backend.remove("board-a");
        const rows = await backend.list();

        assert.equal(rows.length, 1);
        assert.equal(rows[0].id, "board-b");
        assert.equal(findBoard(rows, "board-a"), null);
    });

    test("B-6 内存 backend：`clear` ⇒ 读回空", async () => {
        const backend = createMemoryBackend();
        await backend.put(boardEntry(DESC_A));
        await backend.put(boardEntry(DESC_B));

        await backend.clear();
        assert.deepEqual(await backend.list(), []);
    });

    test("B-6 内存 backend：`list()` 的顺序**确定**（按 `id` 升序）", async () => {
        const backend = createMemoryBackend();
        await backend.put(boardEntry(DESC_C));
        await backend.put(boardEntry(DESC_A));
        await backend.put(boardEntry(DESC_B));

        const rows = await backend.list();
        assert.deepEqual(rows.map((row) => row.id), ["board-a", "board-b", "board-c"]);
    });

    test("B-7 没有 IndexedDB 的环境 ⇒ **明确地**返回「不支持」（不抛、不假装成功）", () => {
        // ⚠️ node 环境**本来就没有** IndexedDB ⇒ 这一条在 node:test 里
        // 天然可测（`globalThis.indexedDB` 是 `undefined`）。
        assert.equal(typeof globalThis.indexedDB, "undefined", "本用例假设 node 环境没有 IndexedDB");

        const backend = createIndexedDbBackend();
        assert.equal(backend.supported, false, "没有 IndexedDB 却说支持");
        assert.equal(typeof backend.reason, "string");
        assert.ok(backend.reason.length > 0, "「不支持」必须带原因");
    });
});

describe("板档案服务 · 存储门面（`createBoardStore`）", () => {
    test("`list()` ⇒ 读全部并按 `id` 升序", async () => {
        const store = createBoardStore(createMemoryBackend());
        await store.put(boardEntry(DESC_C));
        await store.put(boardEntry(DESC_A));
        await store.put(boardEntry(DESC_B));

        const rows = await store.list();
        assert.deepEqual(rows.map((row) => row.id), ["board-a", "board-b", "board-c"]);
    });

    test("`put(entry)` ⇒ 写一条（可读回）", async () => {
        const store = createBoardStore(createMemoryBackend());
        await store.put(boardEntry(DESC_A));

        const rows = await store.list();
        assert.equal(rows.length, 1);
        assert.deepEqual(rows[0], boardEntry(DESC_A));
    });

    test("`remove(id)` ⇒ 删一条", async () => {
        const store = createBoardStore(createMemoryBackend());
        await store.put(boardEntry(DESC_A));
        await store.put(boardEntry(DESC_B));

        await store.remove("board-a");
        const rows = await store.list();
        assert.equal(rows.length, 1);
        assert.equal(rows[0].id, "board-b");
    });

    test("`clear()` ⇒ 清空全部", async () => {
        const store = createBoardStore(createMemoryBackend());
        await store.put(boardEntry(DESC_A));
        await store.put(boardEntry(DESC_B));

        await store.clear();
        assert.deepEqual(await store.list(), []);
    });

    test("存储不可用 ⇒ `list()` 返回**空数组**（不抛错，让上层能显式降级）", async () => {
        const store = createBoardStore(createIndexedDbBackend());
        assert.equal(store.supported, false);
        assert.deepEqual(await store.list(), []);
    });

    test("存储不可用 ⇒ 写操作**明确地失败**（不静默假装成功）", async () => {
        const store = createBoardStore(createIndexedDbBackend());
        await assert.rejects(
            () => store.put(boardEntry(DESC_A)),
            /E_STORE-01/,
            "存储不可用时写操作必须明确失败",
        );
        await assert.rejects(() => store.remove("board-a"), /E_STORE-01/);
        await assert.rejects(() => store.clear(), /E_STORE-01/);
    });

    test("常量按规格 §3.3 钉住（库名独立 ⇒ 不碰别的库）", () => {
        assert.equal(BOARD_DB_NAME, "riscv-sim-board-store");
        assert.equal(BOARD_DB_VERSION, 1);
        assert.equal(BOARD_STORE_NAME, "boards");
        assert.equal(BOARD_STORE_KEY, "id");
        assert.equal(BOARD_DESCRIPTION_SCHEMA_VERSION, 1);
    });
});
