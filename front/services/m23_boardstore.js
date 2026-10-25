// M-23 板档案服务（Board archive store）—— STAGE-15 §3.3 的前端服务层。
//
// # 这个模块管什么
//
// 「板档案」= 用户在界面上配好、**显式保存过**的板级内存图描述的集合
// （台账 D-136 的「板级配置不持久化」+ D-140 的「多板并存」）。
//
// 本模块是它的**存储层**，由两部分组成：
//   - **纯逻辑**（与存储无关 ⇒ 可直接在 node:test 里测）；
//   - **存储适配器**（backend 可注入 ⇒ 测试用内存实现，浏览器里用原生
//     IndexedDB）。
//
// # 为什么「纯逻辑」与「存储」要分开
//
// 前端**零依赖、零构建**（AGENTS.md）⇒ **不许**引 `fake-indexeddb` ⇒
// IndexedDB 那一层在 `node:test` 里**没法测**（规格 §6.3 明写了这个
// 覆盖缺口）。⇒ 于是把「板档案的语义」全部放进**纯函数**里，把「往哪儿
// 存」收敛成**三个方法**的 backend 契约 ⇒ 存储实现可替换 ⇒ 默认门禁
// （node:test）覆盖的是纯逻辑层 + 内存 backend，真实 IndexedDB 靠浏览器
// 真机门禁（规格 T-3）。
//
// # 落库的门槛**不在**本模块
//
// 规格裁决 B：**只有内核接受过的描述才允许落库**（`SetMemoryMap` 命令
// 成功往返 ⇒ 内核的 `validate()` 已经过了）⇒ 库里不会有「永远装不上的
// 板」。⇒ 本模块**不写第二份描述校验器**（延续 STAGE-14 裁决 E）。
// ⇒ 本模块**只**判一件事：**档案里的 `id` 不重复**（台账 D-140 做法 A 的
// 「唯一 id」）⇒ 那是**档案的**约束，不是**描述合法性**的约束 ⇒ 两件事
// 不混。
//
// # 确定性
//
// 落库条目 = `{ id, description, rules_version }`，**不存时间戳**（规格裁决 E）
// ⇒ 同一个描述落库两次 ⇒ 库里是同一条、同样的字节（AD §8.1 D0「种子 + 时间戳化
// 输入事件序列 → 结果唯一」⇒ 落库内容里**不引入**时间戳）。
//
// ⚠️ **不另存 `name`** —— `name` 在描述里面 ⇒ 另存一份就是**第二份真相**
// （名字改了会两边不一致）。
//
// ⚠️ **`rules_version` 是「这块板是按哪一版规则校验过的」**（STAGE-18 · D-143
// · 裁决 F / G）⇒ 它是**库这一侧的元数据** ⇒ **不塞进描述 DTO**（塞进去就是
// 两个真相：描述 + 版本戳 ⇒ 两者会不一致）⇒ ⇒ 落在**条目的容器**里。
//
// ⚠️ **它不参与描述的形状** ⇒ `PLATFORM_SCHEMA_VERSION` **不升**（仍 1）。

import {
    FRONT_ERROR_CODES,
    PLATFORM_RULES_VERSION,
    PLATFORM_SCHEMA_VERSION,
    protocolError,
} from "../lib/message.js";

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/**
 * IndexedDB 库名。
 *
 * ⚠️ **独立库名** ⇒ 浏览器里多一个库，但**与用户已有的本地数据互不相干**
 * （规格 §7.1 第 3 条破坏性变更）。
 */
export const BOARD_DB_NAME = "riscv-sim-board-store";

/** IndexedDB 库版本（首次建库 = 1）。 */
export const BOARD_DB_VERSION = 1;

/** objectStore 名（一个库里只有板档案这一张表）。 */
export const BOARD_STORE_NAME = "boards";

/**
 * objectStore 的 `keyPath` = 主键 = **描述的 `id`**（规格裁决 E）。
 *
 * ⚠️ 已有板的 `id` **只读** ⇒ 「改 `id` 的入口不存在」⇒ 主键稳定 ⇒
 * 覆盖同一条板就是「更新」，不是「多一块」。
 */
