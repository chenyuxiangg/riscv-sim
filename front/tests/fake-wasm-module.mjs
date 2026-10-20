// Test double for the wasm-pack ESM build of `crates/kernel`.
//
// The real module is produced by `tools/build-wasm.sh` into
// `front/assets/pkg/`, which is **gitignored** — the front-end gate
// therefore cannot depend on it (a machine that never built the wasm
// would fail the gate). This double implements the same two exports the
// worker uses:
//
//   default               — the wasm-pack init function
//   process_message(json) — one contract message in, a JSON **array** of
//                           contract messages out
//
// The behaviour is selected through the URL query, so every test case
// gets its own module instance:
//
//   ?mode=nop       → one `Tick` event
//   ?mode=two       → two events (the first one answers the command)
//   ?mode=throw     → `process_message` throws
//   ?mode=bad-json  → `process_message` returns a non-JSON string
//   ?mode=not-array → `process_message` returns a JSON object
//   ?mode=empty     → `process_message` returns an **empty** array
//   ?mode=init-fail → the init function throws
//   ?mode=init-fail-late → the init function throws **after** a delay
//                           (so a command sent right away is still in
//                           flight when the failure hits)
//
// The module is imported by the worker through the `self.__RISCV_WASM_URL__`
// test seam (see `front/kernel-worker.js`).

const mode = new URL(import.meta.url).searchParams.get("mode") ?? "nop";

/**
 * Build a contract `Message` (the frozen envelope, serde shape).
 *
 * @param {number} seq
 * @param {object} payload — the inner `EventPayload` value
 * @returns {object}
 */
function eventMessage(seq, payload) {
    return {
        schema_version: 1,
        direction: "Event",
        seq,
        payload: { Event: payload },
    };
}

/**
 * The wasm-pack init function (`--target web` exports it as `default`).
 *
 * @returns {Promise<void>}
 */
export default async function init() {
    if (mode === "init-fail") {
        throw new Error("假 WASM：init 失败");
    }
    if (mode === "init-fail-late") {
        // Fail **after** a delay: a command sent immediately is still
        // waiting for the init when the failure hits.
        await new Promise((r) => setTimeout(r, 50));
        throw new Error("假 WASM：init 延迟失败");
    }
}

/**
 * The bridge: one contract message in, a JSON array of contract
 * messages out.
 *
 * @param {string} json
 * @returns {string}
 */
export function process_message(json) {
    // The command is echoed into the answer where it helps the test
    // prove that the *command* reached the kernel untouched.
    const command = JSON.parse(json);
    const commandSeq = command?.seq ?? 0;

    if (mode === "throw") {
        throw new Error("假 WASM：内核炸了");
    }
    if (mode === "bad-json") {
        return "not json at all";
    }
    if (mode === "not-array") {
        return JSON.stringify(eventMessage(commandSeq + 1, { Tick: { elapsed_ps: 0, pc: 0 } }));
    }
    if (mode === "empty") {
        // An **empty** event list: the bridge's contract says at least one
        // event must come back, so the worker has to reject this rather
        // than answer the command with nothing.
        return JSON.stringify([]);
    }
    if (mode === "two") {
        return JSON.stringify([
            eventMessage(commandSeq + 1, { Tick: { elapsed_ps: 1000, pc: 4 } }),
            eventMessage(commandSeq + 2, { Halted: { reason: "Breakpoint", pc: 0x20 } }),
        ]);
    }
    // mode === "nop"
    return JSON.stringify([
        eventMessage(commandSeq + 1, { Tick: { elapsed_ps: 0, pc: 0 } }),
    ]);
}
