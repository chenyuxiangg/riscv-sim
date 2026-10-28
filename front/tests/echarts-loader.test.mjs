// 波形库加载器（STAGE-38 · v0.53 批 · 施工期订正 4）的前端门禁（A-17 ~ A-20）。
//
// ⚠️ **为什么这一组在 v0.53 批**：跑真机门禁时**偶发**红了一条 ——「降级路径」
//   那条（B-30）⇒ ⇒ 查下来是**加载器的一个真缺陷**：一个**在途的旧加载**
//   在「关掉」之后 resolve 时会把 `instance` 又装回去 ⇒ ⇒ 「关掉」这件事
//   **不生效** ⇒ ⇒ 界面从「波形库不可用」**变回**「能画」⇒ ⇒ **真机门禁
//   不稳定** ⇒ ⇒ 门禁**不稳定**就是**没做完** ⇒ ⇒ 本批把它修掉并钉住。
//
// ⚠️ **不用真的联网** ⇒ ⇒ 用一个 `data:` URL 当「一个能加载成功的源」
//   ⇒ ⇒ Node 的动态 `import()` 支持 `data:` ⇒ ⇒ 零依赖、零网络、零夹具文件。
//
// ⚠️ **模块级单例** ⇒ ⇒ 本文件的用例**按顺序**改加载器的来源 ⇒ ⇒ 每个用例
//   结束时把来源设回 `"off"` ⇒ ⇒ 后面任何一次误触发**都不会**去拉 CDN。

import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const loader = await importFront("lib/echarts-loader.js");

/** 一个**能加载成功**的源（`data:` URL ⇒ ⇒ 零网络、零夹具）。 */
const FAKE_OK = "data:text/javascript,export default { init: () => {} };";

beforeEach(() => {
    // ⚠️ **每个用例前把来源设回「关掉」** ⇒ ⇒ 误触发**不会**去拉 CDN。
    loader.setChartSource("off");
});

describe("波形库加载器（关掉 / 换源 / 失败）", () => {
    test("A-17 · 「关掉」⇒ ⇒ `loadECharts()` 返回 `null`（**不发网络请求**）", async () => {
        loader.setChartSource("off");
        const mod = await loader.loadECharts();
        assert.equal(mod, null, "「关掉」时必须返回 `null`（界面走降级）");
        assert.equal(loader.getChartSource(), "off", "来源应当仍是 `off`");
        assert.equal(loader.getECharts(), null, "「关掉」时不该有已加载的模块");
    });

    test("A-18 · **在途的旧加载**在换源之后 ⇒ ⇒ 结果**作废**（不复活旧源）", async () => {
        // ⚠️ **这一条是本组的核心** ⇒ ⇒ 以前 `if (instance) return instance`
        // 排在「源是 off」**前面** ⇒ ⇒ 一个在途的旧加载会把 `instance` 装回去
        // ⇒ ⇒ 下一次 `loadECharts()` 把**已经关掉**的库返回出去 ⇒ ⇒
        // ⇒ ⇒ 界面从「波形库不可用」变回「能画」⇒ ⇒ 真机门禁偶发红。
        loader.setChartSource(FAKE_OK);
        const inflight = loader.loadECharts();   // ⚠️ **在途**
        loader.setChartSource("off");            // ⚠️ **换源**（并且关掉）
        await inflight;                          // ⚠️ 等它在途的那一份 resolve

        assert.equal(
            loader.getECharts(),
            null,
            "在途的旧加载**不许**把已经换掉的源装回去",
        );
        const after = await loader.loadECharts();
        assert.equal(after, null, "「关掉」之后必须返回 `null`");
    });

    test("A-19 · 加载**失败**⇒ ⇒ 返回 `null` 且**不抛**（界面走降级）", async () => {
        loader.setChartSource("file:///definitely/not/here/echarts.mjs");
        const mod = await loader.loadECharts();
        assert.equal(mod, null, "加载失败必须返回 `null`（**不抛**）");
        assert.equal(loader.getECharts(), null, "失败之后不该留下半个模块");
    });

    test("A-20 · 加载**成功**⇒ ⇒ 返回模块且 `getECharts()` 能读到它", async () => {
        loader.setChartSource(FAKE_OK);
        const mod = await loader.loadECharts();
        assert.ok(mod, "加载成功必须返回模块");
        assert.ok(loader.getECharts(), "加载成功之后 `getECharts()` 必须能读到它");
        // ⚠️ **再加载一次** ⇒ ⇒ 走缓存 ⇒ ⇒ 同一个对象 ⇒ ⇒ 不重复加载。
        const again = await loader.loadECharts();
        assert.equal(again, mod, "第二次加载必须走缓存（同一个对象）");
    });
});