export const BOARD_STORE_KEY = "id";

/**
 * 描述格式版本 —— **从契约镜像层 import**
 * （`front/lib/message.js` 的 `PLATFORM_SCHEMA_VERSION`，
 *  镜像 `crates/contract/src/platform.rs` 的同名常量）。
 *
 * ⚠️ **这与契约信封的 `schema_version`（现在是 6）是两个不同的东西**：
 * 前者管「内存图描述长什么样」，后者管「消息信封长什么样」。
 *
 * ⚠️ **为什么住在 `front/lib/` 而不是本模块**（STAGE-15 裁决 K）：
 * 视图层 `front/views/components/memory-map.js` 要用**同一个**值 ⇒
 * 两处各定义一份就是**两个真相**（会漂移）⇒ 常量住在**中立层** `front/lib/`
 * ⇒ 服务层与视图层**都从那里 import** ⇒ **一个定义处**，而依赖方向依然单向
 * （两边都只依赖 `front/lib/`，**互不依赖** ⇒ 服务层**不 import 视图层**）。
 */
export { PLATFORM_SCHEMA_VERSION };

/**
 * 合法性**规则集**版本 —— **从契约镜像层 import**
 * （`front/lib/message.js` 的 `PLATFORM_RULES_VERSION`，
 *  镜像 `crates/contract/src/platform.rs` 的同名常量）。
 *
 * ⚠️ **它与 `PLATFORM_SCHEMA_VERSION` 是两个不同的东西**：前者管**形状**
 * （描述长什么样），它管**合法性规则集**（哪些描述合法）⇒ ⇒ **本批两个
 * 版本号都不升**（STAGE-18 裁决 O）。
 *
 * ⚠️ **唯一落点**在 `front/lib/message.js`（STAGE-18 裁决 E）⇒ 这里只是
 * **re-export** ⇒ 服务层与视图层**都从那里 import** ⇒ **一个定义处**。
 */
export { PLATFORM_RULES_VERSION };

// ---------------------------------------------------------------------------
// 纯逻辑（与存储无关 ⇒ 可测）
// ---------------------------------------------------------------------------

/**
 * 取描述的 `id`。
 *
 * ⚠️ **判据与契约层 V-2 逐字一致**（`crates/contract/src/platform.rs`：
 * `self.id.is_empty()`）⇒ **不做 trim** ⇒ 前端与内核对「空 id」的判断
 * 不会分叉（前端若自作主张 trim，就会出现「前端说合法、内核说非法」）。
 *
 * @param {object|null|undefined} description — 板级内存图描述
 * @returns {string|null} 非空字符串的 `id`；否则 `null`
 */
export function boardId(description) {
    const id = description?.id;
    if (typeof id !== "string" || id.length === 0) return null;
    return id;
}

/**
 * 把一个描述变成**落库条目**。
 *
 * ⚠️ 字段集合**恰为三个**：`id` + `description` + `rules_version` ⇒
 * **不存时间戳**（确定性）、**不存 `name`**（`name` 在描述里 ⇒ 另存就是
 * 第二份真相）。
 *
 * ⚠️ **`rules_version` = 这块板是按哪一版合法性规则校验过的**
 * （STAGE-18 · D-143 · 裁决 F）⇒ 它**不**进描述 DTO（那是两个真相）⇒
 * ⇒ 落在**条目这个容器**里。
 *
 * ⚠️ **描述字段一个都不改** ⇒ ⇒ `PLATFORM_SCHEMA_VERSION` **不升**。
 *
 * @param {object|null|undefined} description
 * @returns {{id: string, description: object, rules_version: number}|null} 取不到 `id` ⇒ `null`
 */
export function boardEntry(description) {
    const id = boardId(description);
    if (id === null) return null;
    return { id, description, rules_version: PLATFORM_RULES_VERSION };
}

