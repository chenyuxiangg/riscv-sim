// App 层 · 探针 / console / 内存图订阅接线族（从 app.js 抽出）。
//
// 三块接线都闭在函数体内：通道表 / 当前图 / 丢弃计数是函数内局部状态，
// 不外露模块级可变状态；外部依赖仅视图状态（M-03）、公共视图派发辅助、
// 纯函数 lib 与 m02 终端组件。wireDebug 调试面接线耦合调试命令族，留后续批次。
import * as viewStore from "../m03_viewstore.js";
import { publishViewState } from "./shared.js";
import { Command } from "../../lib/message.js";
import { mergeEdges, WAVE_SAMPLE_LIMIT } from "../../lib/wave-samples.js";
import {
    getChartSource,
    loadECharts,
    setChartSource,
} from "../../lib/echarts-loader.js";
import { loadXterm } from "../../lib/xterm-loader.js";
import {
    get as getViewComponent,
    resetTerminalScreen,
    resetTerminalXterm,
} from "../m02_viewcomponents.js";

/**
 * 波形流的 id 前缀：**寄存器**波形流归 M-10（寄存器视图）消费。
 *
 * ⚠️ 判别方向不能反：第 1 阶段那版写的是「只认 `probe.` 开头的」，
 * 而**真内核**的探针通道 id 是 `"{外设 id}.{信号名}"`（M-18 的
 * `drain_waveform`），也就是 `GPIO0.OUT`——它不叫 `probe.*`。写成只认
 * `probe.` 的话，真内核的通道会被**全部过滤掉**，波形视图永远是空的。
 *
 * 所以判别式是「**不是**寄存器流」，而不是「是某种前缀」。
 */
const REGISTER_STREAM_PREFIX = "cpu.";

/**
 * Wire the kernel's probe channels into the waveform view (D-46).
 *
 * 内核侧把探针边沿推成 `EventPayload::WaveformHistory`，**通道列表**和
 * **边沿增量**走的是同一个事件形状（契约冻结，没有单独的「通道列表」
 * 变体）。所以这里：
 *
 * - 通道列表：从边沿事件的 `streams` 里读（订阅时内核会**强制**推一次
 *   空边沿的波形，通道就在里面）；
 * - 边沿计数：**累加**。内核每次推送的是「自上次推送以来」的边沿
 *   （`drain_waveform` 会清空），直接覆盖的话，视图上永远只有最后一批
 *   的边沿数。
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void, subscribeProbeStream: () => Promise<object>}}
 */
