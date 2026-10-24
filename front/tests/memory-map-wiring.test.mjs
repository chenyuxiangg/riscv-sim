// v0.34 · Lane D（前端）—— 器件连线的**透传**与规则集版本（STAGE-19 · D-125 · 裁决 F / G）
//
// 本套件盯住验收 **C-1 ~ C-3**：
//
//   C-1 `draftFromDescription` → `draftToDescription` 往返 ⇒ `wiring` **逐字段**保留
//   C-2 草稿里 `wiring` 的**多余键** ⇒ ⇒ **不**进契约产物
//   C-3 前端镜像的 `PLATFORM_RULES_VERSION` = **3**
//       （`SCHEMA_VERSION` **仍 7**、`PLATFORM_SCHEMA_VERSION` **仍 1**）
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。
// ⚠️ **用例必须能变红**（不许恒真断言）⇒ 每条断言都在钉一个具体行为。
//
// # ⚠️ C-1 与 C-2 必须**各自独立**能红
//
// 变异自检：M1「`draftToDescription` **丢** `wiring`」⇒ ⇒ **精确红 C-1**；
// M2「`draftToDescription` **透传**草稿里 `wiring` 的多余键」⇒ ⇒ **精确红
// C-2** ⇒ ⇒ 所以 **C-2 那条不许直接摸 `description.wiring[0]`**（M1 之下它会
// 跟着一起红 ⇒ 就不「精确」了）⇒ ⇒ C-2 只断言「多余键**不在**契约产物
// 里」⇒ ⇒ 「连线**还在不在**」由 C-1 负责。
//
// # ⚠️ 跨层那一半**不在**本套件
//
// C-3 的另一半是「前端镜像 == 契约层」⇒ ⇒ 那条**跨层**断言由**整合批**验
// （`tools/wasm-smoke.mjs` 的 X-1 / X-2）⇒ ⇒ 本套件**只**钉住**本地常量**
// ⇒ ⇒ 本地红**不等于**跨层红。
//
// # 零依赖、零构建

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const mm = await importFront("views/components/memory-map.js");
const message = await importFront("lib/message.js");

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/**
 * 一块**带连线**的板（字面量）。
 *
 * ⚠️ 形状与契约层的 `PlatformDescription` 对齐（规格 §2.1 钉死）：
 *   `wiring: [{ from: { device, event }, to: { device, source } }]`
 *
 * ⚠️ **器件参数写全**（`irq_line` / `baud_rate` / `sources` / `contexts`）⇒
 * ⇒ 草稿 → 描述的往返**逐字段相同**（少写一个 ⇒ 往返会把缺省补上 ⇒ 就不
 * 逐字段相同了 ⇒ 那条断言会变成「断言另一件事」）。
 */
const WIRED_DESC = {
    schema_version: 1,
    id: "wired-board",
    name: "带连线的板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
        {
            kind: "peripheral",
            base: 268435456,
            device: { device: "uart16550", id: "UART0", irq_line: 1, baud_rate: 115200 },
        },
        {
            kind: "peripheral",
            base: 268439552,
            device: { device: "plic", id: "PLIC", sources: 32, contexts: 1 },
        },
    ],
    wiring: [
        { from: { device: "UART0", event: "rx_ready" }, to: { device: "PLIC", source: 3 } },
    ],
};

/**
 * 一块**没有连线**的板（字面量）。
 *
 * ⚠️ **压根没有 `wiring` 这个键**（不是 `wiring: []`、不是 `wiring: null`）⇒
 * ⇒ 契约层 serde 那边就是 `None` ⇒ ⇒ 「没有连线的板」这条存量情形。
 */
const PLAIN_DESC = {
    schema_version: 1,
    id: "plain-board",
    name: "普通板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
    ],
};

/**
 * 草稿里的**多余键**（草稿是**内部**形状 ⇒ 多出来的键**不许**进契约）。
 */
const EXTRA_KEY = "不该进契约";

// ---------------------------------------------------------------------------

