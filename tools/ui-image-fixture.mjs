// 固件加载 UI 真机门禁的**夹具**（STAGE-32 · v0.47 批 · B 组）。
//
// ⚠️ **这个模块只造字节，不做任何 I/O** ⇒ ⇒ 它可以被单独 import、被单独
// 验证 ⇒ ⇒ 真机门禁（`tools/browser-verify.mjs`）与「拿夹具跑一遍 headless
// runner」这两条路**用的是同一份字节**⇒ ⇒ 夹具本身出错时，两条路会**同时**
// 红 ⇒ ⇒ 不会「浏览器里红、命令行里绿」那种各说各话。
//
// ## 夹具的形状
//
// 一个**最小 RV32 ELF32**（一个 `PT_LOAD`），落址 `0x8400_0000`
// （QEMU virt 高段 RAM 窗口里）⇒ ⇒ 宿主自动选 `qemu-virt` 档 ⇒ ⇒ UART0
// （`0x1000_0000`）被映射 ⇒ ⇒ 夹具能真的往串口写字节。
//
// 程序（11 条指令）：
//
//   lui  x1, 0x84000    ; x1 = 0x8400_0000（固件自己的 RAM）
//   lui  x2, 0x10000    ; x2 = 0x1000_0000（UART0 基址）
//   addi x3, x0, 0x123  ; x3 = 0x123        ← B-2 的特征值
//   addi x4, x0, 'U' / sb x4, 0(x2)         ← UART0.THR
//   addi x4, x0, 'I' / sb x4, 0(x2)
//   addi x4, x0, 'L' / sb x4, 0(x2)         ← 串口上出现 "UIL"
//   sw   x3, 0(x1)      ; 把 0x123 写进固件自己的 RAM
//   jal  x0, 0          ; 自环
//
// ## 为什么用**编码器**而不是手写的十六进制
//
// 手抄机器码会把 `rs2` / `rs1` / `funct3` 这几个字段算错 ⇒ ⇒ 而且
// **看不出来**（不报错、不崩，只是写错了地方）⇒ ⇒ 本夹具的第一版就是这样：
// `sb` 被写成 `sb x2, 0(x0)` ⇒ ⇒ 字节进了 RAM，串口一个字节也没有 ⇒ ⇒
// 改用编码器 ⇒ ⇒ 字段由助记符**推**出来 ⇒ ⇒ {@link checkEncoderAgainstDemoFirmware}
// 拿仓内那份**已被内核执行过**的 demo 固件当标尺，把五个编码形式逐个钉住。

/**
 * 夹具镜像的 `PT_LOAD` 落址。
 *
 * ⚠️ 落在 QEMU virt 的高段 RAM 窗口里（0x8000_0000 + 128 MiB）⇒ ⇒ 宿主
 * 按「镜像**实际用到的地址**」自动选 `qemu-virt` 档 ⇒ ⇒ **UART0 有映射**。
 */
export const UI_IMAGE_ELF_PADDR = 0x8400_0000;

/** 夹具往 UART0 的 THR 写的三个字节（B-3 的判据字面量）。 */
export const UI_IMAGE_ELF_MARKER = "UIL";

/** 夹具留在 `x3` 里的特征值（B-2 的判据：与 demo 固件的 `x1 = 0x2a` 完全不同）。 */
export const UI_IMAGE_ELF_X3 = 0x00000123;

/**
 * U 型编码（`LUI` / `AUIPC`）。
 *
 * @param {number} opcode
 * @param {number} rd
 * @param {number} imm — 20 位
 * @returns {number}
 */
export function uType(opcode, rd, imm) {
    if (!Number.isInteger(imm) || imm < 0 || imm > 0xfffff) {
        throw new Error(`U 型立即数越界：${imm}`);
    }
    return ((((imm & 0xfffff) << 12) | ((rd & 0x1f) << 7) | (opcode & 0x7f)) >>> 0);
}

/**
 * I 型编码（`ADDI` / `JALR` …）。
 *
 * @param {number} opcode
 * @param {number} rd
 * @param {number} rs1
 * @param {number} imm — 12 位有符号
 * @returns {number}
 */
export function iType(opcode, rd, rs1, imm) {
    if (!Number.isInteger(imm) || imm < -2048 || imm > 2047) {
        throw new Error(`I 型立即数越界：${imm}`);
    }
    return ((((imm & 0xfff) << 20) | ((rs1 & 0x1f) << 15)
        | ((rd & 0x1f) << 7) | (opcode & 0x7f)) >>> 0);
}

