// App 层 · 页头固件装载接线（顶栏入口控件 + 短状态条同步）（从 app.js 下沉）。
//
// ⚠️ 单向依赖：视图状态（M-03）、shared 派发辅助、image-loader 命令族与
// 共享组件；不回引门面 ⇒ ⇒ 不形成子模块 ↔ 门面循环。

import * as viewStore from "../m03_viewstore.js";
import { setNote } from "./shared.js";
import { handleLoadImageCommand } from "./image-loader.js";
import {
    describeImageLoadShort,
    IMAGE_ENTRY_FROM,
    imageLoadDecision,
    loadImageFileIntoPending,
    publishProductState,
    renderImageLoaderControl,
    setImageLoadState,
} from "../components/image-loader.js";

/**
 * 页头那条短状态条**只订阅一次**（`riscv:render` 是**窗口级**事件）。
 *
 * ⚠️ **为什么要这个标志**：这个函数可能被多次调用（测试里每次 `setup()` 都
 * 调）⇒ ⇒ 每次都 `addEventListener` ⇒ ⇒ 监听器会**堆积** ⇒ ⇒ 一次状态
 * 变化触发 N 次同步 ⇒ ⇒ 白费。⇒ ⇒ 标志只管**这一个**监听器（控件本体
 * 每次都重挂 ⇒ ⇒ 按钮的监听器总是跟着新元素走 ⇒ ⇒ 不会指向旧元素）。
 */
let topbarBriefWired = false;

/**
 * Wire the **top-bar** firmware loader (STAGE-33 · v0.48 批).
 *
 * ⚠️ **为什么在 app 层**：页头**不属于任何视图面板** ⇒ ⇒ 视图层**管不到**
 * 它 ⇒ ⇒ 事件只能挂在这里（页头由 app 层统一接线）。
 *
 * ⚠️ **控件本体由共享组件渲染**（`renderImageLoaderControl`）⇒ ⇒ `accept`
 * 列表与按钮文案**只有一处** ⇒ ⇒ **不写第二套**。
 *
 * ⚠️ **状态同源**：页头那条短状态条**只读** `product.imageLoad` ⇒ ⇒ 不是
 * 第二个真相 ⇒ ⇒ 同步挂在 **`riscv:render` 事件**上（**不**挂
 * `viewStore.subscribe` ⇒ ⇒ `reset()` 会清空订阅者 ⇒ ⇒ 会永久失联）。
 *
 * ⚠️ **页头的输入框不在视图面板里** ⇒ ⇒ 视图重渲染（`body.innerHTML` 被
 * 换掉）**碰不到它** ⇒ ⇒ 用户选完文件后，页头那个输入框**仍然留着那个
 * 文件** ⇒ ⇒ 真机门禁因此能拿到一条**正向**判据（「输入框里真的有那个
 * 文件」）⇒ ⇒ 而产品视图那个输入框**做不到**（重渲染就被换掉了）。
 *
 * ⚠️ **v0.58 批：这是**唯一**那个加载入口**（产品视图里那个重复的入口
 * 撤掉了）⇒ ⇒ 「点一下该怎么办」由 `imageLoadDecision` **一处**判定 ⇒ ⇒
 * 四格：`loading`（幂等忽略）/ `load`（装）/ `already-loaded`（**不报失败**）
 * / `no-file`（**真的**没选过文件才报错）。
 *
 * ⚠️ **v0.59 批加第五格**：`reading`（**正在读那个文件**）⇒ ⇒ 读取期间
 * ⛔ **不许**说「已装载」⇒ ⇒ 用户刚选的新文件那一下**不会被吞掉**⇒ ⇒
 * 详见 `imageLoadDecision` 那里写的原因。
 *
 * ⚠️ **导出是为了让它可测**（同 `handleCommand` 的理由）：真机门禁要断言
 * 「默认落地页上就有这个入口」⇒ ⇒ 那条断言必须能在**不启动整个 app** 的
 * 情况下把控件挂上去。
 *
 * @param {object} services
 */