export function wireWaveform(commandRouter) {
    /**
     * 累加出来的通道表。内核每次只推**增量**，所以边沿数必须在这里
     * 累加，不能被下一批覆盖。
     *
     * ⚠️ **STAGE-36 · v0.51 批**：条目里**多了一个** `samples`（**能画图的
     * 边沿**）⇒ ⇒ 此前内核推来的边沿**只**被用来累加计数与取末值、**样本本身
     * 被丢掉** ⇒ ⇒ 「真图」缺数据源 ⇒ ⇒ 本批把**样本留住**。
     *
     * ⚠️ **两个数不是同一个数**（裁决 B）：
     *   - `edges`   = **边沿总数**（内核口径，累加，**不受**保留上限影响）；
     *   - `samples` = **保留下来能画图的边沿**（每通道最近 `WAVE_SAMPLE_LIMIT` 条）。
     *
     * @type {Map<string, {id: string, label: string, edges: number, samples: Array<{t:number,v:number}>, lastValue: number|null}>}
     */
    const channels = new Map();

    /**
     * 波形图的**状态**（STAGE-36 · 裁决 H）。
     *
     * ⚠️ 住**视图状态** `wave.chart` ⇒ ⇒ 视图层**纯渲染**（按 `status` 分支）
     * ⇒ ⇒ app 层**写**、视图层**只读** ⇒ ⇒ 分层不破。
     *
     * @param {"idle"|"loading"|"ok"|"degraded"} status
     * @param {string} [reason]
     */
    function setChartState(status, reason) {
        viewStore.setState("waveform", (prev) => ({
            ...prev,
            wave: {
                ...(prev.wave ?? {}),
                chart: { status, ...(reason ? { reason } : {}) },
            },
        }));
        publishViewState("waveform");
    }

    /**
     * **懒加载**波形库（裁决 E）。
     *
     * ⚠️ **为什么懒**：echarts 的 ESM 产物约一兆 ⇒ ⇒ 启动就拉 ⇒ ⇒ 首屏白等
     * ⇒ ⇒ 只在**真的有边沿可画**的时候才拉。
     *
     * ⚠️ **失败不抛** ⇒ ⇒ 状态落 `degraded` ⇒ ⇒ 界面显示「波形库不可用」
     * ⇒ ⇒ **通道列表仍在**（裁决 F）⇒ ⇒ 核心调试闭环不受影响。
     */
    function ensureWaveChartLoaded() {
        const status = viewStore.getState("waveform")?.wave?.chart?.status;
        if (status === "ok" || status === "loading") return;
        setChartState("loading");
        loadECharts().then((mod) => {
            setChartState(
                mod ? "ok" : "degraded",
                mod ? undefined : "波形库不可用",
            );
        });
    }

    commandRouter.subscribe("waveform.history", (event) => {
        const streams = (event.value?.streams ?? []).filter(
            (s) => !String(s.id ?? "").startsWith(REGISTER_STREAM_PREFIX),
        );
        if (streams.length === 0) return;
        for (const stream of streams) {
            const id = String(stream.id ?? "");
            if (!id) continue;
            const prev = channels.get(id) ?? null;
            // ⚠️ **累加与截断是纯函数**（裁决 C）⇒ ⇒ 只读 `prev`、只写返回值
            // ⇒ ⇒ 视图状态里那份**不被改**。
            const merged = mergeEdges(prev, stream, WAVE_SAMPLE_LIMIT);
            // ⚠️ **「最新值」是通道列表一直在用的字段** ⇒ ⇒ 它**不是**样本里的
            // 东西 ⇒ ⇒ 这里单独从原始流取 ⇒ ⇒ 不动 `mergeEdges` 的契约。
            const raw = Array.isArray(stream.edges) ? stream.edges : [];
            merged.lastValue = raw.length > 0
                ? (raw[raw.length - 1].value ?? prev?.lastValue ?? null)
                : (prev?.lastValue ?? null);
            channels.set(id, merged);
        }
        viewStore.setState("waveform", (prev) => ({
            ...prev,
            wave: {
                ...(prev.wave ?? {}),
                channels: [...channels.values()],
            },
        }));
        publishViewState("waveform");
        // ⚠️ **有边沿了才去拉波形库**（懒加载 ⇒ ⇒ 启动不拉那一兆）。
        ensureWaveChartLoaded();
    });

    return {
        /** 新会话：旧的通道不算数（FR-13 —— 每个 session 独立命名空间）。 */
        reset() {
            channels.clear();
            viewStore.setState("waveform", (prev) => ({
                ...prev,
                wave: { ...(prev.wave ?? {}), channels: [] },
            }));
            publishViewState("waveform");
        },
        /**
         * 订阅内核侧的探针流。
         *
         * 必须是**强制**的：`LoadProject` 清掉了内核的主题集合，不重订
         * 的话探针推送会**静默停掉**（和 D-45 是同一个坑）。
         *
         * @returns {Promise<object>}
         */
        async subscribeProbeStream() {
            return commandRouter.dispatchCommand(Command.subscribe("sim.waveform"));
        },
        /**
         * 换波形库来源并**重新加载**（STAGE-36 · 裁决 G）。
         *
         * ⚠️ **为什么要有它**：降级路径**必须可达** ⇒ ⇒ 不能只靠「碰巧没网」
         * ⇒ ⇒ 真机门禁**主动**触发降级 ⇒ ⇒ **降级逻辑永远有人管** ⇒ ⇒
         * 不能只靠「碰巧没网」。
         *
         * @param {string} next —— `"off"` = 关掉；给 URL = 换源；给空 = 回到默认。
         * @returns {string} 换之后的来源。
         */
        setChartSource(next) {
            const source = setChartSource(next);
            setChartState("loading");
            loadECharts().then((mod) => {
                setChartState(
                    mod ? "ok" : "degraded",
                    mod ? undefined : "波形库不可用",
                );
            });
            return source;
        },
        /** 当前波形库来源。 */
        chartSource() {
            return getChartSource();
        },
        /**
         * 各通道的**计数快照**（脚本口用）。
         *
         * ⚠️ **为什么要有它**（STAGE-36 · 判别力）：真机门禁要拿
         *   「**图上画了几个点**」与「**视图状态里留了几条样本**」比对 ⇒ ⇒
         *   两者必须是**两个独立来源** ⇒ ⇒ ⇒ 只给**数字**、不给样本本身 ⇒ ⇒
         *   门禁读到的是**状态**、不是界面自己的说法。
         *
         * ⚠️ **`edges` 与 `samples` 是两个数**（裁决 B）⇒ ⇒ 两个都报 ⇒ ⇒
         *   「图被截断了」这件事在真机上也看得见。
         *
         * @returns {Array<{id:string,label:string,edges:number,samples:number}>}
         */
        channelStats() {
            return Array.from(channels.values()).map((c) => ({
                id: c.id,
                label: c.label,
                edges: c.edges,
                samples: c.samples.length,
            }));
        },
    };
}

