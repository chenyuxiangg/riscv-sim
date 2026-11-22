// App 层 · M-11 调试面接线（断点三函数 / wireDebug 订阅与停下续发 / 脚本命令口）。
//
// ⚠️ 单向依赖：视图状态（M-03）、公共视图派发辅助、debug-state 基础层、
// run-state 运行态族与纯函数 lib；不回引门面 ⇒ ⇒ 不形成子模块 ↔ 门面循环。

import * as viewStore from "../m03_viewstore.js";
import { publishViewState } from "./shared.js";
import { updateDebugState, writeErrorSlot } from "./debug-state.js";
import {
    getRunState,
    setRunState,
    bumpRunBusy,
    transitionRunState,
    performReadback,
} from "./run-state.js";
import {
    setPausePending,
    consumePausePending,
    getPendingErrorSlot,
    setPendingErrorSlot,
} from "./state.js";
import { Command } from "../../lib/message.js";
// ⚠️ `kind` 的归一化只有一处（视图层那份）⇒ ⇒ app 层引同一处，不另写一套。
import { normalizeBreakpointKind } from "../m02_viewcomponents.js";
import {
    HALT_REASON_END_OF_EXECUTION,
    RUN_ACTION_CONTINUE,
    nextRunAction,
    runStateAfter,
    stopRunState,
} from "../../lib/run-loop.js";
import { READBACK_REASON, readbackReasonOf } from "../../lib/readback.js";

/**
 * 拉一次**内核断点表**（STAGE-55）。
 *
 * ⚠️ **列表只有一份真相**：`debug.bpEntries` 是 `ListBreakpoints` /
 *   `SetBreakpointEnabled` 回执的**原样**条目 ⇒ ⇒ 谁改了内核的表，谁就
 *   拉一次回来 ⇒ ⇒ 前端**不再**自己拼「我请求过哪些断点」（那是第二份
 *   真相，谓词条目一多就必然对不上：它们没有地址，也不占地址槽）。
 *
 * ⚠️ **注册失败时不拉**：命令抛错 ⇒ ⇒ 内核表**没变** ⇒ ⇒ 界面也**不该**
 *   变（沿用「失败不留假断点」的既有判据）。
 *
 * @param {object} commandRouter — M-05
 * @returns {Promise<void>}
 */
export async function refreshBreakpoints(commandRouter) {
    await commandRouter.dispatchCommand(Command.listBreakpoints());
}

/**
 * Wire the M-11 debug surface (disassembly / trace export / trace switch)
 * into the debug view.
 *
 * The four events (`Disasm` / `SourceLocation` / `TraceExport` /
 * `TraceState`) are **reply events** (STAGE-5 §9.1): the kernel sends
 * them straight back to the requester, correlated by the transport
 * frame's `reqId` — they are **not** pushed streams, so there is
 * nothing to subscribe to on the kernel side.
 *
 * The router still publishes every event to its topic subscribers, so
 * the debug view picks them up by topic exactly like M-10 does with
 * `sim.state.delta`.
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void}}
 */
/**
 * M-11 调试面的接线（反汇编 / trace 导出 / trace 开关 / 停下后的**自动读回**）。
 *
 * ⚠️ **`services` 是可选的**（v0.67 批）⇒ ⇒ 旧调用点（只传 `commandRouter`）
 *   **照样能链接**⇒ ⇒ 最坏是「**没有自动读回**」（降级）⇒ ⇒ **不会白屏**。
 *
 * @param {object} commandRouter — M-05
 * @param {{registerMemory?: object|null}} [services]
 * @returns {object}
 */