export function wireTopbarImageLoader(services) {
    const slot = document.querySelector("[data-image-load-slot]");
    if (!slot) return;
    slot.innerHTML = renderImageLoaderControl({
        label: "固件镜像",
        buttonText: "加载固件",
        withBrief: true,
    });
    // ⚠️ **读字节必须在 `change` 里**（浏览器把文件**只**交给这个事件；
    // 点击时 `input.files` 已经是空的了）⇒ ⇒ 「读字节 + 存 pending」那段
    // **住在共享组件里**（`loadImageFileIntoPending`）⇒ ⇒ **不写第二套**。
    const fileInput = slot.querySelector(
        `[data-image-input][data-image-load-from="${IMAGE_ENTRY_FROM}"]`,
    );
    fileInput?.addEventListener("change", (ev) => {
        const file = ev.target?.files?.[0] ?? null;
        if (!file) return;
        void loadImageFileIntoPending(file);
    });
    // ⚠️⚠️ **同一个文件重选要能再次触发 `change`**（v0.59 批 · 用户报的现象
    // 「复位后无法再运行」的第二段）。
    //
    // ⚠️ **浏览器的行为**：`<input type="file">` 的**值没变** ⇒ ⇒ **不触发**
    // `change` ⇒ ⇒ 用户「装一次 ⇒ 复位 ⇒ **再选同一个固件**」时**选不动** ⇒ ⇒
    // 固件再也回不来 ⇒ ⇒ 界面在骗人（看起来「点了没反应」）。
    //
    // ⚠️ **为什么落点在 `click` 而不是 `change`**：`change` **压根不触发** ⇒ ⇒
    // 在那个事件里清 `value` 是**永远轮不到**的 ⇒ ⇒ 必须**在选择器打开之前**
    // 清 ⇒ ⇒ `click` 是**唯一**能提前知道「用户又要选文件了」的那个事件。
    //
    // ⚠️ **清 `value` 不影响任何既有判据**：清完之后 `input.files` 为空 ⇒ ⇒
    // 用户**选完**文件浏览器**自己**把新文件填回去 ⇒ ⇒ 真机门禁那条「输入框里
    // 真的有那个文件」的**正向**判据**照旧成立**（它走 CDP 选文件 ⇒ ⇒ **不经过**
    // `click` ⇒ ⇒ 值不会被清）。
    fileInput?.addEventListener("click", (ev) => {
        // ⚠️ **清空当前选择** ⇒ ⇒ 于是「选回同一个文件」在浏览器看来**也是变了**
        // ⇒ ⇒ `change` 照常触发 ⇒ ⇒ 「重装同一个固件」这条路**重新通上**。
        ev.target.value = "";
    });
    // ⚠️ **「装载」⇒ 走那**一条**装载路径**（`handleLoadImageCommand`）
    // ⇒ ⇒ 「重订流 / 清残留」只有一处。
    slot.querySelector(`[data-action="load-image"][data-image-load-from="${IMAGE_ENTRY_FROM}"]`)
        ?.addEventListener("click", () => {
            // ⚠️ **判定只有一处**（`imageLoadDecision`）⇒ ⇒ 四格的后果
            // 各不相同 ⇒ ⇒ 尤其「上次装成功了」那一格 **⛔ 不报失败**。
            const decision = imageLoadDecision(viewStore.getState("product")?.imageLoad);
            if (decision.kind === "loading") {
                // ⚠️ **幂等**：正在装 ⇒ ⇒ 这一下**忽略** ⇒ ⇒ 连点两下
                // **不会**发第二条 `LoadProject`（内核被连着换两次工程）。
                return;
            }
            if (decision.kind === "reading") {
                // ⚠️ **读取中**（v0.59 批）⇒ ⇒ 字节**还没到**⇒ ⇒ ⛔ **不许**
                // 说「已装载」⇒ ⇒ 用户刚选的新文件**没被吞掉**⇒ ⇒ 如实说
                // 「正在读」⇒ ⇒ 用户等状态条变成「已选…」再点一下。
                setNote("正在读取固件 —— 等页头状态条变成「已选…」之后再点「加载固件」");
                return;
            }
            if (decision.kind === "already-loaded") {
                // ⚠️ **用户报的缺陷**：装完再点一下 ⇒ ⇒ 上次**成功**了 ⇒ ⇒
                // ⛔ **不许**报「装载失败」⇒ ⇒ 如实说「已装载，无需重复加载」。
                setNote(`固件「${decision.name}」已装载 —— 无需重复加载（要换固件请重新选文件）`);
                return;
            }
            if (decision.kind === "no-file") {
                setImageLoadState({
                    status: "error",
                    error: "没有待加载的文件 —— 请先用文件选择器选一个固件镜像",
                    pending: null,
                });
                publishProductState();
                setNote("加载固件失败：没有待加载的文件 —— 请先用文件选择器选一个固件镜像");
                return;
            }
            void handleLoadImageCommand({ params: decision.pending }, services);
        });
    if (!topbarBriefWired) {
        topbarBriefWired = true;
        window.addEventListener("riscv:render", syncTopbarImageBrief);
    }
    syncTopbarImageBrief();
}

/**
 * 把 `product.imageLoad` **同步**到页头那条短状态条。
 *
 * ⚠️ **只读**（不写）⇒ ⇒ 页头**不持有**第二份状态 ⇒ ⇒ 唯一真相仍是
 * `product.imageLoad`。
 */
function syncTopbarImageBrief() {
    const brief = document.querySelector("[data-image-brief]");
    if (!brief) return;
    const state = viewStore.getState("product")?.imageLoad ?? null;
    brief.textContent = describeImageLoadShort(state);
    brief.setAttribute("data-image-brief-status", String(state?.status ?? "idle"));
}