/**
 * 终端 xterm 库的状态（住视图状态 `term.xterm.status` ⇒ 重渲染不丢）。
 *
 * @param {"idle"|"loading"|"ok"|"degraded"} status
 * @param {string} [reason]
 */
function setTerminalXtermStatus(status, reason) {
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: {
            ...(prev.term ?? {}),
            xterm: { ...(prev.term?.xterm ?? {}), status, ...(reason ? { reason } : {}) },
        },
    }));
    publishViewState("terminal");
}

/**
 * **懒加载**终端仿真库（与 `ensureWaveChartLoaded` 同形）。
 *
 * ⚠️ **失败不抛** ⇒ 状态落 `degraded` ⇒ 视图退回自研屏幕仿真器降级路径，
 *   核心调试闭环不受影响。Node 门禁里加载器直接返回 `null` ⇒ 不碰网络。
 */
export function ensureTerminalXtermLoaded() {
    const status = viewStore.getState("terminal")?.term?.xterm?.status;
    // ⚠️ `degraded` 也挡重入：失败这一轮会话内不再重试（刷新页面才重拉）⇒
    //   否则「落 degraded ⇒ 发布渲染 ⇒ 渲染监听再触发加载」会成死循环。
    if (status === "ok" || status === "loading" || status === "degraded") return;
    setTerminalXtermStatus("loading");
    loadXterm().then((mod) => {
        setTerminalXtermStatus(
            mod ? "ok" : "degraded",
            mod ? undefined : "终端库不可用",
        );
    });
}

/**
 * Wire the guest's **console** byte stream (contract v4's `Console` event)
 * into the terminal view.
 *
 * ⚠️ **console 是流，不是命令的回复**：内核侧**订阅了** `sim.console`
 * 才会推（`console_event()` 没订阅就返回 `None`）⇒ 前端必须先订阅。
 *
 * ⚠️ **载荷是字节数组**（`bytes: [104, 105]`），不是字符串 ⇒ 解码交给
 * `terminalView.onDelta`（分块展开，几万个字节也不会爆调用栈）。
 *
 * ⚠️ **主题字面量**：`"sim.console"` 必须与 `front/lib/message.js` 的
 * `EVENT_TOPIC.Console` **逐字一致**（唯一真相在那儿；这里是使用方，
 * 与 `wireWaveform` 的 `"waveform.history"` 同一个口径）。
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void, subscribeConsoleStream: () => Promise<object>}}
 */