/**
 * S 型编码（`SB` / `SW` …）。
 *
 * @param {number} opcode
 * @param {number} funct3
 * @param {number} rs1
 * @param {number} rs2
 * @param {number} imm — 12 位有符号
 * @returns {number}
 */
export function sType(opcode, funct3, rs1, rs2, imm) {
    if (!Number.isInteger(imm) || imm < -2048 || imm > 2047) {
        throw new Error(`S 型立即数越界：${imm}`);
    }
    const imm12 = imm & 0xfff;
    return (((((imm12 >> 5) & 0x7f) << 25) | ((rs2 & 0x1f) << 20) | ((rs1 & 0x1f) << 15)
        | ((funct3 & 0x7) << 12) | ((imm12 & 0x1f) << 7) | (opcode & 0x7f)) >>> 0);
}

/**
 * R 型编码（`ADD` / `SUB` …）。
 *
 * @param {number} opcode
 * @param {number} funct3
 * @param {number} rd
 * @param {number} rs1
 * @param {number} rs2
 * @param {number} [funct7]
 * @returns {number}
 */
export function rType(opcode, funct3, rd, rs1, rs2, funct7 = 0) {
    const word = ((funct7 & 0x7f) << 25)
        | ((rs2 & 0x1f) << 20)
        | ((rs1 & 0x1f) << 15)
        | ((funct3 & 0x7) << 12)
        | ((rd & 0x1f) << 7)
        | (opcode & 0x7f);
    return word >>> 0;
}

/**
 * J 型编码（`JAL`）—— 立即数是**字节**偏移。
 *
 * ⚠️ J 型的立即数字段是**错开**的：`imm[20]` 在 bit31、`imm[10:1]` 在
 * bit30:21、`imm[11]` 在 bit20、`imm[19:12]` 在 bit19:12。
 *
 * @param {number} opcode
 * @param {number} rd
 * @param {number} imm — 21 位有符号的**字节**偏移（必须是偶数）
 * @returns {number}
 */
export function jType(opcode, rd, imm) {
    if (!Number.isInteger(imm) || imm % 2 !== 0 || imm < -1048576 || imm > 1048574) {
        throw new Error(`J 型立即数越界：${imm}`);
    }
    const imm21 = imm & 0x1fffff;
    return (((((imm21 >> 20) & 1) << 31) | (((imm21 >> 1) & 0x3ff) << 21)
        | (((imm21 >> 11) & 1) << 20) | (((imm21 >> 12) & 0xff) << 12)
        | ((rd & 0x1f) << 7) | (opcode & 0x7f)) >>> 0);
}

/**
 * 编码器自检：**逐字节**复现仓内那份**已被内核执行过**的 demo 固件。
 *
 * ⚠️ **为什么必须自检**：编码器写错时**不报错**（只有立即数越界才抛）⇒ ⇒
 * 夹具会「跑得好好的，只是没做它该做的事」⇒ ⇒ 用一份**已知正确**的字节当
 * 标尺（`front/dev/demo-firmware.js` 的 `DEMO_FIRMWARE_BYTES` —— 内核门禁
 * 的 `x1 = 0x2a` 判据就靠它）⇒ ⇒ 五个编码形式（U / I / S / J / R）**逐个**
 * 被钉住。
 *
 * @returns {{ok: boolean, detail: string}}
 */
export function checkEncoderAgainstDemoFirmware() {
    const encoded = [
        iType(0x13, 1, 0, 42),       // 0x00  addi x1, x0, 42    → 0x02A00093
        iType(0x13, 2, 0, 7),        // 0x04  addi x2, x0, 7     → 0x00700113
        rType(0x33, 0, 3, 1, 2),     // 0x08  add  x3, x1, x2    → 0x002081B3
        sType(0x23, 2, 0, 3, 0x100), // 0x0c  sw   x3, 0x100(x0) → 0x10302023
        jType(0x6f, 0, 4),           // 0x10  jal  x0, +4        → 0x0040006F
        uType(0x37, 5, 0x40000),     // 0x14  lui  x5, 0x40000   → 0x400002B7
        iType(0x13, 6, 0, 1),        // 0x18  addi x6, x0, 1     → 0x00100313
        sType(0x23, 2, 5, 6, 4),     // 0x1c  sw   x6, 4(x5)     → 0x0062A223
        iType(0x13, 4, 0, 1),        // 0x20  addi x4, x0, 1     → 0x00100213
        jType(0x6f, 0, 0),           // 0x24  jal  x0, 0         → 0x0000006F
    ];
    const known = [
        0x02a00093, 0x00700113, 0x002081b3, 0x10302023, 0x0040006f,
        0x400002b7, 0x00100313, 0x0062a223, 0x00100213, 0x0000006f,
    ];
    const bad = [];
    encoded.forEach((word, i) => {
        if ((word >>> 0) !== known[i]) {
            bad.push(`第 ${i} 条：编码得 0x${(word >>> 0).toString(16)}，`
                + `demo 固件里是 0x${known[i].toString(16)}`);
        }
    });
    return {
        ok: bad.length === 0,
        detail: bad.length === 0
            ? `逐字节复现了 demo 固件的 ${known.length} 条指令`
            : bad.join("；"),
    };
}

