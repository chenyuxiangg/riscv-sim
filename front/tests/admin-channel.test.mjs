// M-04/05/07/10 管理员通道（契约 v8 · D-129）+ 场景码落位（STAGE-29 裁决 E）gate.
//
// 本文件钉四件事：
//
//   1. **契约 v8 的镜像**：信封版本 = 8、两个 admin topic、两个事件载荷
//      构造器（验收 C-1）；
//   2. **场景码表**：前端自有码与契约层镜像码**不重叠**、两个时间字段是
//      **十进制字符串**、前端服务层**没有裸字面量**（验收 C-2 / C-3）；
//   3. **跨层一致性**：契约层与前端镜像的 DTO 形状**逐字相同**
//      —— 两边读**同一份** fixture（验收 C-7）；
//   4. **最小接线**：订阅 `admin.logs` / `admin.diag` 能**拿到事件**
//      （验收 C-8）。
//
// ⚠️ **为什么 DTO 的形状要读 Rust 侧的 fixture**：契约层定了一份形状
//   （`LogEntryDto` 8 字段 / `AdminDiagDto` 4 字段）⇒ ⇒ 前端是它的 **JS
//   镜像** ⇒ ⇒ 两棵树描述一个形状 ⇒ ⇒ 「一个形状、两个真相」是本仓
//   最典型的静默故障 ⇒ ⇒ 所以形状**只写一次**
//   （`crates/contract/fixtures/admin-dto.json`）⇒ ⇒ Rust 侧断言自己的
//   序列化与那份文件逐字相符，本文件断言**前端镜像**与**同一份**文件
//   逐字相符 ⇒ ⇒ 任何一侧改了形状，另一侧就红。
//
// ⚠️ **本文件按「服务层」归属**（它测的是 `front/lib/message.js` 的镜像
//   与 `front/services/m05_commandrouter.js` 的分发路径）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
    SCHEMA_VERSION,
    EVENT_TOPIC,
    ADMIN_TOPICS,
    Event,
    eventMessage,
    decodeMessage,
    FRONT_ERROR_CODES,
    MIRRORED_ERROR_CODES,
    RUST_ERROR_CODE_REFS,
} from "../lib/message.js";
import { mountCommandRouter } from "../services/m05_commandrouter.js";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

/** 跨层唯一真相 —— 契约层与前端镜像**都**读它。 */
const FIXTURE_REL = "crates/contract/fixtures/admin-dto.json";
const fixture = JSON.parse(readFileSync(resolve(repoRoot, FIXTURE_REL), "utf8"));

/** 前端自己知道的 DTO 形状（**不**从 fixture 抄 ⇒ ⇒ 两侧各钉一次）。 */
const LOG_ENTRY_FIELDS = [
    "time_virt_ps",
    "time_host_ms",
    "session_id",
    "level",
    "ctx",
    "err_code",
    "msg",
    "truncated",
];
const ADMIN_DIAG_FIELDS = [
    "err_code",
    "snapshot_summary",
    "input_seq_tail",
    "host_clock_available",
];

// ---------------------------------------------------------------------------
// C-1 · 契约 v8 的前端镜像
// ---------------------------------------------------------------------------

describe("契约 v8 · 前端镜像（C-1）", () => {
    test("契约信封版本是 **8**（与契约层同批升版）", () => {
        assert.equal(SCHEMA_VERSION, 8, "契约信封版本必须是 8（STAGE-29 裁决 A）");
    });

    test("两个 admin topic 在表里，且拼写逐字一致", () => {
        assert.equal(EVENT_TOPIC.AdminLogs, "admin.logs");
        assert.equal(EVENT_TOPIC.AdminDiag, "admin.diag");
        assert.deepEqual(ADMIN_TOPICS, ["admin.logs", "admin.diag"]);
    });

    test("`ADMIN_TOPICS` 是**派生**的（不是第二份真相）", () => {
        assert.deepEqual(
            [...ADMIN_TOPICS],
            [EVENT_TOPIC.AdminLogs, EVENT_TOPIC.AdminDiag],
            "admin topic 清单必须从 `EVENT_TOPIC` 派生 ⇒ ⇒ 加一个 admin 事件只改一处",
        );
    });

    test("两个事件载荷构造器存在，且形状是**单键对象**", () => {
        assert.deepEqual(Object.keys(Event.adminLogs([])), ["AdminLogs"]);
        assert.deepEqual(Object.keys(Event.adminLogs([]).AdminLogs), ["entries"]);
        assert.deepEqual(Object.keys(Event.adminDiag({})), ["AdminDiag"]);
        assert.deepEqual(Object.keys(Event.adminDiag({}).AdminDiag), ["diag"]);
    });

    test("`adminLogs` 的条目是**原样透传**（前端不复制 DTO 形状）", () => {
        const entries = fixture.logs_event.AdminLogs.entries;
        const evt = Event.adminLogs(entries);
        assert.equal(
            JSON.stringify(evt),
            JSON.stringify(fixture.logs_event),
            "前端镜像必须能逐字重建契约层的 `AdminLogs` 事件",
        );
    });
});