/**
 * 取一条板档案条目的**规则集版本**。
 *
 * ⚠️ **缺失 / 不是数字 ⇒ 隐含 1**（STAGE-18 · 裁决 G）：
 *   - 隐含值 **1** 的依据 = 「规则集收紧**之前**的那一版」（裁决 E）⇒ ⇒
 *     老条目的语义**如实**是「按老规则校验过的」；
 *   - **容忍缺省是必须的** ⇒ 否则老库**直接读不出来**（破坏性变更预警第①条）。
 *
 * @param {object|null|undefined} entry — 板档案条目
 * @returns {number} 条目上的规则集版本（缺省 ⇒ 1）
 */
export function boardRulesVersion(entry) {
    const version = entry?.rules_version;
    if (typeof version === "number" && Number.isFinite(version)) return version;
    return 1;
}

/**
 * 查一条板档案条目的**合法性规则集版本**对不对。
 *
 * ⚠️ **与 `checkBoardVersion` 是两件事**（STAGE-18 · 裁决 H）：
 *   - `checkBoardVersion` 管**形状**（`schema_version` 不等于 1）⇒
 *     `reason: "version"`；
 *   - 本函数管**规则已收紧**（条目是老规则下校验过的）⇒ `reason: "rules"`。
 * ⇒ ⇒ **两个 reason 不共用** ⇒ 前端能**分开显示**。
 *
 * ⚠️ **它不判「这块板现在合不合法」** ⇒ 合法性**只**由内核那一处裁决 ⇒
 * ⇒ 前端**不写第二份校验器**（STAGE-14 裁决 E / STAGE-18 裁决 I）。
 *
 * @param {object|null|undefined} entry — 板档案条目
 * @returns {{ok: true}|{ok: false, reason: "rules", expected: number, actual: number}}
 */
export function checkBoardRules(entry) {
    const actual = boardRulesVersion(entry);
    if (actual === PLATFORM_RULES_VERSION) return { ok: true };
    return {
        ok: false,
        reason: "rules",
        expected: PLATFORM_RULES_VERSION,
        actual,
    };
}

/**
 * 一块**规则版本落后**的板 ⇒ 选用时**必须说清楚**的那句话。
 *
 * ⚠️ **不静默**（STAGE-18 · 裁决 I）：提示里**同时**给出「这块板的规则版本」
 * 与「当前规则版本」⇒ ⇒ 用户看得见差在哪。
 *
 * ⚠️ **它只是提示** ⇒ 提示之后**仍然可以选用**（草稿照常载入）⇒ ⇒
 * 前端**不判**合法性（合法性只由内核裁决）。
 *
 * @param {object|null|undefined} entry — 板档案条目
 * @returns {string|null} 需要提示时给一句话；规则版本已是当前 ⇒ `null`
 */
export function boardRulesNotice(entry) {
    const check = checkBoardRules(entry);
    if (check.ok) return null;
    return `这块板是按旧规则保存的（规则版本 ${check.actual}），当前规则版本 ${check.expected}`;
}

/**
 * 把一条板并进档案（**纯函数**）。
 *
 * ⚠️ **纯函数**：**不改入参**、**返回新 archive**（调用方拿到的返回值
 * 与传进去的数组**不是同一个对象**）⇒ 视图层不会因为「忘了拷一份」而
 * 把已保存的档案改掉。
 *
 * - 同 `id` ⇒ **覆盖**（`changed: true`，位置不变）；
 * - 新 `id` ⇒ **追加**到末尾（`changed: true`）；
 * - `entry` 为 `null`（或没有合法 `id`）⇒ `{archive, changed: false}`。
 *
 * @param {Array<object>} archive — 板档案（元素形如 `{ id, description, rules_version }`）
 * @param {{id: string, description: object, rules_version?: number}|null} entry
 * @returns {{archive: Array<object>, changed: boolean}}
 */
export function mergeBoard(archive, entry) {
    const base = asArchive(archive);
    const id = entry?.id;
    if (typeof id !== "string" || id.length === 0) {
        // ⚠️ 没有合法主键的条目**不能进档案**（主键就是 `id`）⇒ 但这里
        // **不抛错**：合并是「并进去」的语义，没有东西可并 ⇒ 如实说
        // 「没变」⇒ 上层据此可以不提示。
        return { archive: base.slice(), changed: false };
    }
    const index = base.findIndex((item) => item?.id === id);
    if (index >= 0) {
        // 同 `id` ⇒ **覆盖**（保留原位置 ⇒ 档案的顺序不因为一次覆盖而抖动）
        const next = base.slice();
        next[index] = entry;
        return { archive: next, changed: true };
    }
    return { archive: [...base, entry], changed: true };
}