export function wireDebug(commandRouter, services = {}) {
    /** Push the disassembly entries into the debug view's state. */
    commandRouter.subscribe("debug.disasm", (event) => {
        const entries = Array.isArray(event.value?.entries) ? event.value.entries : [];
        viewStore.setState("debug", (prev) => ({
            ...prev,
            sim: { ...(prev.sim ?? {}), disasm: entries },
        }));
        publishViewState("debug");
    });

    /** Push the export block into the debug view's state. */
    commandRouter.subscribe("debug.export", (event) => {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), export: event.value ?? null },
        }));
        publishViewState("debug");
    });

    /** Push the trace recording state into the debug view's state. */
    commandRouter.subscribe("debug.trace", (event) => {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), trace: event.value ?? null },
        }));
        publishViewState("debug");
    });

    /**
     * 内核断点表的**只读回执**（契约 v13 · `debug.breakpoints`）。
     *
     * ⚠️ 这是断点列表的**唯一数据源**：`ListBreakpoints` 与
     * `SetBreakpointEnabled` 的回复都走这里 ⇒ ⇒ 前端不再自己猜「内核有
     * 哪些断点」。
     */
    commandRouter.subscribe("debug.breakpoints", (event) => {
        const entries = Array.isArray(event.value?.breakpoints) ? event.value.breakpoints : [];
        // ⚠️ **一条一条原样收**（STAGE-55）：id 是内核给的句柄（谓词条目
        //   没有地址，只有 id），`kind` 是来源标记，`condition` 是表达式
        //   原文 ⇒ ⇒ 列表的**成员与内容**都来自这里，前端不添油加醋。
        const items = entries.map((item) => {
            const bp = item?.breakpoint ?? item ?? {};
            return {
                id: Number(bp.id ?? 0) >>> 0,
                addr: Number(bp.addr ?? 0) >>> 0,
                kind: normalizeBreakpointKind(bp.kind),
                condition: bp.condition ?? null,
                symbol: bp.source_hint ? String(bp.source_hint) : null,
                enabled: bp.enabled !== false,
                hits: Number(item?.hit_count ?? 0),
                degraded: bp.degraded === true,
            };
        });
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), bpEntries: items },
        }));
        publishViewState("debug");
    });

    /**
     * The kernel's source-location reply (M-11's DWARF line table).
     *
     * ⚠️ `SourceLocation` 是**回复事件**（不是推送流）⇒ 内核侧没有可
     * 订阅的东西，但路由器会把每个事件发布给 topic 订阅者 ⇒ 源码视图
     * 按 topic 接住它（与 `debug.disasm` 同一条路）。
     *
     * ⚠️ **两种「没有答案」要能区分**：`line === 0` = 根本没有 DWARF；
     * `line === 0xFFFFFFFF` = 有 DWARF 但地址没命中行表。视图**如实**
     * 区分这两种，不显示错误的行号。
     */
    /**
     * 观察表达式轮询回执（契约 v14 · `debug.watch`）。
     * 值变化由内核计算，前端只显示。
     */
    commandRouter.subscribe("debug.watch", (event) => {
        const values = Array.isArray(event.value?.values) ? event.value.values : [];
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), watchValues: values },
        }));
        publishViewState("debug");
    });

    commandRouter.subscribe("debug.source", (event) => {
        viewStore.setState("source", (prev) => ({
            ...prev,
            source: {
                ...(prev.source ?? {}),
                location: event.value ?? null,
                // 换了一个地址 ⇒ 之前选定的候选**不再适用**。
                selected: null,
            },
        }));
        publishViewState("source");
    });

    /**
     * The kernel's **halt report** (the `Halted` event).
     *
     * ⚠️ **这是「停下」的**唯一落点**（STAGE-35 · v0.50 批 · 裁决 L）——
     * 脚本口 `window.riscv_kernel.lastHalt()` 与调试视图的「命中断点」提示
     * **读同一份** ⇒ ⇒ **不订阅两次**（那会是两份状态 ⇒ ⇒ 第二个真相）。
     *
     * ⚠️ **`reason === "Breakpoint"`** 是「命中断点」的唯一依据（裁决 M）
     * ⇒ ⇒ 前端**不自己判断**「跑没跑到那个地址」。
     *
     * ⚠️ **复位时保留**（与 `debug.trace` / `debug.error` 同一条规矩）：
     * 它是**内核最后一次的报告**，不是程序数据 ⇒ ⇒ 跨复位保留。
     */
    commandRouter.subscribe("halted", (event) => {
        const reason = event.value?.reason ?? null;
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                halt: {
                    reason: event.value?.reason ?? null,
                    pc: Number(event.value?.pc ?? 0) >>> 0,
                },
            },
        }));
        // ⚠️ **连续运行的续发落点**（STAGE-37 · v0.52 批 · 裁决 C）。
        //
        // ⚠️ **落在** `halted` **这一个订阅上** ⇒ ⇒ 不新订阅一次 ⇒ ⇒
        // 「内核的停下报告」**仍然只有一份** ⇒ ⇒ 不是第二个真相。
        //
        // ⚠️ **必须由订阅驱动**（不能写成 `await` 循环）—— 命令的 promise
        // 只 resolve **第一个**带 reqId 的事件（那是 `Tick`）⇒ ⇒ `Halted`
        // **只能**从订阅拿到（F8）。
        //
        // ⚠️ **判据在纯函数里**（`nextRunAction`）⇒ ⇒ 「该不该续」这件事
        // Node 侧**零 DOM** 就能验 ⇒ ⇒ 这里只是一次**调用**。
        const run = getRunState();
        const action = nextRunAction({ mode: run.mode, reason });
        // ⚠️ **在途计数 −1**（STAGE-38 · v0.53 批 · 裁决 B）⇒ ⇒ 一次执行
        // ⇒ ⇒ 一次停下报告 ⇒ ⇒ 这里**就是**那个退休点（**只有一处** ⇒ ⇒
        // 不是第二个真相）。
        const next = runStateAfter(run, reason);
        const busy = Math.max(0, next.busy - 1);
        if (action === RUN_ACTION_CONTINUE) {
            // ⚠️ ⚠️ **先减后加在同一个值里算完**（裁决 C · 施工期订正 2）
            // ⇒ ⇒ 「内核让出控制权」的那一瞬间计数**不**会掉到 0 ⇒ ⇒
            // 界面**不会**把活值闪出来一下 ⇒ ⇒ **顺序是硬约束**。
            // ⚠️ **「跑过没有」那一格带上**（`run-loop.js` 只搬三格）。
            setRunState({ ...next, busy: busy + 1, started: run.started === true });
            // ⚠️ **「用户要的那次停」作废**（v0.67 批）⇒ ⇒ 用户点过暂停**又**
            //   点了运行 ⇒ ⇒ 那个「等一次停」的意图**已经被覆盖**⇒ ⇒ 不能留着
            //   去匹配**后面**某次停下 ⇒ ⇒ 否则会冒出来一次**没人要**的读回。
            setPausePending(false);
            // ⚠️ **不 await** ⇒ ⇒ 这个订阅回调**不等**内核 ⇒ ⇒ 事件处理
            // 不会卡住 ⇒ ⇒ 错误也不往这里抛（抛了就是**未处理的拒绝**）。
            void Promise.resolve(commandRouter.dispatchCommand(Command.continueRun()))
                .catch(() => {
                    // ⚠️ **续发失败 ⇒ 退出连续模式** ⇒ ⇒ 界面不再显示
                    // 「在跑」⇒ ⇒ 状态位不会骗人。
                    //
                    // ⚠️ **计数也要退回来**⇒ ⇒ 否则内核**根本没跑**而计数说
                    // 「在跑」⇒ ⇒ 活值会**永远**藏着 ⇒ ⇒ 界面在骗人。
                    bumpRunBusy(-1);
                    transitionRunState(stopRunState(getRunState()));
                });
        } else {
            // ⚠️ **真停**（断点 / 观察点 / 陷阱 / 用户暂停）⇒ ⇒ 退出连续模式
            // ⇒ ⇒ **不续发** ⇒ ⇒ 连续模式在断点后**不会**继续跑
            // ⇒ ⇒ 活值**恢复显示**（`isRunning()` 变成假）。
            // ⚠️ **「跑过没有」那一格带上**（`run-loop.js` 只搬三格）。
            setRunState({ ...next, busy, started: run.started === true });
            // ⚠️ ⚠️ **自动读回**（v0.67 批）⇒ ⇒ 程序**真停**在这儿 ⇒ ⇒ 把
            //   用户正在看的那几样东西**重新取回来**（寄存器 + 内存单元 +
            //   转储）⇒ ⇒ **内存单元表跟着刷新**（用户报的那个现象）。
            //
            // ⚠️ **只在「真停」这一支里做** ⇒ ⇒ 连续运行里**每个窗口**都会来的
            //   那次 `EndOfExecution` **不会**触发 ⇒ ⇒ 真机门禁的脚本口**不会**
            //   因为每个批次多发两条命令而**慢到不可用**。
            //
            // ⚠️ **判据在纯函数里**（`shouldAutoReadback`）⇒ ⇒ Node 侧**零
            //   DOM** 就能验 ⇒ ⇒ 这里只是一次**调用**。
            const stopReason = readbackReasonOf(reason);
            // ⚠️ **用户点过暂停** ⇒ ⇒ 这次 `EndOfExecution` **就是**用户要的
            //   那次停（暂停**只停续发**，正在跑的那一个窗口跑完才停）⇒ ⇒ 也
            //   该读回。
            const pauseHit = reason === HALT_REASON_END_OF_EXECUTION && consumePausePending();
            if (stopReason !== null || pauseHit) {
                // ⚠️ **不 await** ⇒ ⇒ 这个订阅回调**不等**内核 ⇒ ⇒ 事件处理
                // 不会卡住 ⇒ ⇒ 错误也不往这里抛（抛了就是**未处理的拒绝**）。
                void performReadback(
                    // ⚠️ **带上 `commandRouter`** ⇒ ⇒ 「读这一段」的转储结果要
                    //   **重发一条** `DumpMemory` ⇒ ⇒ 而那条命令**只能**从这里
                    //   发 ⇒ ⇒ `wireDebug` 自己手里就有它 ⇒ ⇒ 不必让调用方
                    //   多传一份。
                    { ...services, commandRouter },
                    stopReason ?? READBACK_REASON.USER_PAUSE,
                    { auto: true },
                ).catch(() => {
                    // ⚠️ 读回失败**已经写进** `debug.readback.error` ⇒ ⇒ 这里
                    // 只是**不让它变成未处理的拒绝** ⇒ ⇒ 不吞（界面看得见）。
                });
            }
        }
        publishViewState("debug");
    });

    /**
     * The kernel's error replies (M-05's error path — the `Error` event
     * is published to the `error` topic **and** rejects the pending
     * command). The debug view shows the last one; there is no second
     * error channel.
     *
     * ⚠️ **契约 v9**：内核的错误**还要**按位显示（`E_DBG-02` ⇒ 条件那一位 /
     * 符号那一位；`E_DBG-03` ⇒ 转储那一位）⇒ ⇒ **写的是内核原文**
     * （`event.value.message`）⇒ ⇒ **不吞、不改写**。
     */
    commandRouter.subscribe("error", (event) => {
        const code = event.value?.code ?? "E_PROTO-08";
        const message = event.value?.message ?? "";
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                error: {
                    code,
                    message,
                },
            },
        }));
        // ⚠️ **按位显示**：谁发的命令 ⇒ 错误落在谁那一位 ⇒ ⇒ 一次错误
        //   **消费掉**这个标记 ⇒ ⇒ 不会串位。
        const slot = getPendingErrorSlot();
        setPendingErrorSlot(null);
        if (slot) writeErrorSlot(slot, message);
        publishViewState("debug");
    });

    /**
     * 符号查询的**答复**（契约 v9 · `debug.symbol`）。
     *
     * ⚠️ **状态写入必须在订阅回调里** ⇒ ⇒ **不是**在 `await
     *   dispatchCommand(...)` 之后 ⇒ ⇒ M-05 的 `handleFrame` 是**先
     *   resolve 命令的 promise、再 publish 事件** ⇒ ⇒ 「await 回来」比
     *   「订阅回调」**更晚** ⇒ ⇒ 写在 await 之后会落在错的那一格上
     *   （既有 `case "disasm"` 就是这个模式）。
     *
     * ⚠️ **这是一次查询的答复**（推导量）⇒ ⇒ 住视图状态、**不持久化**
     *   ⇒ ⇒ 不是内核状态的第二份副本。
     */
    commandRouter.subscribe("debug.symbol", (event) => {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), symbol: event.value ?? null },
        }));
        publishViewState("debug");
    });

    /**
     * 整段转储的**答复**（契约 v9 · `debug.dump`）。
     *
     * ⚠️ **落点在 `debug` 视图状态**（规格 §5.2：`debug.dump` · v0.65 批随
     *   「寄存器 / 内存」视图并进调试视图而改落点）⇒ ⇒ **不**另存一份 ⇒ ⇒
     *   调试视图「内存窗口」显示的**就是**这一份。
     *
     * ⚠️ **同样在订阅回调里写**（理由同 `debug.symbol`）。
     */
    commandRouter.subscribe("debug.dump", (event) => {
        const addr = Number(event.value?.addr ?? 0) >>> 0;
        const bytes = Array.isArray(event.value?.bytes) ? event.value.bytes : [];
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                dump: { addr, bytes: bytes.map((b) => Number(b) & 0xff) },
            },
        }));
        publishViewState("debug");
    });

    return {
        /**
         * New session: the disassembly / export data of the **previous**
         * program does not count.
         *
         * ⚠️ The trace **switch** state and the last error are **kept**:
         * they are the kernel's last *report*, not program data — the
         * front end does not second-guess the kernel about them.
         */
        reset() {
            viewStore.setState("debug", (prev) => ({
                ...prev,
                sim: { ...(prev.sim ?? {}), disasm: [] },
                // ⚠️ **符号查询的答复也清掉**（契约 v9）⇒ ⇒ 它是**上一个程序**
                //   的符号 ⇒ ⇒ 留着就是一份**错**的答案。
                //
                // ⚠️ **转储结果也清掉**（同一个道理 ⇒ ⇒ 它是**上一个程序**的
                //   内存）⇒ ⇒ 与符号**同一笔**写、**同一拍**发布 ⇒ ⇒ 视图
                //   **不会**看见「一半清了、一半没清」的中间态。
                debug: {
                    ...(prev.debug ?? {}),
                    export: null,
                    symbol: null,
                    dump: null,
                    // ⚠️ **断点 / 符号标签也必须清**：内核在 `LoadProject` /
                    //   `UnloadProject` 里调 `clear_debug_state()`，但契约
                    //   **没有**「列出断点」的事件 ⇒ ⇒ 前端不在这里清，就会
                    //   保留上一程序的假断点（STAGE-55 之后列表来自内核
                    //   回执，这一条仍然成立：新工程还没拉过表）。
                    bpEntries: [],
                    bpError: "",
                    watchValues: [],
                    watchNextId: 1,
                    // ⚠️ **读回的那一份也清掉**（v0.67 批）⇒ ⇒ 基准与「上次
                    //   读回」说的都是**上一个程序**⇒ ⇒ 留着就是一份**不存在的**
                    //   对比⇒ ⇒ 用户会看到**上一趟**的高亮。
                    readback: null,
                },
            }));
            // ⚠️ 断点的**草稿**住在共享的 `debugger` 键里（D-119）⇒ ⇒ 新程序
            //   的地址 / 表达式都不再指同一个东西 ⇒ ⇒ 一并清掉（否则框里还
            //   挂着一个**上一条程序**的表达式，用户一按回车就撞墙）。
            viewStore.setState("debugger", (prev) => ({
                ...prev,
                bpKind: "Address",
                bpDraft: "",
                bpTabIndex: -1,
            }));
            publishViewState("debug");
            // ⚠️ **「用户要的那次停」也作废**（v0.67 批）⇒ ⇒ 复位之后正在跑的
            //   那个窗口**不算**用户要的那次停⇒ ⇒ 否则复位之后会冒出来一次
            //   **没人要**的读回。
            setPausePending(false);
            // ⚠️ **源码树留着**，但**行号**要清掉：行号是**上一个程序**的
            // 数据（新工程 = 新程序 ⇒ 旧行号是错的）。源码树是**用户装的
            // 资产**，与程序无关 ⇒ 不能跟着新工程一起没掉（STAGE-7 验收
            // A-20：装载工程后源码树仍在）。
            viewStore.setState("source", (prev) => ({
                ...prev,
                source: { ...(prev.source ?? {}), location: null, selected: null },
            }));
            publishViewState("source");
        },
    };
}

