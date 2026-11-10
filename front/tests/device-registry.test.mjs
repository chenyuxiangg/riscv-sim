// STAGE-43 · v0.66 批 —— **内核侧中间层**的前端门禁（N 组 + X 组）。
//
// 这个套件盯**内核侧中间层的两条只读通道**在界面上的落点：
//
//   - 「器件类型（内核支持）」—— **内核实现了哪些类型** · 每个类型的窗口
//     大小 · 寄存器**偏移表**（**不含基址**）· 实例参数的合法区间；
//   - 「器件实例（逐实例）」—— **每个实例**绑到了哪个基址（`base: null`
//     ⇒ **尚未生效** ⇒ ⇒ 界面**显式**标出来）。
//
// ⚠️ **三条不变式**：
//   ① **前端不抄第二份** —— 类型集合与参数区间**只**来自内核 ⇒ ⇒ 前端
//      那份 `DEVICE_KINDS` 只能当「描述格式能写什么」的副本 ⇒ ⇒ 跨层
//      门禁钉住它 == 契约层的变体集合（裁决 I）。
//   ② **前端不自己判合法性** —— 区间**只显示**内核给的 ⇒ ⇒ 界面上
//      **不许**出现「越界 / 非法」这类前端自造的判断。
//   ③ **空态分得开** —— 「还没收到事件」与「收到但一个都没有」**两句话
//      不一样**（否则用户分不清是内核没说话还是内核说没有）。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（与 `load-image.test.mjs` 同一条理由）。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const devices = await importFront("services/m15_devices.js");
const registry = await importFront("views/components/device-registry.js");
const memoryMapView = await importFront("views/components/memory-map.js");
const { Command } = await importFront("lib/message.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

const productView = m02.get("product");

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** 内核会推的**器件类型表**（两条：一条有寄存器、一条没有参数）。 */
const TYPES = Object.freeze([
    {
        kind: "uart16550",
        window_bytes: 8,
        registers: [
            { name: "THR", offset: 0, width_bits: 32 },
            { name: "SCR", offset: 7, width_bits: 32 },
        ],
        params: [{ name: "baud_rate", min: 1, max: null }],
    },
    {
        kind: "gpio",
        window_bytes: 16,
        registers: [
            { name: "DIR", offset: 0, width_bits: 32 },
            { name: "OUT", offset: 4, width_bits: 32 },
        ],
        params: [],
    },
]);

/** 逐实例状态：一条**已绑定**、一条**待生效**。 */
const INSTANCES = Object.freeze([
    { id: "GPIO0", kind: "gpio", base: 0x40000000, window_bytes: 16 },
    { id: "UART0", kind: "uart16550", base: null, window_bytes: 8 },
]);

/** 渲染产品视图（**带上**器件那两张卡的数据）。 */
function renderProduct(extra = {}) {
    return productView.render({
        tree: { root: { name: "MainBoard", children: [] } },
        memoryMap: null,
        imageLoad: null,
        deviceTypes: null,
        deviceInstances: null,
        ...extra,
    });
}

// ---------------------------------------------------------------------------
// N 组
// ---------------------------------------------------------------------------

describe("STAGE-43 · 器件类型注册表与逐实例状态（产品视图）", () => {
    // ⚠️ **v0.77 产品视图重构**：原来的「器件类型（内核支持）」「器件实例（逐
    // 实例）」两张卡随产品视图改成「左树 + 右器件内存窗口」一起下线（新视图
    // 不展示这两张只读清单，器件落在左树里、内存落在右窗口）⇒ ⇒ N-1 ~ N-5
    // **随界面退役**。
    //
    // ⚠️ 留下的 X 组是**跨层**判据（前端那份类型清单 == 契约层的变体集合），
    // 它与界面无关 ⇒ ⇒ **一个字都不动**。

    test("N-6 跨层：前端的器件类型清单 == 契约层的变体集合（裁决 I）", () => {
        const platformRs = resolve(repoRoot, "crates/contract/src/platform.rs");
        assert.ok(existsSync(platformRs), "契约层源码不在（跨层门禁必须是硬失败，不是跳过）");
        const src = readFileSync(platformRs, "utf8");
        // ⚠️ **这里不许用带引号的正则**（D-174 那条洞）：归属口径工具的掩码
        // 认识注释与字符串、**不认识正则字面量** ⇒ ⇒ 正则里那个引号会被当成
        // 字符串开头 ⇒ ⇒ 后面若干条用例被**整段抹掉** ⇒ ⇒ 那个文件的用例数
        // 被**静默**少算 ⇒ ⇒ 与门禁总数对不上（749 vs 752）⇒ ⇒ 对账对出来的
        // ⇒ ⇒ 本批实测**撞了两次**（先 `&'static` 后 `/"([^"]+)"/`）⇒ ⇒
        // ⇒ 这里改成**纯字符串切分**（零正则、零引号配对问题）。
        const at = src.indexOf("pub const KINDS");
        assert.notEqual(at, -1, "契约层读不到 `DeviceSpec::KINDS`");
        // ⚠️ **要取「= 后面的那个 `[」** —— 类型标注里也有一个 `[`（`[&'static str; 4]`）
        // ⇒ ⇒ 直接找第一个 `[` 会切错地方 ⇒ ⇒ 从 `=` 之后再找。
        const eq = src.indexOf("=", at);
        const open = src.indexOf("[", eq);
        const close = src.indexOf("]", open);
        assert.ok(open > 0 && close > open, "契约层的 `DeviceSpec::KINDS` 形状不对");
        const rust = src
            .slice(open + 1, close)
            .split(String.fromCharCode(34))
            .filter((part) => part.length > 0 && part.trim().length === part.length)
            .sort();
        const js = [...memoryMapView.DEVICE_KINDS].sort();
        assert.deepEqual(
            js,
            rust,
            "前端的器件类型清单与契约层的变体集合不一致 ⇒ ⇒ 前端抄的那份已经漂了",
        );
    });
});

// ---------------------------------------------------------------------------
// 服务层接线
// ---------------------------------------------------------------------------

describe("STAGE-43 · 器件中间层的前端接线", () => {
    test("两张表进**产品视图**的视图状态（内核说什么就是什么）", () => {
        const router = {
            subs: new Map(),
            subscribe(topic, fn) {
                this.subs.set(topic, fn);
            },
            async dispatchCommand(cmd) {
                return cmd;
            },
        };
        const bridge = devices.wireDevices(router);
        assert.equal(bridge.getTypes(), null, "还没收到事件时，类型表应是 null（不是空数组）");
        assert.equal(bridge.getInstances(), null, "还没收到事件时，实例表应是 null（不是空数组）");

        router.subs.get("device.types")({ value: { types: TYPES } });
        router.subs.get("device.instances")({ value: { instances: INSTANCES } });

        assert.deepEqual(bridge.getTypes(), TYPES, "类型表没进缓存");
        assert.deepEqual(bridge.getInstances(), INSTANCES, "实例表没进缓存");
        const state = m03.getState("product");
        assert.deepEqual(state.deviceTypes, TYPES, "类型表没进产品视图的视图状态");
        assert.deepEqual(state.deviceInstances, INSTANCES, "实例表没进产品视图的视图状态");
    });

    test("订阅两条流（unit 变体是**裸字符串**）", async () => {
        const sent = [];
        const router = {
            subs: new Map(),
            subscribe(topic, fn) {
                this.subs.set(topic, fn);
            },
            async dispatchCommand(cmd) {
                sent.push(cmd);
                return cmd;
            },
        };
        const bridge = devices.wireDevices(router);
        await bridge.subscribeDeviceStreams();
        assert.deepEqual(
            sent,
            [
                { Subscribe: { topic: "device.types" } },
                { Subscribe: { topic: "device.instances" } },
            ],
            "订阅命令的形状不对（两个主题各一条）",
        );
        assert.equal(Command.listDeviceTypes(), "ListDeviceTypes", "unit 变体应是裸字符串");
        assert.equal(Command.listDeviceInstances(), "ListDeviceInstances", "unit 变体应是裸字符串");
    });

    test("新会话把两张表**清空**（状态必须跟着工程重建）", () => {
        const router = {
            subs: new Map(),
            subscribe(topic, fn) {
                this.subs.set(topic, fn);
            },
            async dispatchCommand(cmd) {
                return cmd;
            },
        };
        const bridge = devices.wireDevices(router);
        router.subs.get("device.types")({ value: { types: TYPES } });
        router.subs.get("device.instances")({ value: { instances: INSTANCES } });
        bridge.reset();
        assert.equal(bridge.getTypes(), null, "reset 之后类型表应该回到 null");
        assert.equal(bridge.getInstances(), null, "reset 之后实例表应该回到 null");
    });
});