/**
 * 从档案里删掉一块板（**纯函数**）。
 *
 * - 删存在的 ⇒ `changed: true`；
 * - 删不存在的 ⇒ `changed: false`（**不报错** ⇒ 删两次是幂等的）。
 *
 * @param {Array<object>} archive
 * @param {string} id
 * @returns {{archive: Array<object>, changed: boolean}}
 */
export function dropBoard(archive, id) {
    const base = asArchive(archive);
    const index = base.findIndex((item) => item?.id === id);
    if (index < 0) return { archive: base.slice(), changed: false };
    const next = base.slice();
    next.splice(index, 1);
    return { archive: next, changed: true };
}

/**
 * 在档案里找一块板。
 *
 * @param {Array<object>} archive
 * @param {string} id
 * @returns {{id: string, description: object, rules_version?: number}|null}
 */
export function findBoard(archive, id) {
    const base = asArchive(archive);
    const found = base.find((item) => item?.id === id);
    return found ?? null;
}

/**
 * 这个 `id` 已经被占了没？
 *
 * ⚠️ **这是前端唯一会判的事**（台账 D-140 做法 A 的「唯一 id」）⇒
 * **不判描述合法性**（合法性由内核裁决 ⇒ STAGE-14 裁决 E）。
 *
 * @param {Array<object>} archive
 * @param {string} id
 * @returns {boolean}
 */
export function isBoardIdTaken(archive, id) {
    if (typeof id !== "string" || id.length === 0) return false;
    return findBoard(archive, id) !== null;
}

/**
 * 查描述的**格式版本**对不对。
 *
 * ⚠️ **绝不许静默按新版本解析**：版本对不上 ⇒ **明确地拒**（`ok:false`）
 * ⇒ 上层据此提示用户，而不是「按 1 的规则硬解一个 2 的描述」。
 * （这条正是台账 D-143「落库的描述没有迁移口径」的落点前的最后一道闸：
 * 至少**看得见**对不上的库存。）
 *
 * ⚠️ 返回值里**不含任何「已修正的描述」** ⇒ 拒了就是拒了，不给「改好了
 * 的那一份」⇒ 免得调用方顺手把修正版存回库里（那就是静默改数据）。
 *
 * @param {object|null|undefined} description
 * @returns {{ok: true}|{ok: false, reason: "version", expected: number, actual: number|null}}
 */
export function checkBoardVersion(description) {
    const actual = description?.schema_version;
    if (actual === PLATFORM_SCHEMA_VERSION) return { ok: true };
    return {
        ok: false,
        reason: "version",
        expected: PLATFORM_SCHEMA_VERSION,
        // ⚠️ 缺失（`undefined`）与「版本是 null」在**判据上**都是「不等于
        // 期望版本」⇒ 如实报成 `null`，不假装它是 0。
        actual: actual ?? null,
    };
}

// ---------------------------------------------------------------------------
// 存储适配层（backend 可注入 ⇒ 测试用内存实现）
// ---------------------------------------------------------------------------

/**
 * 内存 backend（**测试用**；浏览器里不装它）。
 *
 * ⚠️ **它必须复刻 IndexedDB 的「结构化克隆」语义**（写入时拷一份、读出
 * 时再拷一份）⇒ 否则调用方之后**改动草稿对象**会把**已落库的档案悄悄改
 * 掉**（内存 backend 与真 backend 行为不一致 ⇒ 测试测不出真问题）。
 *
 * ⚠️ `list()` 的顺序**确定**（按 `id` 升序）⇒ 断言可以写死。
 *
 * @returns {{supported: true, list: Function, put: Function, remove: Function, clear: Function}}
 */
