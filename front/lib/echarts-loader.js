// front/lib/echarts-loader.js —— 波形库的**加载器**（STAGE-36 · v0.51 批）。
//
// ⚠️ **本批第一次引入外部依赖**（echarts）⇒ ⇒ 这是**一个显式的破口**，不是
//   「顺手加个库」⇒ ⇒ 台账与架构 ADR-6 / §11.2 三处都已登记。
//
// ⚠️ **为什么走 CDN 而不是把库 vendor 进仓**（STAGE-36 §1.3）：
//   vendor 进仓 ⇒ ⇒ 「离线不可用」这个**已知缺口**就不存在了 ⇒ ⇒ 降级路径变成
//   **装饰**（永远走不到）⇒ ⇒ 而用户拍板的语义恰恰是「**离线 / CDN 不可达时
//   优雅降级**」⇒ ⇒ ⇒ 所以走 CDN，**降级是真能走到的**。
//
// ⚠️ **「零构建」仍然成立**：不引入任何打包步骤 ⇒ ⇒ 浏览器**原生**的动态
//   `import()` 就是加载方式 ⇒ ⇒ 加载的是 echarts 的 **ESM** 产物。
//
// ⚠️ **URL 是全仓唯一落点**（STAGE-36 裁决 E）⇒ ⇒ 「第二个来源」要红。

/**
 * echarts 的**唯一**来源字面量（全仓只此一处）。
 *
 * ⚠️ **锁版本**：不写 `@5` / `@latest` ⇒ ⇒ 同一个 URL 永远给同一个产物
 * ⇒ ⇒ 界面上画出来的图不会因为上游发新版而变形。
 */
export const ECHARTS_URL =
    "https://cdn.jsdelivr.net/npm/echarts@5.5.1/dist/echarts.esm.min.js";

/**
 * 「关掉波形库」的哨兵值。
 *
 * ⚠️ **它的用途不是测试后门，是**能力协商**：离线环境、或用户不想为一个图
 *   拉一兆字节的时候 ⇒ ⇒ 界面如实显示「波形库不可用」⇒ ⇒ **通道列表仍在**
 *   ⇒ ⇒ 核心调试闭环（跑 / 断点 / 寄存器 / 反汇编）**不受影响**。
 */
export const ECHARTS_SOURCE_OFF = "off";

/** 当前来源。 */
let source = ECHARTS_URL;

/** 已经加载好的 echarts 模块（没加载好是 `null`）。 */
let instance = null;

/** 正在进行的加载（避免并发时重复发请求）。 */
let pending = null;

/**
 * 当前波形库来源。
 *
 * @returns {string}
 */
export function getChartSource() {
    return source;
}

/**
 * 换波形库来源。
 *
 * ⚠️ **换源会丢掉已加载的模块** ⇒ ⇒ 下一次 `loadECharts()` 会重新加载 ⇒ ⇒
 *   这是「关掉再打开」能**恢复**的原因（STAGE-36 验收 B-31）。
 *
 * @param {string} next —— `"off"` = 关掉；给 URL = 换源；给空 = 回到默认。
 * @returns {string} 换之后的来源。
 */
export function setChartSource(next) {
    const v = String(next ?? "").trim();
    source = v || ECHARTS_URL;
    instance = null;
    pending = null;
    return source;
}

/**
 * 已经加载好的 echarts 模块（**同步**读）。
 *
 * @returns {object|null}
 */
export function getECharts() {
    return instance;
}

/**
 * 加载 echarts。
 *
 * ⚠️ **失败不抛**（STAGE-36 裁决 F）⇒ ⇒ 返回 `null` ⇒ ⇒ 界面走**降级**
 *   ⇒ ⇒ 「波形库不可达」这件事**不会把整条流炸掉**。
 *
 * ⚠️ **为什么用 `Promise.resolve().then(() => import(source))`**：
 *   动态 `import()` 在某些环境里（例：Node 的 ESM 加载器）会**同步**抛
 *   （`ERR_UNSUPPORTED_ESM_URL_SCHEME`）⇒ ⇒ 包一层 then ⇒ ⇒ 那个同步抛
 *   也变成**拒绝** ⇒ ⇒ 被后面的 `.catch` 接住 ⇒ ⇒ 一条路都不漏。
 *
 * ⚠️ **「关掉」优先于缓存**（STAGE-38 · v0.53 批 · 施工期订正 4）：以前
 *   `if (instance) return Promise.resolve(instance)` **排在**「源是 off」
 *   的判断**前面** ⇒ ⇒ 一个**在途的旧加载**在「关掉」之后 resolve 时会把
 *   `instance` 又装回去 ⇒ ⇒ 下一次 `loadECharts()` 就把**已经关掉**的库
 *   返回出去 ⇒ ⇒ 界面从「波形库不可用」**变回**「能画」⇒ ⇒ 真机门禁的
 *   降级路径**偶发**红 ⇒ ⇒ 「关掉」这件事**不生效** ⇒ ⇒ **一个真缺陷**。
 *
 * @returns {Promise<object|null>}
 */
export function loadECharts() {
    // ⚠️ **先看「是不是关掉了」** ⇒ ⇒ 哪怕之前加载成功过，也**不**返回它
    // ⇒ ⇒ 「关掉」是真的关掉（STAGE-36 验收 A-13）。
    if (source === ECHARTS_SOURCE_OFF) {
        instance = null;
        pending = null;
        return Promise.resolve(null);
    }
    if (instance) return Promise.resolve(instance);
    if (pending) return pending;
    // ⚠️ **记下这次请求的来源** ⇒ ⇒ 换源之后这一份结果**作废**
    // ⇒ ⇒ 否则「在途的旧加载」会**复活**已经换掉的源。
    const requested = source;
    pending = Promise.resolve()
        .then(() => import(/* @vite-ignore */ requested))
        .then((mod) => {
            if (source !== requested) {
                // ⚠️ **换过源了** ⇒ ⇒ 这一份是**上一个源**的结果 ⇒ ⇒ 作废
                // （不装进 `instance` ⇒ ⇒ 「关掉」不会被它复活）。
                pending = null;
                return null;
            }
            // ⚠️ echarts 的 **ESM** 产物是**具名导出**（`init` / `use` / `version` …）
            // ⇒ ⇒ **没有** default ⇒ ⇒ 用**命名空间对象**本身。
            instance = mod?.default ?? mod ?? null;
            if (!instance) {
                pending = null;
                return null;
            }
            return instance;
        })
        .catch(() => {
            instance = null;
            pending = null;
            return null;
        });
    return pending;
}
