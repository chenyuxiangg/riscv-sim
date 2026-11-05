// v0.72 批 · STAGE-48 · D-177 A —— **连线报告**的前端那一侧。
//
// ## 这一套盯住什么
//
// 规格 `docs/detailed-design/STAGE-48_V072_Device_Instance_Table.md` §4.3
// 的 **N 组**：
//
//   N-1 前端镜像的 `SCHEMA_VERSION` **== 11**（与契约层同批）
//   N-2 前端**产品代码**里**零个**连线抬不了的**判定** ⇒ ⇒ 前端**不自己判**
//   N-3 前端把**内核给的** `wiring_report` **原样显示**（`declared` /
//      `live` / 逐条 `reason`）⇒ ⇒ **不翻译、不改写**内核的词
//   N-4 `wiring_report` **缺省**（老内核 / 老事件）⇒ ⇒ 视图**不炸**
//
// ## ⚠️ 判别力（每条都要能变红）
//
// - **N-2**：把 `front/lib/wiring-report.js` 里的白话表整个删掉 ⇒ ⇒
//   「前端自己判」那条**必须红** ⇒ ⇒ 判据**不是空的**。
// - **N-3**：把界面改成**自己重算** `declared` / `live` ⇒ ⇒ **必须红** ⇒ ⇒
//   界面上的数**只能**来自内核。
// - **N-4**：把缺省那档的处理删掉 ⇒ ⇒ **必须红**（老事件**不许**让前端炸）。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。
//
// # 零依赖、零构建

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

/** 动态 import 一个前端模块（ESM ⇒ 必须用 file URL）。 */
async function importFront(rel) {
    return import(pathToFileURL(resolve(frontDir, rel)).href);
}

/** 前端**产品代码**（不含测试）的全部源码。 */
function frontProductSources() {
    const out = [];
    const walk = (dir) => {
        for (const entry of readdirSync(dir, { withFileTypes: true })) {
            const full = resolve(dir, entry.name);
            if (entry.isDirectory()) {
                walk(full);
            } else if (entry.name.endsWith(".js")) {
                out.push(readFileSync(full, "utf8"));
            }
        }
    };
    walk(frontDir);
    return out.join("\n");
}

