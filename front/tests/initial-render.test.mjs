// Regression gate for the "the first render event is dropped" defect
// (FRONTEND_Review.md #20).
//
// The initial route dispatches the **first** `riscv:render` event. If the
// view component library is not subscribed yet, that event is lost — and
// the page comes up (no white screen, no console error) with every view
// body *empty*. It is invisible to a stub test that routes *after*
// bootstrap, and only shows up in a real browser.
//
// This file boots the front end with `?role=product`, so the initial
// view is one that actually has a component, and asserts the body really
// got rendered.
//
// A separate file is required: the `?role=` query is read at import time
// and the modules are singletons — node:test runs every test file in its
// own process.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));
// The initial view must be one that has a component — the "embedded"
// workbench view has none in the MVP, which would hide the defect.
dom.location.search = "?role=product";

await import(pathToFileURL(resolve(frontDir, "app.js")).href);
await new Promise((r) => setTimeout(r, 0));

test("初始路由的 render 事件不丢失（视图体必须已渲染）", () => {
    const body = dom.document.querySelector('[data-view-body="product"]');
    assert.ok(body, "产品视图体未挂载");
    assert.ok(
        body.innerHTML.includes("产品树"),
        `初始视图体是空的 —— 首次 render 事件被丢弃（innerHTML 长度 ${body.innerHTML.length}）`,
    );
    assert.ok(body.innerHTML.includes("内存映射"), "初始视图体缺少内存映射区");
});