// ---------------------------------------------------------------------------
// C-2 / C-3 · 场景码表
// ---------------------------------------------------------------------------

describe("场景码表（C-2 / C-3）", () => {
    test("两张表**不重叠**：`E_CTX-05` 只在镜像表里", () => {
        const own = Object.keys(FRONT_ERROR_CODES);
        const mirrored = Object.keys(MIRRORED_ERROR_CODES);
        const overlap = own.filter((c) => mirrored.includes(c));
        assert.deepEqual(overlap, [], "两张码表不许重叠（两处定义 ⇒ 破「唯一定义」）");
        assert.ok(
            mirrored.includes("E_CTX-05"),
            "`E_CTX-05` 是契约层的码 ⇒ ⇒ 必须留在镜像表里",
        );
        assert.ok(
            !own.includes("E_CTX-05"),
            "`E_CTX-05` 不许在前端自有表里再定义一遍",
        );
    });

    test("三张表**两两不重叠**（前端自产 / 契约层镜像 / Rust 侧引用）", () => {
        const tables = [
            ["FRONT_ERROR_CODES", Object.keys(FRONT_ERROR_CODES)],
            ["MIRRORED_ERROR_CODES", Object.keys(MIRRORED_ERROR_CODES)],
            ["RUST_ERROR_CODE_REFS", Object.keys(RUST_ERROR_CODE_REFS)],
        ];
        for (let i = 0; i < tables.length; i += 1) {
            for (let j = i + 1; j < tables.length; j += 1) {
                const [an, a] = tables[i];
                const [bn, b] = tables[j];
                const overlap = a.filter((c) => b.includes(c));
                assert.deepEqual(
                    overlap,
                    [],
                    `${an} 与 ${bn} 重叠了：${overlap.join(", ")}`,
                );
            }
        }
    });

    test("每张表的**键就是码本身**（工具靠这个形状抽出码集合）", () => {
        for (const table of [FRONT_ERROR_CODES, MIRRORED_ERROR_CODES, RUST_ERROR_CODE_REFS]) {
            for (const [key, value] of Object.entries(table)) {
                assert.equal(key, value, `码表的键必须是码本身：${key} ≠ ${value}`);
            }
        }
    });

    test("前端自有码表**不多不少**是那 11 个", () => {
        assert.deepEqual(Object.keys(FRONT_ERROR_CODES).sort(), [
            "E_CTX-01",
            "E_CTX-06",
            "E_CTX-11",
            "E_STORE-01",
            "E_STORE-02",
            "E_STORE-03",
            "E_TRX-01",
            "E_TRX-02",
            "E_TRX-03",
            "E_TRX-04",
            "E_TRX-05",
        ]);
    });

    test("镜像表与 Rust 侧引用表里的码**都**在 Rust 侧有定义处", () => {
        // ⚠️ 这张表里的码**不是**前端自产的 ⇒ ⇒ 它们的定义处**在 Rust 侧**
        // ⇒ ⇒ 前端只引用、不定义 ⇒ ⇒ 契约层 / 内核 crate 的源码里必须
        // 找得到对应的 `const E_XXX_NN…`。
        const rust = [
            readFileSync(resolve(repoRoot, "crates/contract/src/error.rs"), "utf8"),
            readFileSync(resolve(repoRoot, "crates/kernel/src/m11_symbol/trace.rs"), "utf8"),
        ].join("\n");
        for (const code of [
            ...Object.keys(MIRRORED_ERROR_CODES),
            ...Object.keys(RUST_ERROR_CODE_REFS),
        ]) {
            const internal = code.replace(/-/g, "_");
            assert.ok(
                new RegExp(`const\\s+${internal}[A-Z0-9_]*\\b`).test(rust),
                `${code} 的定义处必须在 Rust 侧（找得到 ${internal}… 的 const）`,
            );
        }
    });
});

// ---------------------------------------------------------------------------
// C-2 · 前端服务层没有裸字面量
// ---------------------------------------------------------------------------