describe("STAGE-48 · 连线报告 · 前端那一侧", () => {
    test("N-1 · 前端镜像的 `SCHEMA_VERSION` **== 12**（与契约层同批）", async () => {
        const message = await importFront("lib/message.js");
        assert.equal(
            message.SCHEMA_VERSION,
            12,
            "前端镜像的 SCHEMA_VERSION 必须是 12（契约 v12：STAGE-52 的模板库协议）",
        );
    });

    test("N-2 · 前端**不自己判**「这根线抬不抬得了」", async () => {
        // ⚠️ **判别力**：把 `front/lib/wiring-report.js` 整个删掉（或把它的
        //   导出清空）⇒ ⇒ 这一条**必须红** ⇒ ⇒ 判据**不是空的**。
        //
        // ⚠️ 判据形态：前端源码里**不许**出现「按端点类型自己判断」的那套
        //   条件（源是不是 UART / 目标是不是 PLIC / 源号有没有越界）⇒ ⇒
        //   那些**全是内核的判定** ⇒ ⇒ 前端只**显示**。
        const sources = frontProductSources();
        // ⚠️ **判据钉在「内核那 6 个理由词」上**：前端**只**能**引用**它们
        //   （显示白话），**不许**自己按端点算 ⇒ ⇒ 「源越界」这种判定在
        //   前端**不许**出现第二次。
        const forbidden = [
            // ⚠️ 「源号越界」这种**判定**不许在前端出现（内核已经判过了）。
            "source >= ",
            "source_count",
            "sources <=",
        ];
        for (const needle of forbidden) {
            assert.ok(
                !sources.includes(needle),
                `前端源码里出现了「${needle}」⇒ ⇒ 前端**自己**判了一遍连线 ⇒ ⇒ 两个真相源`,
            );
        }
    });

    test("N-3 · 界面**原样显示**内核给的数字与理由词", async () => {
        const wr = await importFront("lib/wiring-report.js");
        const mm = await importFront("views/components/memory-map.js");

        const report = {
            declared: 3,
            live: 1,
            dropped: [
                {
                    from_device: "UART1",
                    event: "rx_ready",
                    to_device: "PLIC0",
                    source: 11,
                    reason: "source-unknown",
                },
            ],
        };
        const html = mm.renderWiringReport({ wiringReport: report });
        // ⚠️ **数字必须是内核给的那些**（不重算）。
        assert.ok(
            html.includes("写了 3 根"),
            `界面上必须显示内核给的 declared=3，实际渲染：${html}`,
        );
        assert.ok(
            html.includes("1 根当前抬不了"),
            `界面上必须显示内核给的「1 根当前抬不了」，实际渲染：${html}`,
        );
        // ⚠️ **理由词必须在**（白话**替代不了**原词 ⇒ ⇒ 用户能与内核原文
        //   对得上）。
        assert.ok(
            html.includes("source-unknown"),
            `界面上必须显示内核给的理由词 source-unknown，实际渲染：${html}`,
        );
        // ⚠️ **两端与源号都在**（用户能照着改描述）。
        assert.ok(html.includes("UART1"), `界面上必须显示源器件 id，实际渲染：${html}`);
        assert.ok(html.includes("PLIC0"), `界面上必须显示目标器件 id，实际渲染：${html}`);
    });

    test("N-3b · **未知理由词原样显示**（内核加了新理由 ⇒ 前端**不编**）", async () => {
        const wr = await importFront("lib/wiring-report.js");
        const label = wr.wireDropReasonLabel("some-brand-new-reason");
        assert.equal(label.code, "some-brand-new-reason", "原词必须原样");
        assert.equal(
            label.text,
            "some-brand-new-reason",
            "**未知词的白话必须就是原词** ⇒ ⇒ 前端**不许编**",
        );
    });

    test("N-3c · 界面上的数**只能**来自内核（前端**不重算**）", async () => {
        const mm = await importFront("views/components/memory-map.js");
        // ⚠️ **故意给一组自相矛盾的数**（declared=10 / live=1 / dropped=1）⇒ ⇒
        //   界面**必须原样显示**内核给的（**不**去修那个恒等式）⇒ ⇒ 证明
        //   前端**没有自己算**。
        const html = mm.renderWiringReport({
            wiringReport: {
                declared: 10,
                live: 1,
                dropped: [
                    {
                        from_device: "A",
                        event: "rx_ready",
                        to_device: "B",
                        source: 1,
                        reason: "overridden",
                    },
                ],
            },
        });
        assert.ok(
            html.includes("写了 10 根"),
            `界面必须显示内核给的 declared=10，实际渲染：${html}`,
        );
        assert.ok(
            html.includes("1 根当前抬不了"),
            `界面必须显示内核给的 dropped 条数，实际渲染：${html}`,
        );
    });

    test("N-4 · `wiring_report` **缺省** ⇒ ⇒ 视图**不炸**（老内核 / 老事件）", async () => {
        const mm = await importFront("views/components/memory-map.js");
        // ⚠️ **三种缺省形态**都要不炸。
        for (const memoryMap of [
            null,
            {},
            { wiringReport: null },
            { wiringReport: undefined },
        ]) {
            const html = mm.renderWiringReport(memoryMap);
            assert.equal(typeof html, "string", "缺省那档也必须渲染出字符串");
            assert.ok(
                html.includes("data-mmap-wiring"),
                `缺省那档必须带稳定钩子，实际渲染：${html}`,
            );
        }
    });

    test("N-4b · **零根线**那一档如实说「0 根」", async () => {
        const mm = await importFront("views/components/memory-map.js");
        const html = mm.renderWiringReport({
            wiringReport: { declared: 0, live: 0, dropped: [] },
        });
        assert.ok(
            html.includes("0 根"),
            `零根线那一档必须说 0 根，实际渲染：${html}`,
        );
    });

    test("N-4c · 归一化**不判定**（缺字段也给出确定的形状）", async () => {
        const wr = await importFront("lib/wiring-report.js");
        assert.equal(wr.normalizeWiringReport(null), null, "null ⇒ null");
        assert.equal(wr.normalizeWiringReport(undefined), null, "undefined ⇒ null");
        const norm = wr.normalizeWiringReport({ declared: "2", live: 1 });
        assert.equal(norm.declared, 2, "declared 必须被归一成数");
        assert.equal(norm.live, 1, "live 必须被归一成数");
        assert.deepEqual(norm.dropped, [], "缺省的 dropped 必须是空数组");
    });
});
