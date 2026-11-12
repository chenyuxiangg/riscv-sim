// M-23 镜像持久化（浏览器本地）——只存“上次选中的镜像”，不存工程。
//
// 为什么单独一个库：镜像最大 16 MiB，localStorage 装不下；板档案库的
// 版本/迁移也不该被镜像字节牵连。这里用独立 IndexedDB，一个 store、
// 一个固定 key（last）。真实 IndexedDB 在 node 门禁里不可用，所以
// 存储适配器可注入：纯逻辑与内存 backend 在 node 测，浏览器走原生实现。

export const IMAGE_DB_NAME = "riscv-sim-image-store";
export const IMAGE_DB_VERSION = 1;
export const IMAGE_STORE_NAME = "images";
export const IMAGE_STORE_KEY = "last";

const IMAGE_KINDS = Object.freeze(["Elf", "RawHex"]);

/**
 * 把界面上的 pending 镜像变成可落库的记录。
 *
 * 只收非空字节；记录形状固定为 { key, name, kind, byteLength, bytes }。
 * 不存 File 句柄（浏览器不允许刷新后按路径重读），只存字节。
 *
 * @param {{name?: string, kind?: string, bytes?: number[]}|null} image
 * @returns {{key: string, name: string, kind: string, byteLength: number, bytes: number[]}|null}
 */
export function imageRecord(image) {
    if (!image || !Array.isArray(image.bytes) || image.bytes.length === 0) return null;
    const name = String(image.name ?? "").trim() || "image.bin";
    if (!IMAGE_KINDS.includes(image.kind)) return null;
    const bytes = Array.from(image.bytes);
    return {
        key: IMAGE_STORE_KEY,
        name,
        kind: image.kind,
        byteLength: bytes.length,
        bytes,
    };
}

/**
 * 存储适配器契约。backend 可以是内存实现（测试）或 IndexedDB。
 *
 * @param {{get: Function, put: Function, delete: Function}|null} backend
 * @returns {{supported: boolean, save: Function, load: Function, clear: Function}}
 */
export function createImageStore(backend) {
    if (
        !backend
        || typeof backend.get !== "function"
        || typeof backend.put !== "function"
        || typeof backend.delete !== "function"
    ) {
        return {
            supported: false,
            async save() { return false; },
            async load() { return null; },
            async clear() {},
        };
    }
    return {
        supported: true,
        async save(image) {
            const record = imageRecord(image);
            if (!record) return false;
            await backend.put(record);
            return true;
        },
        async load() {
            const record = await backend.get(IMAGE_STORE_KEY);
            if (!record || !Array.isArray(record.bytes) || record.bytes.length === 0) {
                return null;
            }
            if (!IMAGE_KINDS.includes(record.kind)) return null;
            return {
                key: IMAGE_STORE_KEY,
                name: String(record.name ?? "").trim() || "image.bin",
                kind: record.kind,
                byteLength: record.bytes.length,
                bytes: Array.from(record.bytes),
            };
        },
        async clear() {
            await backend.delete(IMAGE_STORE_KEY);
        },
    };
}

/**
 * 浏览器 IndexedDB backend；没有 IndexedDB 时返回 null（显式降级）。
 *
 * @param {IDBFactory|undefined} indexedDb
 * @returns {{get: Function, put: Function, delete: Function}|null}
 */
export function createIndexedDbImageBackend(indexedDb = globalThis.indexedDB) {
    if (!indexedDb || typeof indexedDb.open !== "function") return null;
    let dbPromise = null;

    function open() {
        if (dbPromise) return dbPromise;
        dbPromise = new Promise((resolve, reject) => {
            const request = indexedDb.open(IMAGE_DB_NAME, IMAGE_DB_VERSION);
            request.onupgradeneeded = () => {
                const db = request.result;
                if (!db.objectStoreNames.contains(IMAGE_STORE_NAME)) {
                    db.createObjectStore(IMAGE_STORE_NAME, { keyPath: "key" });
                }
            };
            request.onsuccess = () => resolve(request.result);
            request.onerror = () => reject(request.error ?? new Error("打开镜像库失败"));
            request.onblocked = () => reject(new Error("镜像库被其它页面占用"));
        });
        return dbPromise;
    }

    function run(mode, work) {
        return open().then((db) => new Promise((resolve, reject) => {
            let tx;
            let result;
            try {
                tx = db.transaction(IMAGE_STORE_NAME, mode);
            } catch (err) {
                reject(err);
                return;
            }
            // ⚠️ 必须等**事务**完成再 resolve：请求成功只说明这一条读写
            // 成功了，事务仍可能 abort（此时数据不会落库）。
            tx.oncomplete = () => resolve(result);
            tx.onerror = () => reject(tx.error ?? new Error("镜像库事务失败"));
            tx.onabort = () => reject(tx.error ?? new Error("镜像库事务中止"));
            try {
                const request = work(tx.objectStore(IMAGE_STORE_NAME));
                request.onsuccess = () => { result = request.result; };
                request.onerror = () => reject(request.error ?? new Error("镜像库读写失败"));
            } catch (err) {
                reject(err);
            }
        }));
    }

    return {
        get: (key) => run("readonly", (store) => store.get(key)),
        put: (record) => run("readwrite", (store) => store.put(record)),
        delete: (key) => run("readwrite", (store) => store.delete(key)),
    };
}