describe("器件连线 · 透传（C-1）", () => {
    test("C-1 描述 → 草稿 → 描述 往返 ⇒ `wiring` **逐字段**保留", () => {
        const draft = mm.draftFromDescription(WIRED_DESC);
        assert.ok(draft, "应当产出一份草稿");
        assert.ok(Array.isArray(draft.wiring), "草稿里的 `wiring` 必须是**数组**");
        assert.equal(draft.wiring.length, 1);
        assert.deepEqual(draft.wiring[0], WIRED_DESC.wiring[0], "草稿里的连线必须逐字段相同");

        const description = mm.draftToDescription(draft);
        assert.deepEqual(description.wiring, WIRED_DESC.wiring, "往返之后连线必须逐字段相同");
    });

    test("C-1 连线的**四个子字段**一个都不许丢", () => {
        const description = mm.draftToDescription(mm.draftFromDescription(WIRED_DESC));
        assert.ok(Array.isArray(description.wiring), "线上描述里必须有 `wiring`");
        const [link] = description.wiring;
        assert.deepEqual(Object.keys(link).sort(), ["from", "to"]);
        assert.deepEqual(Object.keys(link.from).sort(), ["device", "event"]);
        assert.deepEqual(Object.keys(link.to).sort(), ["device", "source"]);
        assert.equal(link.from.device, "UART0");
        assert.equal(link.from.event, "rx_ready");
        assert.equal(link.to.device, "PLIC");
        assert.equal(link.to.source, 3);
    });

    test("C-1 整份描述往返 ⇒ **逐字段相同**（`wiring` 不许把别的东西挤掉）", () => {
        const roundTrip = mm.draftToDescription(mm.draftFromDescription(WIRED_DESC));
        assert.ok(
            mm.sameDescription(WIRED_DESC, roundTrip),
            `往返必须逐字段相同（实际：${JSON.stringify(roundTrip)}）`,
        );
    });

    test("C-1（附）前端**不判**连线合法性 ⇒ 不静默纠正（合法性只由内核裁决）", () => {
        // ⚠️ 这条连线**指向不存在的器件**、**source 越界** ⇒ 前端**照单全收**
        // ⇒ ⇒ 合法性**只**由内核那一处裁决（`E_PLT_16`）⇒ ⇒ 前端**不写
        // 第二个校验器**。
        const description = mm.draftToDescription(mm.draftFromDescription({
            ...WIRED_DESC,
            wiring: [
                {
                    from: { device: "NO_SUCH_DEVICE", event: "rx_ready" },
                    to: { device: "GHOST", source: 9999 },
                },
            ],
        }));
        assert.ok(Array.isArray(description.wiring), "线上描述里必须有 `wiring`");
        assert.equal(description.wiring[0].from.device, "NO_SUCH_DEVICE");
        assert.equal(description.wiring[0].to.device, "GHOST");
        assert.equal(description.wiring[0].to.source, 9999);
    });
});

describe("器件连线 · 逐字段重建（C-2）", () => {
    test("C-2 草稿里 `wiring` 的**多余键** ⇒ ⇒ **不**进契约产物", () => {
        const draft = mm.draftFromDescription(WIRED_DESC);
        // ⚠️ **手工**往草稿里塞多余键（`from.extra` / `to.extra` / 条目级 `extra`）
        // ⇒ ⇒ 逐字段重建这条纪律**必须**把它们挡在契约之外。
        draft.wiring = [
            {
                from: { device: "UART0", event: "rx_ready", extra: EXTRA_KEY },
                to: { device: "PLIC", source: 3, extra: EXTRA_KEY },
                extra: EXTRA_KEY,
            },
        ];

        const description = mm.draftToDescription(draft);
        assert.equal(
            JSON.stringify(description).includes(EXTRA_KEY),
            false,
            "草稿里的多余键漏进了契约产物",
        );
        // ⚠️ **逐字段重建** ⇒ 线上那一条的键集合**恰好**是那两个（不多不少）。
        // ⚠️ 这里用 `?? []`：连线**整体**丢没丢由 C-1 负责 ⇒ ⇒ 这条用例只
        // 盯「多余键进没进契约」⇒ ⇒ 变异 M1 之下它**不**跟着红。
        for (const entry of description.wiring ?? []) {
            assert.deepEqual(Object.keys(entry).sort(), ["from", "to"], "条目级不许有多余键");
            assert.deepEqual(Object.keys(entry.from ?? {}).sort(), ["device", "event"]);
            assert.deepEqual(Object.keys(entry.to ?? {}).sort(), ["device", "source"]);
        }
    });
});