/**
 * 造夹具镜像的**机器码**（11 条指令）。
 *
 * @returns {number[]}
 */
export function buildUiImageWords() {
    return [
        uType(0x37, 1, 0x84000),   // lui  x1, 0x84000   ; x1 = 0x8400_0000（固件自己的 RAM）
        uType(0x37, 2, 0x10000),   // lui  x2, 0x10000   ; x2 = 0x1000_0000（UART0 基址）
        iType(0x13, 3, 0, 0x123),  // addi x3, x0, 0x123 ; x3 = 0x123（特征值）
        iType(0x13, 4, 0, 85),     // addi x4, x0, 85    ; 'U'
        sType(0x23, 0, 2, 4, 0),   // sb   x4, 0(x2)     ; UART0.THR = 'U'
        iType(0x13, 4, 0, 73),     // addi x4, x0, 73    ; 'I'
        sType(0x23, 0, 2, 4, 0),   // sb   x4, 0(x2)     ; UART0.THR = 'I'
        iType(0x13, 4, 0, 76),     // addi x4, x0, 76    ; 'L'
        sType(0x23, 0, 2, 4, 0),   // sb   x4, 0(x2)     ; UART0.THR = 'L'
        sType(0x23, 2, 1, 3, 0),   // sw   x3, 0(x1)     ; 把 0x123 写进固件自己的 RAM
        jType(0x6f, 0, 0),         // jal  x0, 0         ; 自环
    ];
}

/**
 * 造夹具镜像（最小 RV32 ELF32 · 一个 `PT_LOAD`）。
 *
 * ⚠️ **形状照着内核自己的测试夹具**（`crates/kernel/src/m06_elf/tests.rs`
 * 的 `make_minimal_elf` 与 `kernel_host/tests_osz_wiring.rs` 的
 * `minimal_elf`）⇒ ⇒ M-06 一定认得它 ⇒ ⇒ 真机门禁测的是**界面那条装载
 * 路**，不是「这个 ELF 能不能被解析」。
 *
 * @returns {Buffer}
 */
export function buildUiImageElf() {
    const EHDR = 52;
    const PHDR = 32;
    const words = buildUiImageWords();
    const text = Buffer.alloc(words.length * 4);
    words.forEach((w, i) => text.writeUInt32LE(w >>> 0, i * 4));

    const out = Buffer.alloc(EHDR + PHDR + text.length);
    out[0] = 0x7f;
    out[1] = 0x45;
    out[2] = 0x4c;
    out[3] = 0x46; // \x7fELF
    out[4] = 1; // EI_CLASS = ELFCLASS32
    out[5] = 1; // EI_DATA  = ELFDATA2LSB
    out[6] = 1; // EI_VERSION
    out.writeUInt16LE(2, 16); // e_type = ET_EXEC
    out.writeUInt16LE(0xf3, 18); // e_machine = EM_RISCV
    out.writeUInt32LE(1, 20); // e_version
    out.writeUInt32LE(UI_IMAGE_ELF_PADDR, 24); // e_entry
    out.writeUInt32LE(EHDR, 28); // e_phoff
    out.writeUInt16LE(EHDR, 40); // e_ehsize
    out.writeUInt16LE(PHDR, 42); // e_phentsize
    out.writeUInt16LE(1, 44); // e_phnum
    const p = EHDR;
    out.writeUInt32LE(1, p); // p_type = PT_LOAD
    out.writeUInt32LE(EHDR + PHDR, p + 4); // p_offset
    out.writeUInt32LE(UI_IMAGE_ELF_PADDR, p + 8); // p_vaddr
    out.writeUInt32LE(UI_IMAGE_ELF_PADDR, p + 12); // p_paddr
    out.writeUInt32LE(text.length, p + 16); // p_filesz
    out.writeUInt32LE(text.length, p + 20); // p_memsz
    out.writeUInt32LE(5, p + 24); // p_flags = PF_R | PF_X
    out.writeUInt32LE(4, p + 28); // p_align
    text.copy(out, EHDR + PHDR);
    return out;
}
