// STAGE-47 · v0.71 批 —— **「未知系统」门禁**的前端门禁（N 组 · **M-02 视图层**）。
//
// ⚠️ **自述模块**（归属口径工具读文件头前 40 行判桶）⇒ ⇒ 本文件只 import 了
//   `lib/message.js`（**跨层共享** ⇒ ⇒ **不参与判桶**）⇒ ⇒ 不自述就会 **TIE**
//   ⇒ ⇒ 归属工具会记 TIE ⇒ ⇒ 门禁**红** ⇒ ⇒ ⇒ 自述是**必须**的。
//
// ⚠️ **本批前端**零改动** ⇒ ⇒ 判定**完全**在核心里 ⇒ ⇒ 前端那几条判据
// 钉的是「**前端不自己判**」这件事 ⇒ ⇒ 「**同一个量只有一处真相**」这条
// 纪律**结构上**成立。
//
// ⚠️ **真交互**（真装固件 ⇒ ⇒ 真内核拒 ⇒ ⇒ 界面显示内核原文）由
//   `tools/browser-verify.mjs` 的 `unknownSystem` 段（**B-141 ~ B-145**）
//   负责 ⇒ ⇒ 两边合起来才是完整的判据。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const message = await importFront("lib/message.js");

/**
 * 前端**产品代码**的源码（用来证明「前端没有自己判」）。
 *
 * ⚠️ **排除 `front/tests/` 与 `front/dev/`** ⇒ ⇒ 测试文件**自己**会提到
 *   「未知系统」（那是**判据的措辞**）⇒ ⇒ 不排除 ⇒ ⇒ **自己把自己抓出来**
 *   ⇒ ⇒ 那条断言**恒红** ⇒ ⇒ 零判别力 ⇒ ⇒ 纪律要钉的是「**产品代码**不
 *   自己判」⇒ ⇒ 不是「仓里没有这个词」。
 */
function frontSources() {
    const out = [];
    const walk = (dir) => {
        for (const entry of readdirSync(dir, { withFileTypes: true })) {
            const full = resolve(dir, entry.name);
            if (entry.isDirectory()) {
                if (entry.name === "tests" || entry.name === "dev") continue;
                walk(full);
            } else if (/\.(js|mjs)$/.test(entry.name)) {
                out.push(readFileSync(full, "utf8"));
            }
        }
    };
    walk(resolve(frontDir));
    return out.join("\n");
}


describe("STAGE-47 · 「未知系统」门禁 · 前端那一侧", () => {
    test("N-1 · 前端镜像的 `SCHEMA_VERSION` **与契约层同批**（当前 **12**）", () => {
        // ⚠️ **这条断言跟版本号走**（literal 钉住当前值）⇒ ⇒ 每批升版
        //   都要**同步**它 ⇒ ⇒ 「一侧升了一侧没升」这件事**当场变红**。
        //
        // ⚠️ **STAGE-47（v0.71）那一批本身是零升版**（当时是 10）⇒ ⇒
        //   「那一批没升版」这件事由 **STAGE-47 规格**记着；本条断言现在
        //   守的是「**两侧同批**」这条**跨层**纪律 ⇒ ⇒ v0.76 批（STAGE-52）
        //   升到 **12** ⇒ ⇒ 这里跟着升。
        assert.equal(
            String(message.SCHEMA_VERSION),
            "12",
            "前端镜像的 SCHEMA_VERSION 变了 ⇒ ⇒ 它与契约层**不同批**了",
        );
    });

    test("N-2 · 前端**不自己判**「未知系统」（源码里**零个**「未知系统」字样）", () => {
        // ⚠️ **判定只在内核里** ⇒ ⇒ 前端**不**写第二份校验器 ⇒ ⇒ 前端
        //   **不该**出现自己编的「未知系统」措辞。
        const sources = frontSources();
        assert.ok(
            !sources.includes("未知系统"),
            "前端源码里出现了「未知系统」⇒ ⇒ 前端**自己**也判了一遍 ⇒ ⇒ 两个真相源",
        );
    });

    test("N-3 · 前端的场景码表里**没有** `E_PLT-17` ⇒ ⇒ 前端**不可能**对它做分支", () => {
        // ⚠️ 前端**不需要**知道这个码 ⇒ ⇒ 它**不在**前端镜像里 ⇒ ⇒ 前端
        //   **不可能**对它做「自己的处理」⇒ ⇒ 界面显示的**只能是内核原文**。
        const refs = message.RUST_ERROR_CODE_REFS ?? {};
        assert.equal(
            Object.prototype.hasOwnProperty.call(refs, "E-PLT-17"),
            false,
            "前端镜像里登记了 `E-PLT-17` ⇒ ⇒ 前端对它做了分支 ⇒ ⇒ 那是第二份判定",
        );
    });
});