// ⚠️ **三个引号字符用 `\xNN` 写** ⇒ ⇒ 源码里**不出现**字面的引号
//    ⇒ ⇒ `tools/front_test_attrib.py` 的 `strip_code`（它**不懂**正则
//    字面量）不会把本文件的后半截误当成字符串 ⇒ ⇒ 用例数与归属桶**不会**
//    被算歪。
const QUOTE_DOUBLE = "\x22";
const QUOTE_SINGLE = "\x27";
const QUOTE_BACKTICK = "\x60";

/** 一个**被引起来**的场景码（三种引号都认）。 */
// ⚠️ **引号那组必须是「捕获组」** ⇒ ⇒ 否则 `\1` **没有可回引的组**
//    ⇒ ⇒ 正则退化成「匹配一个字面的 1」⇒ ⇒ **一条都匹配不到** ⇒ ⇒
//    判据**恒绿** ⇒ ⇒ **零判别力**（实测踩过：MT4 变异打不动它）。
const QUOTED_CODE_RE = new RegExp(
    `([${QUOTE_DOUBLE}${QUOTE_SINGLE}${QUOTE_BACKTICK}])` +
        "((?:E_[A-Z]{3,5}(?:[-_][0-9]{2})(?:_[A-Z0-9]+)*))" +
        "\\1",
    "g",
);

/**
 * 抹掉**注释**（保留长度 ⇒ 偏移不变）—— 字符串**内容**原样保留。
 *
 * ⚠️ **必须**认字符串：`"http://x"` 里的 `//` **不是**注释 ⇒ ⇒ 否则
 * 注释掩码会把字符串里的码判成「在注释里」⇒ ⇒ 判据**恒绿** ⇒ ⇒
 * **零判别力**。
 */
