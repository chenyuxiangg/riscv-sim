// front/lib/xterm-loader.js —— 终端仿真库（xterm.js）的**加载器**。
//
// ⚠️ **与 echarts-loader 同一条口径**（STAGE-36 · v0.51 批确立的外部依赖破口）：
//   - 零构建：不引入打包步骤 ⇒ 浏览器**原生**动态 `import()` 加载 ESM 产物；
//   - 锁版本：URL 不带 `@latest` ⇒ 同一个 URL 永远给同一个产物；
//   - 失败不抛：加载失败返回 `null` ⇒ 界面走**自研屏幕仿真器**降级路径，
//     核心调试闭环不受影响；
//   - Node 门禁安全：动态 `import()` 包在 `Promise.resolve().then(...)` 里 ⇒
//     Node 加载器同步抛的 `ERR_UNSUPPORTED_ESM_URL_SCHEME` 也变成拒绝，被
//     `.catch` 接住 ⇒ 一条路都不漏。
//
// ⚠️ **URL 是全仓唯一落点**（与 ECHARTS_URL 同规矩）⇒ 「第二个来源」要红。

/**
 * xterm 核心的**唯一**来源字面量（jsDelivr 的 +esm 包装产物，具名导出 `Terminal`）。
 *
 * ⚠️ 锁版本 5.5.0：不写 `@5` / `@latest` ⇒ 上游发新版不会让终端行为变形。
 */
export const XTERM_URL =
    "https://cdn.jsdelivr.net/npm/@xterm/xterm@5.5.0/+esm";

/** Fit 自适应附加件的**唯一**来源字面量（具名导出 `FitAddon`）。 */
export const XTERM_FIT_URL =
    "https://cdn.jsdelivr.net/npm/@xterm/addon-fit@0.10.0/+esm";

/** xterm 基础样式表（布局 / 视口样式；配色由主题选项从 CSS 令牌喂）。 */
export const XTERM_CSS_URL =
    "https://cdn.jsdelivr.net/npm/@xterm/xterm@5.5.0/css/xterm.css";

/** 已经加载好的库模块（没加载好是 `null`）。 */
let instance = null;

/** 正在进行的加载（避免并发时重复发请求）。 */
let pending = null;

/** 样式表是否已经注入过 `<head>`（只注一次）。 */
let cssLoaded = false;

/**
 * 同步读已经加载好的库模块（**读不到返回 `null`**）。
 *
 * ⚠️ 视图层只通过这个 getter 读已就绪的库 ⇒ **加载**走 `loadXterm()`，
 * 与 echarts-loader 的 `getECharts` 同形。
 *
 * @returns {{ Terminal: object, FitAddon: object } | null}
 */
export function getXterm() {
    return instance;
}

/**
 * 把 xterm 的基础样式表注入 `<head>`（幂等；Node 侧无 `document` 时跳过）。
 */
function injectCss() {
    if (cssLoaded) return;
    cssLoaded = true;
    if (typeof document === "undefined" || typeof document.head === "undefined") return;
    if (document.querySelector(`link[href="${XTERM_CSS_URL}"]`)) return;
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = XTERM_CSS_URL;
    document.head.appendChild(link);
}

/**
 * 加载 xterm 核心 + Fit 附加件。
 *
 * ⚠️ **失败不抛** ⇒ 返回 `null` ⇒ 视图层走自研仿真器降级路径。
 *
 * @returns {Promise<{ Terminal: object, FitAddon: object } | null>}
 */
export function loadXterm() {
    if (instance) return Promise.resolve(instance);
    if (pending) return pending;
    // ⚠️ **非浏览器环境（Node 门禁 / 测试替身）直接降级**：不发起跨源
    //   `import()` ⇒ 既不碰网络，也不会被 URL scheme / DNS 解析挂住。
    //   ⚠️ 不能只判 `window`：测试替身（dom-stub）会装一份 `window` 全局。
    const isNode = typeof process !== "undefined"
        && Boolean(process.versions && process.versions.node);
    if (isNode || typeof document === "undefined") {
        return Promise.resolve(null);
    }
    pending = Promise.resolve()
        .then(() => Promise.all([
            import(/* @vite-ignore */ XTERM_URL),
            import(/* @vite-ignore */ XTERM_FIT_URL),
        ]))
        .then(([core, fit]) => {
            const Terminal = core?.Terminal ?? core?.default?.Terminal ?? null;
            const FitAddon = fit?.FitAddon ?? fit?.default?.FitAddon ?? null;
            if (!Terminal || !FitAddon) {
                pending = null;
                return null;
            }
            injectCss();
            instance = { Terminal, FitAddon };
            pending = null;
            return instance;
        })
        .catch(() => {
            instance = null;
            pending = null;
            return null;
        });
    return pending;
}
