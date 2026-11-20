// 终端库加载器（xterm.js）的前端门禁 —— 与 `echarts-loader.test.mjs` 同口径。
//
// ⚠️ **不用真的联网**：Node 门禁 / 测试替身环境下 `loadXterm()` 必须直接返回
//   `null`（不发起跨源 `import()`）⇒ ⇒ 既不碰网络，也不会被 URL scheme /
//   DNS 解析挂住；界面走自研屏幕仿真器降级路径。
//
// ⚠️ **URL 是全仓唯一落点**：锁版本（不写 `@latest`）⇒ ⇒ 上游发新版不会让
//   终端行为变形。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const loader = await importFront("lib/xterm-loader.js");

describe("终端库加载器（xterm.js · Node 门禁安全）", () => {
    test("Node 门禁环境 `loadXterm()` 返回 `null`，不碰网络、不留已加载实例", async () => {
        assert.equal(loader.getXterm(), null, "初始不该有已加载的模块");
        const mod = await loader.loadXterm();
        assert.equal(mod, null, "非浏览器环境必须降级返回 `null`");
        assert.equal(loader.getXterm(), null, "降级之后也不该留下实例");
    });

    test("三个来源都锁版本（唯一落点，不写 `latest`）", () => {
        assert.match(loader.XTERM_URL, /@5\.5\.0\//, "核心必须锁 5.5.0");
        assert.match(loader.XTERM_FIT_URL, /@0\.10\.0\//, "Fit 附加件必须锁 0.10.0");
        assert.match(loader.XTERM_CSS_URL, /@5\.5\.0\//, "样式表必须锁 5.5.0");
        assert.ok(loader.XTERM_URL.includes("cdn.jsdelivr.net"), "来源必须是 jsDelivr");
    });
});