export function wireConsole(commandRouter) {
    commandRouter.subscribe("sim.console", (event) => {
        const bytes = event.value?.bytes;
        if (!Array.isArray(bytes) || bytes.length === 0) return;
        const terminal = getViewComponent("terminal");
        if (!terminal) return;
        // 收到第一路串口字节 ⇒ 懒触发终端库加载（已在加载 / 就绪则直接返回）。
        ensureTerminalXtermLoaded();
        // ⚠️ 走**既有的** `onDelta` 路径：它把字节追加进 store（唯一权威）
        // 并用 `textContent` 写 DOM（**不解析 HTML** ⇒ ⇒ ANSI 转义与 `<` `&`
        // 都是安全的）。这条路径此前是**死接口**（`termDelta` 没有生产者）。
        terminal.onDelta({ termDelta: { bytes } });
    });

    return {
        /**
         * 新程序 ⇒ 上一个程序打到串口上的字节不算数（与 D-45 同一个坑：
         * 状态必须跟着工程一起重建）。
         */
        reset() {
            viewStore.setState("terminal", (prev) => ({
                ...prev,
                term: { ...(prev.term ?? {}), bytes: "" },
            }));
            // ⚠️ **屏幕缓存也要一起清**（STAGE-41 · v0.56）：终端视图那边
            //   有一份**增量**的屏幕实例 ⇒ ⇒ 字节流清了而它没清 ⇒ ⇒
            //   下一个程序的字节会被**接在上一段工程的屏幕后面**⇒ ⇒ 显示错乱。
            resetTerminalScreen();
            // ⚠️ xterm 实例同样要清空（直输模式的持久宿主）⇒ ⇒ 与上面成对。
            resetTerminalXterm();
            publishViewState("terminal");
        },
        /**
         * 订阅内核侧的 console 流。
         *
         * 必须是**强制**的：`LoadProject` 清掉了内核的主题集合，不重订
         * 的话 console 推送会**静默停掉**（和 D-45 / D-46 是同一个坑）。
         *
         * @returns {Promise<object>}
         */
        async subscribeConsoleStream() {
            return commandRouter.dispatchCommand(Command.subscribe("sim.console"));
        },
    };
}

/**
 * Wire the kernel's **board memory map** (contract v6's `MemoryMap` event,
 * **v7 added `loaded` / `revision`**) into the product view's M-14 table and
 * the board-memory-map editor.
 *
 * ⚠️ **唯一缓存**（规格 §3.3 / 裁决 F）：「当前生效的图」只有**一份** ⇒
 * 产品视图那张 M-14 内存映射表与板级内存图编辑器拿到的是**同一个对象
 * 引用**（不是两份拷贝）⇒「两个视图显示的是不是同一张图」这件事是**结构上**
 * 成立的，不是靠两边各写一遍同步逻辑维持的。
 *
 * ⚠️ **事件载荷是「当前生效的图」**（不是「用户配的那份」）⇒ 表格显示的
 * 就是内核此刻真正在用的那张图；「待生效」由事件里的 `pending` 标志给出
 * （宿主的 `pending` 是**推导量**：用户的板 ≠ 当前生效的图）。
 *
 * ⚠️ **`revision` 也住在这份缓存里**（v0.31 · 裁决 A）⇒ 提交时原样作为
 * `expected_revision` 带上 ⇒ 并发丢更新**看得见**（`E_PLT-13`）。
 *
 * ⚠️ **主题字面量**：`"memory.map"` 必须与 `front/lib/message.js` 的
 * `EVENT_TOPIC.MemoryMap` 逐字一致（与 `wireConsole` 同一个口径）。
 *
 * ⚠️ **乱序护栏**（STAGE-17 · v0.32 · D-146 · 裁决 F / G）：事件**乱序
 * 到达**时（迟到的旧事件），**更旧**的那份**不许覆盖**较新那份 ⇒ 状态留在
 * 较新那份 ⇒ 差量**算在较新的那张图上**（而不是算在旧图上）。
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void, subscribeMemoryMapStream: () => Promise<object>, get: () => object|null, discardedCount: () => number}}
 */
