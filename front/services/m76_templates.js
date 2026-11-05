// M-76 模板库的前端侧接线（STAGE-52 · v0.76 · 裁决 M）。
//
// # 这个模块管什么
//
// 内置板模板库（契约 v12）：
//   - `platform.templates` —— **模板清单**（内核**订阅即推**：id / 名称 /
//     优先级 / 器件数 · priority 升序）；
//   - `GetPlatformTemplate{id}` —— 按 `id` 取**完整描述**（未知 id 拒
//     `E_PLT-22`）。
//
// 本模块把清单**接进视图状态**，并提供「按 id 取模板描述」的**异步**通道。
//
// # 为什么不自己抄一份
//
// ⚠️ **模板清单的真相在内核**（裁决 M）⇒ ⇒ 前端**不**维护自己的模板清单
// ⇒ ⇒ 「前端抄一份」就是第二个真相源（模板改了数据前端还挂着旧的）。
//
// ⚠️ **前端不写第二份校验器** ⇒ ⇒ 描述合法性由内核裁决（E_PLT-*），
// 这里只透传。
//
// # 依赖方向
//
// 本模块在**服务层** ⇒ ⇒ 它 import `front/lib/message.js`（中立层）与
// `front/views/m03_viewstore.js`（视图状态）⇒ ⇒ **不**被视图层 import
// ⇒ ⇒ 依赖方向仍是单向的。

import { Command, EVENT_TOPIC } from "../lib/message.js";
import * as viewStore from "../views/m03_viewstore.js";

/**
 * 把内核的模板库接进 **memory-map 视图**的视图状态。
 *
 * ⚠️ 模板清单**只有一份**（`memory-map` 视图持有）⇒ ⇒ 板级编辑器用；
 * 产品视图不重复渲染（同一件事只显示一处）。
 *
 * @param {object} commandRouter — M-05 命令路由
 * @returns {{reset: () => void, subscribeTemplateStreams: () => Promise<object>,
 *            getTemplates: () => Array<object>|null,
 *            loadTemplate: (id: string) => Promise<object>}}
 */
export function wireTemplates(commandRouter) {
    /**
     * 模板清单（内核**订阅即推**）。
     *
     * ⚠️ **`null` = 还没收到事件** ⇒ ⇒ 与「收到但 0 条」**分得开**
     * （空态两句话不一样）。
     *
     * @type {Array<object>|null}
     */
    let templates = null;

    /** 把当前清单写进视图状态并重渲染。 */
    function publish() {
        viewStore.setState("memory-map", (prev) => ({
            ...prev,
            templates,
        }));
        window.dispatchEvent(new CustomEvent("riscv:render", {
            detail: {
                viewId: "memory-map",
                type: "update",
                state: viewStore.getState("memory-map"),
            },
        }));
    }

    commandRouter.subscribe(EVENT_TOPIC.PlatformTemplates, (event) => {
        templates = Array.isArray(event.value?.templates) ? event.value.templates : [];
        publish();
    });

    return {
        /** 新会话 ⇒ 上一会话的模板清单**不算数**。 */
        reset() {
            templates = null;
            publish();
        },
        /** 订阅内核侧的模板清单流（订阅即推一条）。 */
        async subscribeTemplateStreams() {
            return commandRouter.dispatchCommand(
                Command.subscribe(EVENT_TOPIC.PlatformTemplates),
            );
        },
        /** 当前缓存（测试用）。 */
        getTemplates() {
            return templates;
        },
        /**
         * 按 `id` 取一块模板的**完整描述**。
         *
         * ⚠️ **未知 `id` ⇒ 拒 `E_PLT-22`**（与「无板装载」同一个码 ⇒ ⇒
         * 引导用户从模板库选择）⇒ ⇒ 这里**原样传播**内核的错误（前端
         * 不吞、不改写）。
         *
         * @param {string} id — 模板 `id`（default / minimal / qemu-virt）
         * @returns {Promise<object>} 解析为 `PlatformDescription`
         */
        async loadTemplate(id) {
            const reply = await commandRouter.dispatchCommand(
                Command.getPlatformTemplate(id),
            );
            return reply?.value?.description ?? null;
        },
    };
}