describe("器件连线 · 已知边界（裁决 G）", () => {
    test("⚠️ **没有** `wiring` 的板往返后**仍然**没有 `wiring`", () => {
        const draft = mm.draftFromDescription(PLAIN_DESC);
        assert.equal(
            "wiring" in draft,
            false,
            "草稿里不该凭空多一个 `wiring`（连空数组都不给）",
        );
        const description = mm.draftToDescription(draft);
        assert.equal("wiring" in description, false, "线上描述不该凭空多一个 `wiring`");
        assert.equal(description.wiring, undefined, "更不许是 `null`");
        assert.ok(
            mm.sameDescription(PLAIN_DESC, description),
            "没有连线的板往返必须逐字段相同",
        );
    });

    test("⚠️ `wiring: []`（空表）与**缺省**同义 ⇒ 线上描述**不带**这个键", () => {
        const draft = mm.draftFromDescription({ ...PLAIN_DESC, wiring: [] });
        const description = mm.draftToDescription(draft);
        assert.equal("wiring" in description, false, "空表也不该造出一个 `wiring` 键");
    });

    test("⚠️ 裁决 G：**从界面提交的板永远没有 `wiring`**（编辑器不暴露它）", () => {
        const draft = mm.defaultDraft();
        assert.equal("wiring" in draft, false, "默认草稿不该带 `wiring`");
        const description = mm.draftToDescription(draft);
        assert.equal("wiring" in description, false, "界面提交的那份不该带 `wiring`");
    });

    test("⚠️ `cloneDraft` **深复制** `wiring`（改草稿不许改到内核给的那份）", () => {
        const draft = mm.draftFromDescription(WIRED_DESC);
        const copy = mm.cloneDraft(draft);
        copy.wiring[0].from.device = "MUTATED";
        copy.wiring[0].to.source = 99;
        assert.equal(draft.wiring[0].from.device, "UART0", "改副本改到了原草稿");
        assert.equal(draft.wiring[0].to.source, 3, "改副本改到了原草稿");
    });
});

describe("规则集版本 · 前端镜像（C-3）", () => {
    test("C-3 `PLATFORM_RULES_VERSION` = **3**（规则集收紧 ⇒ 这个数 +1）", () => {
        assert.equal(
            message.PLATFORM_RULES_VERSION,
            3,
            "规则集版本必须是 3（STAGE-19 裁决 F：2 → 3）",
        );
    });

    test("C-3 另外两个版本号**一个都不升**", () => {
        assert.equal(message.SCHEMA_VERSION, 7, "契约信封版本必须仍是 7（零信封变更）");
        assert.equal(
            message.PLATFORM_SCHEMA_VERSION,
            1,
            "描述格式版本必须仍是 1（只加了一个带缺省的可选字段）",
        );
    });

    test("C-3 规则集版本**只**住在镜像层这一个地方（服务层与视图层都 re-export 它）", async () => {
        const boardstore = await importFront("services/m23_boardstore.js");
        assert.equal(
            boardstore.PLATFORM_RULES_VERSION,
            message.PLATFORM_RULES_VERSION,
            "服务层与镜像层必须是**同一个**版本号（两个真相会漂）",
        );
        assert.equal(
            mm.PLATFORM_SCHEMA_VERSION,
            message.PLATFORM_SCHEMA_VERSION,
            "视图层与镜像层必须是**同一个**形状版本号",
        );
    });
});