export function wireMemoryMap(commandRouter) {
    /**
     * **当前生效**的图（`{description, pending, loaded, revision}`）——唯一的那一份。
     *
     * ⚠️ **`revision` 住在这个对象里**（裁决 A · v0.31）：它是内核事件带来的
     * ⇒ 与「当前生效的图」**同一份事实** ⇒ **不另设**一个可以独立翻转的
     * 标志位 ⇒ 提交时原样作为 `expected_revision` 带上。
     *
     * @type {{description: object|null, pending: boolean, loaded: boolean, revision: number}|null}
     */
    let current = null;

    /**
     * **乱序丢弃计数**（STAGE-17 · D-146 · 裁决 F：「丢弃要可观测」）。
     *
     * ⚠️ **只记数、不记内容** ⇒ 它**不是**「当前生效的图」的第二份副本
     * ⇒ 不构成第二个真相源。
     *
     * ⚠️ **只增不减**：`reset()`（新会话）**不**清它 ⇒ 清掉等于把「丢弃
     * 发生过」的证据悄悄抹掉 ⇒ 「不静默」这条承诺就破了。
     *
     * @type {number}
     */
    let discardedOutOfOrder = 0;

    commandRouter.subscribe("memory.map", (event) => {
        const next = {
            description: event.value?.description ?? null,
            pending: event.value?.pending === true,
            loaded: event.value?.loaded === true,
            revision: Number(event.value?.revision ?? 0),
        };
        // ⚠️ **乱序护栏**（裁决 F）：`revision` **严格更旧**的事件 ⇒ **不覆盖**
        // 已有状态 ⇒ 状态留在较新那份 ⇒ 差量**算在较新的那张图上**。
        //
        // ⚠️ **判据是「严格更旧」不是「不等」**（裁决 G）：`UnloadProject`
        // 清订阅 ⇒ 前端重订 ⇒ 内核**立即**推一次当前图 ⇒ 那次推送的
        // `revision` 与手上那份**相同**（卸载**不改图** ⇒ 修订号不 +1）⇒
        // 「不等就挡」会让卸载后的前端**停在旧状态**。
        //
        // ⚠️ **护栏不改提交路径的其它行为**（裁决 H）：基底被判为旧时，提交
        // **照常**发命令 ⇒ 内核的 `expected_revision` 检查**照常兜底**。
        if (current !== null && next.revision < current.revision) {
            discardedOutOfOrder += 1;
            return;
        }
        current = next;
        // ⚠️ **同一个引用**写进两个视图状态 ⇒ 两个视图不可能显示不同的图。
        viewStore.setState("product", (prev) => ({ ...prev, memoryMap: current }));
        publishViewState("product");
    });

    return {
        /**
         * 新会话：上一个会话的内存图不算数（与 D-45 / D-46 同一个坑：状态
         * 必须跟着工程一起重建）。
         */
        reset() {
            current = null;
            viewStore.setState("product", (prev) => ({ ...prev, memoryMap: current }));
            publishViewState("product");
        },
        /**
         * 订阅内核侧的内存图流。
         *
         * 必须是**强制**的：`LoadProject` 清掉了内核的主题集合 ⇒ 不重订就
         * **静默**停在旧图上（和 D-45 / D-46 是同一个坑）。
         *
         * @returns {Promise<object>}
         */
        async subscribeMemoryMapStream() {
            return commandRouter.dispatchCommand(Command.subscribe("memory.map"));
        },
        /** 当前缓存（测试用：断言两个视图拿到的是**同一个**对象）。 */
        get() {
            return current;
        },
        /**
         * **被丢弃的乱序（旧）事件数**（STAGE-17 · D-146 · 裁决 F）。
         *
         * ⚠️ **不静默**靠**这个数** ⇒ 判定不靠文案、靠**数**。
         *
         * ⚠️ **不**随 `reset()` 清零（见上面那条注释：清掉等于抹掉证据）。
         *
         * @returns {number}
         */
        discardedCount() {
            return discardedOutOfOrder;
        },
    };
}