export function createMemoryBackend() {
    /** @type {Map<string, object>} */
    const store = new Map();

    return {
        supported: true,

        /**
         * 读全部（按 `id` 升序）。
         *
         * @returns {Promise<Array<object>>}
         */
        async list() {
            return [...store.values()]
                .map((entry) => cloneValue(entry))
                .sort((a, b) => compareIds(a?.id, b?.id));
        },

        /**
         * 按 `entry.id` 写入（**覆盖**同 `id` 的旧条目）。
         *
         * @param {{id: string, description: object, rules_version?: number}} entry
         * @returns {Promise<void>}
         */
        async put(entry) {
            const id = requireEntryId(entry);
            store.set(id, cloneValue(entry));
        },

        /**
         * 按 `id` 删除（**不存在就是不存在** ⇒ 不报错）。
         *
         * @param {string} id
         * @returns {Promise<void>}
         */
        async remove(id) {
            store.delete(id);
        },

        /**
         * 清空全部。
         *
         * @returns {Promise<void>}
         */
        async clear() {
            store.clear();
        },
    };
}

/**
 * 原生 IndexedDB backend（**浏览器里真正用的那一个**）。
 *
 * ⚠️ **在不支持 IndexedDB 的环境里不许抛异常** ⇒ 明确地返回
 * `{ supported: false, reason }` ⇒ 上层据此**显式降级**成「板档案只存在
 * 于本会话」并提示用户（规格 §3.3 / 验收 B-7 / C-10）⇒ **不静默假装
 * 成功**。
 *
 * ⚠️ 库是**独立**的（`BOARD_DB_NAME`）⇒ 与用户已有的本地数据互不相干。
 *
 * @returns {{supported: true, list: Function, put: Function, remove: Function, clear: Function}
 *         |{supported: false, reason: string}}
 */
export function createIndexedDbBackend() {
    const idb = globalThis.indexedDB;
    if (!idb || typeof idb.open !== "function") {
        return {
            supported: false,
            reason: "当前环境没有 IndexedDB（浏览器外运行，或浏览器禁用了本地存储）",
        };
    }

    /** @type {Promise<IDBDatabase>|null} */
    let dbPromise = null;

    /**
     * 打开（或复用）板档案库。
     *
     * ⚠️ **懒打开**：构造 backend 时**不**碰 IndexedDB ⇒ 构造永远不抛、
     * 不产生副作用 ⇒ 「支不支持」这件事在构造时就已判定清楚。
     *
     * @returns {Promise<IDBDatabase>}
     */
    function openDatabase() {
        if (!dbPromise) {
            dbPromise = new Promise((resolve, reject) => {
                /** @type {any} */
                let request;
                try {
                    request = idb.open(BOARD_DB_NAME, BOARD_DB_VERSION);
                } catch (err) {
                    reject(err);
                    return;
                }
                // ⚠️ 升级时建表：`keyPath = "id"` ⇒ 主键就是描述的 `id`
                // ⇒ `put(entry)` 直接放整条 entry，不用手工拆键。
                request.onupgradeneeded = () => {
                    const db = request.result;
                    if (!db.objectStoreNames.contains(BOARD_STORE_NAME)) {
                        db.createObjectStore(BOARD_STORE_NAME, { keyPath: BOARD_STORE_KEY });
                    }
                };
                request.onsuccess = () => resolve(request.result);
                request.onerror = () => reject(request.error ?? new Error("IndexedDB 打开失败"));
                request.onblocked = () => reject(new Error("IndexedDB 打开被阻塞（别的标签页占着旧版本）"));
            }).catch((err) => {
                // ⚠️ 失败**不缓存** ⇒ 下次调用能重试（否则一次临时故障就
                // 把这个 backend 永久废掉）。
                dbPromise = null;
                throw err;
            });
        }
        return dbPromise;
    }

    /**
     * 在一个事务里做一件事，返回该请求的结果。
     *
     * ⚠️ **写操作在 `tx.oncomplete` 才 resolve**（而不是请求成功就
     * resolve）⇒ 事务被回滚时**不会**假装写成功了。
     *
     * @param {"readonly"|"readwrite"} mode
     * @param {(store: IDBObjectStore) => any} work
     * @returns {Promise<any>}
     */
    function runTransaction(mode, work) {
        return openDatabase().then((db) => new Promise((resolve, reject) => {
            /** @type {IDBTransaction} */
            let tx;
            try {
                tx = db.transaction(BOARD_STORE_NAME, mode);
            } catch (err) {
                reject(err);
                return;
            }
            /** @type {any} */
            let request = null;
            try {
                request = work(tx.objectStore(BOARD_STORE_NAME));
            } catch (err) {
                try {
                    tx.abort();
                } catch {
                    // 事务可能已经没了 ⇒ 忽略（真正的错是上面那个 err）
                }
                reject(err);
                return;
            }
            tx.oncomplete = () => resolve(request?.result);
            tx.onerror = () => reject(tx.error ?? new Error("IndexedDB 事务失败"));
            tx.onabort = () => reject(tx.error ?? new Error("IndexedDB 事务被中止"));
        }));
    }

    return {
        supported: true,

        /**
         * 读全部（`getAll()` ⇒ 一次读完 ⇒ 顺序由键（`id`）决定）。
         *
         * @returns {Promise<Array<object>>}
         */
        async list() {
            const rows = await runTransaction("readonly", (store) => store.getAll());
            return Array.isArray(rows) ? rows : [];
        },

        /**
         * 按 `entry.id` 写入（**覆盖**同 `id` 的旧条目）。
         *
         * @param {{id: string, description: object, rules_version?: number}} entry
         * @returns {Promise<void>}
         */
        async put(entry) {
            const id = requireEntryId(entry);
            await runTransaction("readwrite", (store) => store.put(entry));
            return id;
        },

        /**
         * 按 `id` 删除。
         *
         * @param {string} id
         * @returns {Promise<void>}
         */
        async remove(id) {
            await runTransaction("readwrite", (store) => store.delete(id));
        },

        /**
         * 清空全部。
         *
         * @returns {Promise<void>}
         */
        async clear() {
            await runTransaction("readwrite", (store) => store.clear());
        },
    };
}