function stripComments(text) {
    const out = text.split("");
    let index = 0;
    while (index < text.length) {
        const char = text[index];
        if (char === "/" && text[index + 1] === "/") {
            while (index < text.length && text[index] !== "\n") {
                out[index] = " ";
                index += 1;
            }
        } else if (char === "/" && text[index + 1] === "*") {
            while (
                index < text.length &&
                !(text[index] === "*" && text[index + 1] === "/")
            ) {
                if (text[index] !== "\n") out[index] = " ";
                index += 1;
            }
            for (let k = 0; k < 2 && index < text.length; k += 1) {
                out[index] = " ";
                index += 1;
            }
        } else if (char === QUOTE_DOUBLE || char === QUOTE_SINGLE || char === QUOTE_BACKTICK) {
            const quote = char;
            index += 1;
            while (index < text.length) {
                if (text[index] === "\\") {
                    index += 2;
                    continue;
                }
                if (text[index] === quote) {
                    index += 1;
                    break;
                }
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    return out.join("");
}

/** 前端**服务层**的文件清单（口径与 `tools/error_code_check.py` 一致）。 */
function serviceFiles() {
    const files = [];
    for (const name of readdirSync(resolve(frontDir, "services")).sort()) {
        if (name.endsWith(".js")) files.push(resolve(frontDir, "services", name));
    }
    for (const name of readdirSync(resolve(frontDir, "lib")).sort()) {
        if (name.endsWith(".js")) files.push(resolve(frontDir, "lib", name));
    }
    files.push(resolve(frontDir, "kernel-worker.js"));
    return files;
}

/** 那三张码表的**表体**所在的行区间（表体内部是「定义处 / 引用处」，不算裸字面量）。 */
function tableBodyLines(code) {
    const pattern =
        /\b(?:FRONT_ERROR_CODES|MIRRORED_ERROR_CODES|RUST_ERROR_CODE_REFS)\b\s*=\s*Object\.freeze\(\{/g;
    const ranges = [];
    let match = pattern.exec(code);
    while (match !== null) {
        const start = code.indexOf("{", match.index) + 1;
        const end = code.indexOf("})", start);
        if (end === -1) break;
        ranges.push([lineOf(code, start), lineOf(code, end)]);
        match = pattern.exec(code);
    }
    return ranges;
}

function lineOf(text, offset) {
    return text.slice(0, offset).split("\n").length;
}

describe("前端服务层 · 没有裸字面量场景码（C-2）", () => {
    test("服务层里每个场景码都**来自常量表**（不是裸字面量）", () => {
        const quoted = QUOTED_CODE_RE;
        const offenders = [];
        for (const file of serviceFiles()) {
            const raw = readFileSync(file, "utf8");
            const code = stripComments(raw);
            const tables = tableBodyLines(code);
            quoted.lastIndex = 0;
            let match = quoted.exec(code);
            while (match !== null) {
                const line = lineOf(code, match.index);
                const inTable = tables.some(([a, b]) => a <= line && line <= b);
                if (!inTable) {
                    // 允许的形态只有一种：`常量表["E_XXX-NN"]` ⇒ ⇒ 紧挨着的
                    // 前一个非空白字符必须是 `[`。
                    let before = match.index - 1;
                    while (before >= 0 && /\s/.test(code[before])) before -= 1;
                    const isSubscript = before >= 0 && code[before] === "[";
                    if (!isSubscript) {
                        offenders.push(
                            `${file.replace(`${repoRoot}/`, "")}:${line} ${match[2]}`,
                        );
                    }
                }
                match = quoted.exec(code);
            }
        }
        assert.deepEqual(
            offenders,
            [],
            `前端服务层不许留裸字面量场景码（改用 FRONT_ERROR_CODES / MIRRORED_ERROR_CODES / RUST_ERROR_CODE_REFS）：\n  ${offenders.join("\n  ")}`,
        );
    });

    test("服务层用到的每个码**都有定义处**（Rust 侧或前端自有码表）", () => {
        // ⚠️ 这一条是 `tools/error_code_check.py` 的新断言 ⑤ 的**前端侧**
        // 对照：前端用到的码要么在 Rust 侧有定义处，要么在前端自有码表里。
        const rust = readFileSync(resolve(repoRoot, "crates/contract/src/error.rs"), "utf8");
        const quoted = QUOTED_CODE_RE;
        const missing = new Set();
        for (const file of serviceFiles()) {
            const code = stripComments(readFileSync(file, "utf8"));
            const tables = tableBodyLines(code);
            quoted.lastIndex = 0;
            let match = quoted.exec(code);
            while (match !== null) {
                const line = lineOf(code, match.index);
                const inTable = tables.some(([a, b]) => a <= line && line <= b);
                const internal = match[2].replace(/-/g, "_");
                const hasRust = new RegExp(`const\\s+${internal}[A-Z0-9_]*\\b`).test(rust);
                const hasFront = Object.hasOwn(FRONT_ERROR_CODES, match[2]);
                if (!inTable && !hasRust && !hasFront) missing.add(match[2]);
                match = quoted.exec(code);
            }
        }
        assert.deepEqual(
            [...missing].sort(),
            [],
            "前端服务层用到的码必须有定义处（Rust 侧或前端自有码表）",
        );
    });
});

// ---------------------------------------------------------------------------
// C-7 · 跨层一致性（契约层 ↔ 前端镜像）
// ---------------------------------------------------------------------------

describe("管理员通道 DTO · 跨层一致性（C-7）", () => {
    test("fixture 存在，且是**唯一真相**", () => {
        assert.ok(
            readFileSync(resolve(repoRoot, FIXTURE_REL), "utf8").length > 0,
            `${FIXTURE_REL} 必须是契约层与前端镜像共读的那份文件`,
        );
    });

    test("日志条目**恰好 8 个字段**，与前端镜像知道的名字逐字一致", () => {
        const entries = fixture.logs_event.AdminLogs.entries;
        assert.equal(entries.length, 1, "fixture 里放一条样例条目");
        assert.deepEqual(
            Object.keys(entries[0]).sort(),
            [...LOG_ENTRY_FIELDS].sort(),
            "`LogEntryDto` 的字段名与前端镜像不一致 ⇒ ⇒ 契约层改了形状",
        );
    });

    test("两个时间字段是**十进制字符串**（契约 v2 §2.2 的 `u64_decimal` 策略）", () => {
        const entry = fixture.logs_event.AdminLogs.entries[0];
        for (const field of ["time_virt_ps", "time_host_ms"]) {
            assert.equal(typeof entry[field], "string", `${field} 必须是字符串`);
            assert.match(entry[field], /^[0-9]+$/, `${field} 必须是纯十进制`);
        }
    });

    test("诊断包**恰好 4 个字段**，与前端镜像知道的名字逐字一致", () => {
        const diag = fixture.diag_event.AdminDiag.diag;
        assert.deepEqual(
            Object.keys(diag).sort(),
            [...ADMIN_DIAG_FIELDS].sort(),
            "`AdminDiagDto` 的字段名与前端镜像不一致 ⇒ ⇒ 契约层改了形状",
        );
    });

    test("前端镜像能**解析**同一份载荷（`decodeMessage`）", () => {
        for (const [member, variant] of [
            ["logs_event", "AdminLogs"],
            ["diag_event", "AdminDiag"],
        ]) {
            const envelope = eventMessage(1, fixture[member]);
            const decoded = decodeMessage(envelope);
            assert.equal(decoded.variant, variant, `${member} 的变体名必须逐字一致`);
            assert.deepEqual(decoded.value, fixture[member][variant]);
        }
    });

    test("前端镜像**逐字重建**契约层的两个事件", () => {
        assert.equal(
            JSON.stringify(Event.adminLogs(fixture.logs_event.AdminLogs.entries)),
            JSON.stringify(fixture.logs_event),
            "`AdminLogs` 的线上形态必须与契约层逐字相同",
        );
        assert.equal(
            JSON.stringify(Event.adminDiag(fixture.diag_event.AdminDiag.diag)),
            JSON.stringify(fixture.diag_event),
            "`AdminDiag` 的线上形态必须与契约层逐字相同",
        );
    });
});

// ---------------------------------------------------------------------------
// C-8 · 最小接线：订阅 admin topic 并拿到事件
// ---------------------------------------------------------------------------

/** 一个传输替身：只提供 M-05 路由用得到的那几个方法。 */
function createFakeTransport() {
    const messageHandlers = new Set();
    return {
        sent: [],
        onMessage(handler) {
            messageHandlers.add(handler);
            return () => messageHandlers.delete(handler);
        },
        onStatus() {
            return () => {};
        },
        sendCommand(message, options = {}) {
            this.sent.push({ message, reqId: options.reqId ?? null });
        },
        emit(frame) {
            for (const fn of [...messageHandlers]) fn(frame);
        },
    };
}

describe("管理员通道 · 最小接线（C-8）", () => {
    test("订阅 `admin.logs` 能拿到 `AdminLogs` 事件", async () => {
        const transport = createFakeTransport();
        const router = await mountCommandRouter({ transport });
        const seen = [];
        router.subscribe(EVENT_TOPIC.AdminLogs, (event) => seen.push(event));
        transport.emit({
            type: "event",
            seq: 1,
            sessionId: "sess-1",
            role: "embedded",
            reqId: null,
            payload: eventMessage(1, Event.adminLogs(fixture.logs_event.AdminLogs.entries)),
        });
        assert.equal(seen.length, 1, "订阅者必须收到 `admin.logs` 事件");
        assert.equal(seen[0].variant, "AdminLogs");
        assert.equal(seen[0].topic, "admin.logs");
        assert.deepEqual(seen[0].value.entries, fixture.logs_event.AdminLogs.entries);
        router.close();
    });

    test("订阅 `admin.diag` 能拿到 `AdminDiag` 事件", async () => {
        const transport = createFakeTransport();
        const router = await mountCommandRouter({ transport });
        const seen = [];
        router.subscribe(EVENT_TOPIC.AdminDiag, (event) => seen.push(event));
        transport.emit({
            type: "event",
            seq: 2,
            sessionId: "sess-1",
            role: "embedded",
            reqId: null,
            payload: eventMessage(2, Event.adminDiag(fixture.diag_event.AdminDiag.diag)),
        });
        assert.equal(seen.length, 1, "订阅者必须收到 `admin.diag` 事件");
        assert.equal(seen[0].variant, "AdminDiag");
        assert.equal(seen[0].topic, "admin.diag");
        assert.deepEqual(seen[0].value.diag, fixture.diag_event.AdminDiag.diag);
        router.close();
    });

    test("两个 admin topic **各订阅一次**（`ADMIN_TOPICS` 就是这两个）", async () => {
        const transport = createFakeTransport();
        const router = await mountCommandRouter({ transport });
        const seen = [];
        for (const topic of ADMIN_TOPICS) {
            router.subscribe(topic, (event) => seen.push(event.topic));
        }
        assert.deepEqual([...router.topics()].sort(), [...ADMIN_TOPICS].sort());
        transport.emit({
            type: "event",
            seq: 3,
            sessionId: "sess-1",
            role: "embedded",
            reqId: null,
            payload: eventMessage(3, Event.adminLogs([])),
        });
        transport.emit({
            type: "event",
            seq: 4,
            sessionId: "sess-1",
            role: "embedded",
            reqId: null,
            payload: eventMessage(4, Event.adminDiag({})),
        });
        assert.deepEqual(seen, ["admin.logs", "admin.diag"]);
        router.close();
    });

    test("**通配订阅**（`*`）也拿到 admin 事件（通用分发路径没被绕过）", async () => {
        const transport = createFakeTransport();
        const router = await mountCommandRouter({ transport });
        const seen = [];
        router.subscribe("*", (event) => seen.push(event.variant));
        transport.emit({
            type: "event",
            seq: 5,
            sessionId: "sess-1",
            role: "embedded",
            reqId: null,
            payload: eventMessage(5, Event.adminDiag({})),
        });
        assert.deepEqual(seen, ["AdminDiag"]);
        router.close();
    });
});
