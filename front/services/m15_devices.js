// M-15 器件中间层的前端侧接线（STAGE-43 · v0.66 · 台账 D-175）。
//
// # 这个模块管什么
//
// 内核侧的**中间层**有两张**只读**的表（契约 v10）：
//   - `device.types` —— **器件类型注册表**（内核支持哪些类型 · 每个类型的
//     窗口大小 · 寄存器**偏移表** · 实例参数的合法区间）；
//   - `device.instances` —— **逐实例状态**（每个实例绑到了哪个基址；
//     `base: null` ⇒ **尚未生效**）。
//
// 本模块把这两张表**接进视图状态**，供产品视图的两张卡渲染。
//
// # 为什么不自己抄一份
//
// ⚠️ **类型集合的真相在内核**（裁决 H）⇒ ⇒ 前端**不**维护自己的器件类型
// 清单 ⇒ ⇒ 「前端抄一份」那个第二个真相源**只留到下一批**（裁决 I：本批
// 加**跨层门禁**钉住它 == 契约层的变体集合）。
//
// ⚠️ **参数合法区间也只来自内核**（裁决 E）⇒ ⇒ 前端**不**写第二份区间表
// ⇒ ⇒ 界面上**只显示**内核给的区间，不自己判。
//
// # 依赖方向
//
// 本模块在**服务层** ⇒ ⇒ 它 import `front/lib/message.js`（中立层）与
// `front/views/m03_viewstore.js`（视图状态）⇒ ⇒ **不**被视图层 import
// ⇒ ⇒ 依赖方向仍是单向的。

import { Command, EVENT_TOPIC } from "../lib/message.js";
import * as viewStore from "../views/m03_viewstore.js";

/**
 * 把内核的两张表接进**产品视图**的视图状态。
 *
 * ⚠️ 器件实例是「产品构成」的信息 ⇒ 只住在 `product` 视图的状态里。
 *
 * @param {object} commandRouter — M-05 命令路由
 * @returns {{reset: () => void, subscribeDeviceStreams: () => Promise<object>,
 *            getTypes: () => Array<object>, getInstances: () => Array<object>}}
 */
export function wireDevices(commandRouter) {
    /**
     * **器件类型注册表**（内核支持什么）。
     *
     * ⚠️ **`null` = 还没收到事件** ⇒ ⇒ 与「收到但 0 条」**分得开**
     * （空态两句话不一样）。
     *
     * @type {Array<object>|null}
     */
    let types = null;

    /**
     * **逐实例状态**。
     *
     * ⚠️ 同样是 `null`（还没收到）vs `[]`（收到但一个实例都没有）。
     *
     * @type {Array<object>|null}
     */
    let instances = null;

    /**
     * 把当前状态写进**产品视图**的视图状态并重渲染。
     *
     * ⚠️ **同一个引用**写进去（不是拷贝）⇒ ⇒ 「视图看到的就是内核说的」
     * ⇒ ⇒ 不会出现两份拷贝不同步。
     */
    function publish() {
        // ⚠️ **只写产品视图**：器件类型清单与逐实例状态各只有一份。
        viewStore.setState("product", (prev) => ({
            ...prev,
            deviceTypes: types,
            deviceInstances: instances,
        }));
        window.dispatchEvent(new CustomEvent("riscv:render", {
            detail: {
                viewId: "product",
                type: "update",
                state: viewStore.getState("product"),
            },
        }));
    }

    commandRouter.subscribe(EVENT_TOPIC.DeviceTypes, (event) => {
        types = Array.isArray(event.value?.types) ? event.value.types : [];
        publish();
    });

    commandRouter.subscribe(EVENT_TOPIC.DeviceInstances, (event) => {
        instances = Array.isArray(event.value?.instances) ? event.value.instances : [];
        publish();
    });

    return {
        /**
         * 新会话 ⇒ 上一会话的两张表**不算数**（与 D-45 / D-46 同一个坑：
         * 状态必须跟着工程一起重建）。
         */
        reset() {
            types = null;
            instances = null;
            publish();
        },
        /**
         * 订阅内核侧的两条流。
         *
         * ⚠️ **必须是强制的**：`LoadProject` 会清掉内核侧的主题集合 ⇒
         * 不重订就**静默**停在旧数据上（和 `memory.map` 是同一个坑）。
         *
         * @returns {Promise<object>}
         */
        async subscribeDeviceStreams() {
            await commandRouter.dispatchCommand(Command.subscribe(EVENT_TOPIC.DeviceTypes));
            return commandRouter.dispatchCommand(Command.subscribe(EVENT_TOPIC.DeviceInstances));
        },
        /** 当前缓存（测试用）。 */
        getTypes() {
            return types;
        },
        /** 当前缓存（测试用）。 */
        getInstances() {
            return instances;
        },
    };
}