// ---------------------------------------------------------------------------
// 存储门面（把 backend 收成「板档案」这一个概念）
// ---------------------------------------------------------------------------

/**
 * 装一个板档案门面。
 *
 * ⚠️ **它不直接碰 IndexedDB** ⇒ 只用 backend 的那三个方法 ⇒ 存储实现可
 * 替换 ⇒ 纯逻辑与存储各测各的。
 *
 * ⚠️ **backend 不支持时**：`list()` 返回**空数组**（**不抛错**）⇒ 上层
 * 可以无条件读档案 ⇒ 「没有数据」与「数据是空的」分得开；**写操作则
 * 明确地失败**（抛 `E_STORE-01`）⇒ **不静默假装成功** ⇒ 上层要么先看
 * `store.supported` 显式降级，要么捕获这个错误。
 *
 * @param {{supported?: boolean, list: Function, put: Function, remove: Function, clear?: Function}} backend
 * @returns {{supported: boolean, list: Function, put: Function, remove: Function, clear: Function}}
 */
export function createBoardStore(backend) {
    if (!backend || typeof backend !== "object") {
        throw protocolError(FRONT_ERROR_CODES["E_STORE-03"], "createBoardStore 需要一个 backend（list / put / remove）");
    }
    const supported = backend.supported !== false;
    // ⚠️ **不支持的 backend 允许「没有方法」**：`createIndexedDbBackend()` 在
    // 没有 IndexedDB 的环境里只返回 `{ supported: false, reason }` ⇒ 门面
    // 必须能装上它（否则上层连「显式降级」都做不到）。
    if (supported && (typeof backend.list !== "function" || typeof backend.put !== "function")) {
        throw protocolError(FRONT_ERROR_CODES["E_STORE-03"], "createBoardStore 需要一个 backend（list / put / remove）");
    }

    return {
        /** 这个档案**能不能落盘**（上层据此显式降级）。 */
        supported,

        /**
         * 读全部（按 `id` 升序 ⇒ 顺序确定 ⇒ 断言可写死）。
         *
         * @returns {Promise<Array<object>>}
         */
        async list() {
            if (!supported) return [];
            const rows = await backend.list();
            return (Array.isArray(rows) ? rows : []).slice().sort((a, b) => compareIds(a?.id, b?.id));
        },

        /**
         * 写一条（按 `entry.id` **覆盖**）。
         *
         * @param {{id: string, description: object, rules_version?: number}} entry
         * @returns {Promise<void>}
         */
        async put(entry) {
            requireEntryId(entry);
            if (!supported) throw unsupportedError();
            await backend.put(entry);
        },

        /**
         * 删一条（**不存在就是不存在** ⇒ 不报错）。
         *
         * @param {string} id
         * @returns {Promise<void>}
         */
        async remove(id) {
            if (!supported) throw unsupportedError();
            await backend.remove(id);
        },

        /**
         * 清空全部。
         *
         * ⚠️ backend 若**没有** `clear`（规格 §3.3 的 backend 契约只写了
         * 三个方法）⇒ 退化成「逐条删」⇒ **三方法契约照样成立**。
         *
         * @returns {Promise<void>}
         */
        async clear() {
            if (!supported) throw unsupportedError();
            if (typeof backend.clear === "function") {
                await backend.clear();
                return;
            }
            const rows = await backend.list();
            for (const row of Array.isArray(rows) ? rows : []) {
                await backend.remove(row?.id);
            }
        },
    };
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/**
 * 档案数组的兜底：不是数组就当**空档案**（不抛错 ⇒ 上层拿到的永远是
 * 可迭代的数组）。
 *
 * @param {unknown} archive
 * @returns {Array<object>}
 */
function asArchive(archive) {
    return Array.isArray(archive) ? archive : [];
}

/**
 * 主键检查（**一个约束一个落点** ⇒ 门面与内存 backend 共用这一个）。
 *
 * ⚠️ 报出来的码是**前端本地码**，**不在**契约层的错误表里 ⇒ 不要拿它
 * 当契约码用。
 *
 * ⚠️ **它带 `err.code`**（`protocolError` ⇒ ⇒ STAGE-28 裁决 F）⇒ ⇒ 上层
 * **能按码分派** ⇒ ⇒ 但**码的归属仍在前端**（契约层**不**定义它）。
 *
 * @param {{id?: unknown}|null|undefined} entry
 * @returns {string} 合法的 `id`
 */
function requireEntryId(entry) {
    const id = entry?.id;
    if (typeof id !== "string" || id.length === 0) {
        throw protocolError(FRONT_ERROR_CODES["E_STORE-02"], "板档案条目缺少合法的 id（主键就是描述的 id）");
    }
    return id;
}

/**
 * 「存储不可用」的显式错误。
 *
 * @returns {Error}
 */
function unsupportedError() {
    return protocolError(FRONT_ERROR_CODES["E_STORE-01"], "板档案存储不可用（没有 IndexedDB）⇒ 板只存在于本会话");
}

/**
 * 按 `id` 比较大小（**按码元比**，不用 `localeCompare`）。
 *
 * ⚠️ `localeCompare` 依赖**运行环境的 locale** ⇒ 同一批数据在不同机器
 * 上可能排不同的序 ⇒ 顺序不确定 ⇒ 断言会飘 ⇒ 这里只按码元比。
 *
 * @param {unknown} a
 * @param {unknown} b
 * @returns {number}
 */
function compareIds(a, b) {
    const left = typeof a === "string" ? a : "";
    const right = typeof b === "string" ? b : "";
    if (left < right) return -1;
    if (left > right) return 1;
    return 0;
}

/**
 * 深拷贝一份（复刻 IndexedDB 的**结构化克隆**语义）。
 *
 * ⚠️ 有 `structuredClone` 就用它（它保 `undefined`）；没有就退回 JSON
 * 往返（描述是纯 JSON 数据 ⇒ 够用）。⇒ 拷贝失败不抛给调用方，而是再试
 * 一次 JSON。
 *
 * @template T
 * @param {T} value
 * @returns {T}
 */
function cloneValue(value) {
    if (value === null || typeof value !== "object") return value;
    if (typeof structuredClone === "function") {
        try {
            return structuredClone(value);
        } catch {
            // 落回 JSON 往返（下面）
        }
    }
    return JSON.parse(JSON.stringify(value));
}