/**
 * 把一组「**脚本可调用**」的命令暴露给页面上的脚本。
 *
 * ⚠️ **这不是产品 UI**：真实 RTOS 的复验脚本（`tools/browser-verify.mjs`）
 * 需要「发一条命令 ⇒ 拿到它的回复」这一段能力——而前端的 UI 里**没有**
 * 「设置断点」或「读 UART 寄存器」的入口（那是 P1 的活）。
 *
 * ⚠️ **只暴露命名清楚的窄接口**，不把整个路由器塞给脚本 ⇒ 脚本能做的事
 * 是**看得见**的（每加一个方法都是一次显式的决定）。
 *
 * @param {object} commandRouter
 * @returns {void}
 */
export function exposeScriptCommands(commandRouter) {
    /**
     * 发一条命令，等它的回复事件 ⇒ 返回回复事件的 `value`。
     *
     * @param {object} payload — `CommandPayload` 的线上形态
     * @param {number} [timeoutMs]
     * @returns {Promise<object|null>}
     */
    async function dispatch(payload, timeoutMs) {
        const event = await commandRouter.dispatchCommand(
            payload,
            timeoutMs ? { timeoutMs } : {},
        );
        return event?.value ?? null;
    }

    /**
     * 最近一次「**停下**」的报告（契约的 `Halted` 事件）。
     *
     * ⚠️ **为什么要有它**：命令的**回复**是「带同一个 reqId 的第一个
     * 事件」——那可能是 `Tick`、也可能是 `Console` ⇒ 从回复里**读不出**
     * 「为什么停、停在哪儿」。⇒ `Halted { reason, pc }` 由 `wireDebug`
     * 记进**视图状态** `debug.halt`（STAGE-35 · v0.50 批 · 裁决 L）。
     *
     * ⚠️ **这里只读，不再订阅一次** ⇒ ⇒ 脚本口与调试视图**读同一份**
     * ⇒ ⇒ 「停下」**只有一个真相**（两份状态 = 第二个真相 ⇒ 会漂）。
     *
     * @returns {{reason: string|null, pc: number}|null}
     */
    function lastHalt() {
        return viewStore.getState("debug")?.debug?.halt ?? null;
    }

    window.riscv_kernel = {
        /** 最近一次「停下」的原因与 PC（`Halted` 事件）；还没有就是 `null`。 */
        lastHalt,
        /**
         * 在 `addr` 下断点（`id` 是 M-08 的断点号）。
         *
         * ⚠️ **地址必须对齐**（RISC-V 压缩指令 2 字节对齐）——M-08 会把
         * 未对齐的地址按 `E_CTX-07` 拒掉。
         */
        setBreakpoint: (addr, id = 1) => dispatch(Command.setBreakpoint({
            id: Number(id) >>> 0,
            addr: Number(addr) >>> 0,
            source_hint: null,
            enabled: true,
        })),
        /** 撤掉一个断点。 */
        clearBreakpoint: (id = 1) => dispatch(Command.clearBreakpoint(Number(id) >>> 0)),
        /** 读一段内存（`size` 是契约的 `WriteSize` 标签：`Byte` / `Half` / `Word`）。 */
        readMemory: (addr, size = "Word") => dispatch(
            Command.readMemory(Number(addr) >>> 0, size),
        ),
        /**
         * 写一段内存（`size` 是契约的 `WriteSize` 标签：`Byte` / `Half` / `Word`）。
         *
         * ⚠️ **为什么前端 UI 里没有这个入口**：真实 RTOS 的复验脚本要往
         * guest 看得见的地方写一个字节（判别式「新 RAM 段可写可读」），而
         * 页面上没有「写内存」的按钮 ⇒ 只能由脚本走这条可编程的路。
         */
        writeMemory: (addr, size = "Word", value = 0) => dispatch(
            Command.writeMemory(Number(addr) >>> 0, size, Number(value) >>> 0),
        ),
        /**
         * 继续跑。
         *
         * ⚠️ **超时放宽到 30 s**：M-05 的默认超时是 500 ms，而真实 RTOS
         * 跑一批（到下一个事件 / 断点）常常超过它 ⇒ 默认值会让这条命令
         * **假超时**（内核还在跑，前端却报了 `E_CTX-06`）。
         *
         * ⚠️ **在途计数 +1**（STAGE-38 · v0.53 批 · F9 / F10）⇒ ⇒ 脚本口
         * 跑的那**一个**窗口也算「运行中」⇒ ⇒ 活值**也**隐藏 ⇒ ⇒ 否则
         * 「运行中」只看 `debug.run.mode` 就会漏掉这一种。
         *
         * ⚠️ **退休点在 `halted` 订阅**（裁决 B）⇒ ⇒ 这里**只**加不减 ⇒ ⇒
         * 减的地方**只有一处** ⇒ ⇒ 不是第二个真相。
         */
        continueRun: () => {
            bumpRunBusy(+1);
            return dispatch(Command.continueRun(), 30000).catch((err) => {
                // ⚠️ **发不出去 ⇒ 计数退回来**⇒ ⇒ 否则内核**根本没跑**而
                // 计数说「在跑」⇒ ⇒ 活值会**永远**藏着 ⇒ ⇒ 界面在骗人。
                bumpRunBusy(-1);
                throw err;
            });
        },
    };
}
