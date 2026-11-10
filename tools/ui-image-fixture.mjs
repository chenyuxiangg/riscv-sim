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
 * **第二份**夹具的特征（STAGE-33 · v0.48 批）—— 给「**页头**那条装载路径」用。
 *
 * ⚠️ **为什么必须有两份**：真机门禁现在有**两条**装载路径 ⇒ ⇒ 若两条装
 * **同一个**固件 ⇒ ⇒ 第二条是「装同一个东西」⇒ ⇒ **恒等变换** ⇒ ⇒ 它的
 * 判据**恒绿**（零判别力）⇒ ⇒ ⇒ 所以第二条必须装**能区分**的那份。
 */
export const UI_IMAGE_ELF_MARKER_B = "UIB";
export const UI_IMAGE_ELF_X3_B = 0x00000456;

/** **引脚级中断**那条固件的**成功标记**（guest 领到了那个源 ⇒ ⇒ 打这三个字符）。 */
export const GPIO_PIN_IRQ_ELF_MARKER = "PIN";

/**
 * **引脚级中断**那条固件的**失败标记**（预算用完**还没**领到那个源）。
 *
 * ⚠️ **为什么要有它**：真机门禁要能区分「**没领到**」与「**根本没跑**」
 * ⇒ ⇒ ⇒ 前者打 `X`、后者**什么都没有**⇒ ⇒ ⇒ 两种情况的**判据不同**。
 */
export const GPIO_PIN_IRQ_ELF_FAIL_MARKER = "X";

/**
 * **引脚复用**那条固件（v0.76 · STAGE-52 · B-171 / B-172）的**阶段标记**。
 *
 * ⚠️ **两个时刻两个源**：同一引脚（`GPIO0.pin3`）先选 `uart_tx`（⇒ 源 3，
 * 打 `A3`），再切到 `gpio_in`（⇒ 源 11，打 `B11`）⇒ ⇒ console 上「先 `A3`
 * 后 `B11`」= **切换跟着变**（B-172）⇒ ⇒ 单独出现 `B11` = **通电**（B-171）。
 *
 * ⚠️ **失败标记沿用 `X`**（与 v0.74 那份共享 ⇒ ⇒ 判据分得开）。
 */
export const PINMUX_IRQ_ELF_MARKER_A3 = "A3";
export const PINMUX_IRQ_ELF_MARKER_B11 = "B11";

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
export function iType(opcode, rd, rs1, imm, funct3 = 0) {
    if (!Number.isInteger(imm) || imm < -2048 || imm > 2047) {
        throw new Error(`I 型立即数越界：${imm}`);
    }
    return ((((imm & 0xfff) << 20) | ((rs1 & 0x1f) << 15)
        | ((funct3 & 0x7) << 12) | ((rd & 0x1f) << 7) | (opcode & 0x7f)) >>> 0);
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
 * **B 型编码**（条件分支 · `BEQ` / `BNE` …）。
 *
 * ⚠️ **v0.74（STAGE-50 · B-156 的固件）**：引脚级那条真机判据要 guest
 *   **轮询** PLIC 的 claim 寄存器 ⇒ ⇒ ⇒ **必须有分支** ⇒ ⇒ ⇒ 之前这个模块
 *   只有 U / I / S / R / J 五种形式（⇒ ⇒ 那条固件**轮询不了**）⇒ ⇒ ⇒
 *   **补上**这第六种。
 *
 * ⚠️ **立即数是「字节偏移」**（相对于**本条指令**的 PC）⇒ ⇒ ⇒ 与汇编器
 *   写的那份**同义**（⇒ ⇒ 判据能对着汇编读）。
 *
 * @param {number} opcode
 * @param {number} rs1
 * @param {number} rs2
 * @param {number} funct3 — `0` = BEQ · `1` = BNE
 * @param {number} imm — 字节偏移（**偶数**）
 * @returns {number}
 */
export function bType(opcode, rs1, rs2, funct3, imm) {
    if (!Number.isInteger(imm) || imm % 2 !== 0) {
        throw new Error(`B 型立即数必须是偶数的字节偏移：${imm}`);
    }
    if (imm < -4096 || imm > 4094) {
        throw new Error(`B 型立即数超出 ±4 KiB：${imm}`);
    }
    const i = imm & 0xfff;
    // ⚠️ **符号位是「13 位」的那个**（imm[12]）⇒ ⇒ **不能**从 12 位掩码里
    //   取（⇒ ⇒ 掩码只有 12 位 ⇒ ⇒ 取出来**永远是 0** ⇒ ⇒ **负偏移会跳到
    //   +4096 附近** ⇒ ⇒ ⇒ 实测踩到过：分支跳飞 ⇒ ⇒ 访客**trap**）。
    const b12 = imm < 0 ? 1 : 0;
    const b11 = (i >> 11) & 1;
    const b105 = (i >> 5) & 0x3f;
    const b41 = (i >> 1) & 0xf;
    return ((((b12 << 31) >>> 0)
        | (b105 << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (funct3 << 12)
        | (b41 << 8)
        | (b11 << 7)
        | (opcode & 0x7f)) >>> 0);
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
 * ⚠️ **可参数化**（STAGE-33 · v0.48 批）：真机门禁现在有**两条**装载路径
 * （产品视图那条 + 页头那条）⇒ ⇒ 两条路径必须装**不同的**固件 ⇒ ⇒ 否则
 * 第二条路径是「装同一个东西」⇒ ⇒ 那是**恒等变换** ⇒ ⇒ 它的判据**恒绿**
 * ⇒ ⇒ ⇒ **一个判别力来源是「被观测的量真的变了」**。
 *
 * ⚠️ **默认值逐字不变**（`x3 = 0x123`、标记 `"UIL"`）⇒ ⇒ STAGE-32 那一批
 * 的夹具字节**一个字节都不变** ⇒ ⇒ 上一批的门禁结论仍然成立。
 *
 * @param {{x3Value?: number, marker?: string}} [options]
 * @returns {number[]}
 */
export function buildUiImageWords(options = {}) {
    const x3Value = options.x3Value ?? UI_IMAGE_ELF_X3;
    const marker = String(options.marker ?? UI_IMAGE_ELF_MARKER);
    if (marker.length !== 3) {
        throw new Error(`标记必须是三个字符：${marker}`);
    }
    return [
        uType(0x37, 1, 0x84000),   // lui  x1, 0x84000   ; x1 = 0x8400_0000（固件自己的 RAM）
        uType(0x37, 2, 0x10000),   // lui  x2, 0x10000   ; x2 = 0x1000_0000（UART0 基址）
        iType(0x13, 3, 0, x3Value), // addi x3, x0, 0x123 ; x3 = 特征值
        iType(0x13, 4, 0, marker.charCodeAt(0)),   // addi x4, x0, 'U'
        sType(0x23, 0, 2, 4, 0),   // sb   x4, 0(x2)     ; UART0.THR = 第 1 个字节
        iType(0x13, 4, 0, marker.charCodeAt(1)),   // addi x4, x0, 'I'
        sType(0x23, 0, 2, 4, 0),   // sb   x4, 0(x2)     ; UART0.THR = 第 2 个字节
        iType(0x13, 4, 0, marker.charCodeAt(2)),   // addi x4, x0, 'L'
        sType(0x23, 0, 2, 4, 0),   // sb   x4, 0(x2)     ; UART0.THR = 第 3 个字节
        sType(0x23, 2, 1, 3, 0),   // sw   x3, 0(x1)     ; 把特征值写进固件自己的 RAM
        jType(0x6f, 0, 0),         // jal  x0, 0         ; 自环
    ];
}

/**
 * 造夹具镜像（最小 RV32 ELF32 · 一个 `PT_LOAD`）。
 *
 * ⚠️ **形状照着内核自己的测试夹具**（`crates/kernel/src/m06_elf/tests.rs`
 * 的 `make_minimal_elf` 与 `kernel_host/tests_osz_bridge.rs` 的
 * `minimal_elf`）⇒ ⇒ M-06 一定认得它 ⇒ ⇒ 真机门禁测的是**界面那条装载
 * 路**，不是「这个 ELF 能不能被解析」。
 *
 * @param {{x3Value?: number, marker?: string}} [options]
 * @returns {Buffer}
 */
export function buildUiImageElf(options = {}) {
    return buildElfFromWords(buildUiImageWords(options), UI_IMAGE_ELF_PADDR);
}

/**
 * **引脚级中断**那条固件的机器码（v0.74 · STAGE-50 · B-156 的固件）。
 *
 * ## 它要证明什么
 *
 * guest **真的**收到那个源的中断 ⇒ ⇒ ⇒ guest 必须**自己**去 PLIC 的 claim
 * 寄存器里**领**那个源 ⇒ ⇒ ⇒ 「内核把挂起位置上了」这件事**在 guest 那一侧
 * 可观察**（⇒ ⇒ 不是「描述被接受了」那种弱证据）。
 *
 * ## 机器码（**逐条**注释 ⇒ ⇒ 禁手抄十六进制）
 *
 *   lui  x1, 0x40000        ; x1 = 0x4000_0000（qemu-virt 的 GPIO0）
 *   lui  x2, 0xc000         ; x2 = 0x0c00_0000（PLIC）
 *   lui  x3, 0x10000        ; x3 = 0x1000_0000（UART0）
 *   lui  x8, 0xc200         ; x8 = 0x0c20_0000（PLIC 的 context 区）
 *   lui  x10, 0xc002        ; x10 = 0x0c00_2000（PLIC 的 enable 区）
 *   addi x4, x0, 1          ; 优先级 = 1（门限默认 0 ⇒ ⇒ 必须 > 门限）
 *   sw   x4, 0x2c(x2)       ; PLIC 的 priority[11] = 1
 *   addi x5, x0, 1
 *   slli x5, x5, 11         ; x5 = 1 << 11（enable 的那一位）
 *   sw   x5, 0(x10)         ; PLIC 的 enable[0] 的第 11 位
 *   addi x6, x0, 8          ; x6 = 1 << 3（引脚 3）
 *   sw   x6, 0xc(x1)        ; GPIO0.TOGGLE ⇒ ⇒ 引脚 3 出现**边沿**
 *   lui  x7, 1              ; x7 = 0x1000（预算的高半 ⇒ ⇒ 12 位立即数**放不下** 5000）
 *   addi x7, x7, 904        ; x7 = 5000（轮询预算 · **有界** ⇒ ⇒ 失败时也会退出）
 *   loop: lw  x9, 4(x8)     ; 读 PLIC 的 claim（ctx 0）
 *         bne  x9, x0, ok   ; 领到了 ⇒ ⇒ 跳到成功
 *         addi x7, x7, -1
 *         bne  x7, x0, loop  ; 还有预算 ⇒ ⇒ 再读一次
 *         addi x11, x0, 'X' ; ⚠️ **失败标记**（预算用完还没领到）
 *         sb   x11, 0(x3)
 *         jal  x0, 0        ; 自环（结束）
 *   ok:  addi x11, x0, 'P'  ; ✅ **成功标记**（"PIN"）
 *         sb   x11, 0(x3)
 *         addi x11, x0, 'I'
 *         sb   x11, 0(x3)
 *         addi x11, x0, 'N'
 *         sb   x11, 0(x3)
 *         jal  x0, 0
 *
 * ⚠️ ⚠️ **为什么预算是 5000 而不是 200**（v0.74 批 · STAGE-50 · 施工期订正）：
 *   **一轮轮询 = 4 条指令** ⇒ ⇒ 200 轮 ≈ **800 条指令** ⇒ ⇒ 而**一个批次**
 *   （`DEFAULT_INSTRUCTION_BUDGET`）是 **10 000 条指令** ⇒ ⇒ ⇒ 整个轮询
 *   **在同一个批次里就跑完了** ⇒ ⇒ ⇒ 而宿主**只在批次边界**调
 *   `sync_interrupts()`（`on_run` 的「跑之前 / 跑之后」各一次）⇒ ⇒ ⇒
 *   ⇒ ⇒ **那个源永远没被抬起来** ⇒ ⇒ ⇒ guest 读到的 claim **恒为 0** ⇒ ⇒ ⇒
 *   ⇒ ⇒ ⇒ 于是固件打的是**失败标记 `X`** ⇒ ⇒ ⇒ **看起来像「引脚级连线没通电」
 *   ⇒ ⇒ ⇒ 其实只是「固件没活到下一个批次边界」**⇒ ⇒ ⇒ ⇒ 5000 轮 ≈
 *   **20 000 条指令 ≈ 两个批次** ⇒ ⇒ ⇒ ⇒ 必**跨过**一次批次边界 ⇒ ⇒ ⇒ ⇒
 *   ⇒ ⇒ ⇒ ⇒ 第二次 `sync_interrupts()` 才把那个源抬起来 ⇒ ⇒ ⇒ ⇒ guest
 *   **领得到** ⇒ ⇒ ⇒ ⇒ ⇒ ⚠️ **教训**：**「guest 轮询等中断」这件事 ⇒ ⇒ 轮询
 *   预算必须大于「一个批次的指令数」** ⇒ ⇒ ⇒ 否则判据**恒假**（⇒ ⇒ 且症状
 *   与「功能没做」**长得一样** ⇒ ⇒ ⇒ 排查成本极高）。
 *
 * ⚠️ **为什么预算要用两条指令拼**（`lui` + `addi`）：**RISC-V 的 `addi` 立即数
 *   是 12 位有符号**（-2048..2047）⇒ ⇒ ⇒ **5000 放不下**⇒ ⇒ ⇒ 若硬写
 *   `addi x7, x0, 5000` ⇒ ⇒ ⇒ 编码器只会留下低 12 位 ⇒ ⇒ ⇒ 预算变成
 *   **904** ⇒ ⇒ ⇒ **又不够跨批次了** ⇒ ⇒ ⇒ ⇒ 症状**完全一样** ⇒ ⇒ ⇒ ⇒
 *   ⇒ ⇒ ⇒ **「立即数放不下」这件事必须当场换编码方式**（⇒ ⇒ 而不是「改小
 *   数值」⇒ ⇒ 那是**把判据改成恒假**）。
 *
 * ⚠️ **为什么成功标记是三个字符**（与 `UIL` / `UIB` 同款）⇒ ⇒ 免得跟别的
 *   段的输出撞上（⇒ ⇒ 撞上了就**恒真**了 ⇒ ⇒ 零判别力）。
 *
 * ⚠️ **为什么要有失败标记 `X`**：预算用完 ⇒ ⇒ 说明**没领到** ⇒ ⇒ 写一个
 *   **看得见**的标记（⇒ ⇒ 真机门禁能区分「没领到」与「根本没跑」）。
 *
 * @returns {number[]}
 */
export function buildGpioPinIrqWords() {
    // ⚠️ **指令下标**（分支偏移是**算**出来的 ⇒ ⇒ 改顺序时**必须**重算）。
    //
    // ⚠️ **下标全部写成常量**（不许出现裸的 14 / 16）⇒ ⇒ 插一条指令时
    //   **只改常量**、偏移**自动**跟上 ⇒ ⇒ ⇒ 不会「插了忘了改偏移」⇒ ⇒ ⇒
    //   ⇒ 那类错的症状是「跳到垃圾地址」⇒ ⇒ ⇒ 极难查。
    const LOOP = 14;       // lw  x9, 4(x8)
    const BR_CLAIM = 15;   // bne x9, x0, ok
    const BR_LOOP = 17;    // bne x7, x0, loop
    const OK = 22;         // addi x11, x0, 'P'
    return [
        uType(0x37, 1, 0x40000),      // 0: lui  x1, 0x40000
        uType(0x37, 2, 0xc000),       // 1: lui  x2, 0xc000
        uType(0x37, 3, 0x10000),      // 2: lui  x3, 0x10000
        uType(0x37, 8, 0xc200),       // 3: lui  x8, 0xc200
        uType(0x37, 10, 0xc002),      // 4: lui  x10, 0xc002
        iType(0x13, 4, 0, 1),        // 5: addi x4, x0, 1
        sType(0x23, 2, 2, 4, 0x2c),  // 6: sw   x4, 0x2c(x2)
        iType(0x13, 5, 0, 1),        // 7: addi x5, x0, 1
        iType(0x13, 5, 5, 11, 1),   // 8: slli x5, x5, 11（funct3 = 001 ⇒ 移位）
        sType(0x23, 2, 10, 5, 0),    // 9: sw   x5, 0(x10)
        iType(0x13, 6, 0, 8),        // 10: addi x6, x0, 8
        sType(0x23, 2, 1, 6, 0xc),   // 11: sw   x6, 0xc(x1)
        uType(0x37, 7, 1),            // 12: lui  x7, 1（预算的高半）
        iType(0x13, 7, 7, 904),      // 13: addi x7, x7, 904 ⇒ x7 = 5000
        iType(0x03, 9, 8, 4, 2),     // 14: lw   x9, 4(x8)（funct3 = 010 ⇒ 字读）
        bType(0x63, 9, 0, 1, (OK - BR_CLAIM) * 4),   // 15: bne  x9, x0, ok
        iType(0x13, 7, 7, -1),       // 16: addi x7, x7, -1
        bType(0x63, 7, 0, 1, (LOOP - BR_LOOP) * 4), // 17: bne  x7, x0, loop
        iType(0x13, 11, 0, 0x58),    // 18: addi x11, x0, 'X'
        sType(0x23, 0, 3, 11, 0),    // 19: sb   x11, 0(x3)
        jType(0x6f, 0, 0),           // 20: jal  x0, 0
        iType(0x13, 0, 0, 0),        // 21: （填充 ⇒ ⇒ 目标下标固定）
        iType(0x13, 11, 0, 0x50),    // 22: addi x11, x0, 'P'
        sType(0x23, 0, 3, 11, 0),    // 23: sb   x11, 0(x3)
        iType(0x13, 11, 0, 0x49),    // 24: addi x11, x0, 'I'
        sType(0x23, 0, 3, 11, 0),    // 25: sb   x11, 0(x3)
        iType(0x13, 11, 0, 0x4e),    // 26: addi x11, x0, 'N'
        sType(0x23, 0, 3, 11, 0),    // 27: sb   x11, 0(x3)
        jType(0x6f, 0, 0),           // 28: jal  x0, 0
    ];
}

/**
 * **引脚级中断**那条固件的镜像（最小 RV32 ELF32）。
 *
 * @returns {Buffer}
 */
export function buildGpioPinIrqElf() {
    return buildElfFromWords(buildGpioPinIrqWords(), UI_IMAGE_ELF_PADDR);
}

/**
 * **引脚复用**那条固件的机器码（v0.76 · STAGE-52 · B-171 / B-172）。
 *
 * ⚠️ **为什么要有它**：pinmux 的「通电」判据必须是「固件**写选择寄存器** ⇒
 * 活动 ⇒ guest **自己**去 PLIC claim 到**那个源**」⇒ ⇒ demo 固件**不写**
 * pinmux 寄存器 ⇒ ⇒ 必须**自造**一份固件（同 v0.74 那套机器码纪律）。
 *
 * ⚠️ **它做什么**（地址按 [`BV_PINMUX_BOARD`] 的布局写死）：
 *
 * 1. 写 `PINMUX` 选择寄存器（`0x4000_1000 + 0x4×pin3 = 0x4000_100C`）← 0
 *    （功能 `uart_tx` ⇒ PLIC 源 3）；
 * 2. 使能 PLIC 源 3（`0xc000_0000 + 0x0c`）⇒ 设 `mie.MEIE` + `mstatus.MIE`
 *    ⇒ `mtvec` = 本固件的 handler；
 * 3. GPIO `DIR` ← 8 + `TOGGLE`（沿 1）⇒ 中断 ⇒ handler claim 到源 3 ⇒
 *    打 `A3`（阶段 0）⇒ complete ⇒ `mret`；
 * 4. 睡眠后**切**选择 ← 1（功能 `gpio_in` ⇒ PLIC 源 11）+ 使能源 11
 *    （`0xc000_0000 + 0x2c`）⇒ `TOGGLE`（沿 2）⇒ handler claim 到源 11 ⇒
 *    打 `B11`（阶段 1）⇒ complete ⇒ `mret`；
 * 5. 打错的源 ⇒ 打 `X`（失败标记）。
 *
 * ⚠️ **指令下标全部写成常量**（先例纪律）⇒ ⇒ 插指令只改常量、偏移自动跟上。
 *
 * ⚠️ **claim / complete 在 `0xc020_0004`**（PLIC base + `0x200004`）⇒ ⇒
 * `0x2004` 超 I 型 12 位 ⇒ ⇒ 用 `lui x14, 0xc020` + `4(x14)` 取址。
 *
 * @returns {number[]}
 */
export function buildPinmuxIrqWords() {
    // ⚠️ **睡眠预算必须跨批次**（v0.74 · STAGE-50 施工期教训）：宿主只在
    //   批次边界（`DEFAULT_INSTRUCTION_BUDGET` = 10 000 条）调
    //   `sync_interrupts()` ⇒ ⇒ 预算 = 6000 轮 × 2 条/轮 = **12 000 条**
    //   ⇒ **稳跨一次边界** ⇒ 中断**真的**投得进来（5000 会恰好卡在边界上）。
    //
    // ⚠️ **立即数 ≤ 2047**：6000 = `lui 0x1` + `addi 1904`（两条拼）。
    //   `addi x?, x?, 6000` 直接写 ⇒ 编码器抛 I 型越界。enable 位 11
    //   = 2048 同样放不下 ⇒ `lui 0x1` + `addi -0x800` 拼。
    //
    // ⚠️ **判源用 x12 阶段**：阶段 0 期望**源 3**（`uart_tx`）⇒ 打 `A3`；
    //   阶段 1 期望**源 11**（`gpio_in`）⇒ 打 `B11`；claim 到的源不对 ⇒
    //   打 `X`。
    //
    // ⚠️ **v0.76 夹具钉正（r46）**：PLIC 必须**双写**（priority 区
    //   `0x0c/0x2c` 写 1 ⇒ claim 判据 priority>threshold；enable 区
    //   `0xc0002000` 写位掩码 8 / 2048 ⇒ 源 3 / 源 11 使能）。只写 priority
    //   （旧版）⇒ PLIC 拒交 ⇒ 中断永不投递。enable 区偏移 0x2000 超出
    //   S 型立即数上限 2047 ⇒ **独立基址寄存器 x15**（`lui 0xc0002` 一步）。
    const LOOP1 = 28;          // bne x13, x0, loop1
    const LOOP2 = 40;          // bne x13, x0, loop2
    const HANDLER = 44;        // 中断 handler 起点（mtvec 落点）
    const STAGE1 = 51;         // 阶段 1 判源入口
    const PRINT_B11 = 54;      // 打 B11
    const PRINT_A3 = 62;       // 打 A3
    const FAIL = 68;           // 打 X
    const WORDS = [];
    const w = (v) => WORDS.push(v);
    w(uType(0x37, 1, 0x40000));       // 0:  lui  x1, 0x40000      ⇒ x1 = 0x4000_0000 (GPIO0)
    w(uType(0x37, 2, 0xc0000));       // 1:  lui  x2, 0xc0000      ⇒ x2 = 0xc000_0000 (PLIC)
    w(uType(0x37, 3, 0x10000));       // 2:  lui  x3, 0x10000      ⇒ x3 = 0x1000_0000 (UART0)
    w(uType(0x37, 5, 0x40001));       // 3:  lui  x5, 0x40001      ⇒ x5 = 0x4000_1000 (PINMUX)
    w(iType(0x13, 5, 5, 0));          // 4:  addi x5, x5, 0        ⇒ 保号（指令号不动）
    w(iType(0x13, 4, 0, 0));          // 5:  addi x4, x0, 0        ⇒ 选择 0（uart_tx ⇒ 源 3）
    w(sType(0x23, 2, 5, 4, 0x0c));    // 6:  sw   x4, 0x0c(x5)     ⇒ pin3 选择寄存器
    w(iType(0x13, 4, 0, 1));          // 7:  addi x4, x0, 1        ⇒ priority 值
    w(sType(0x23, 2, 2, 4, 0x0c));    // 8:  sw   x4, 0x0c(x2)     ⇒ PLIC 源 3 priority ← 1
    w(uType(0x37, 15, 0xc0002));      // 9:  lui  x15, 0xc0002     ⇒ x15 = 0xc000_2000（enable 区基址）
    w(iType(0x13, 4, 0, 8));          // 10: addi x4, x0, 8        ⇒ enable 位 3（1<<3）
    w(sType(0x23, 2, 15, 4, 0x0));    // 11: sw   x4, 0(x15)       ⇒ PLIC enable[0] 位 3
    w(uType(0x37, 6, 0x1));           // 12: lui  x6, 0x1          ⇒ x6 = 0x1000
    w(iType(0x13, 6, 6, -2048));      // 13: addi x6, x6, -2048    ⇒ x6 = 0x800（MEIE · 拆两条免 I 型越界）
    w(iType(0x73, 7, 6, 0x304, 0b010)); // 14: csrrs x7, mie, x6
    w(iType(0x13, 6, 0, 8));          // 15: addi x6, x0, 8        ⇒ MIE
    w(iType(0x73, 7, 6, 0x300, 0b010)); // 16: csrrs x7, mstatus, x6
    w(uType(0x37, 6, 0x80000));       // 17: lui  x6, 0x80000      ⇒ 固件落址高（0x8000_0000）
    w(iType(0x13, 6, 6, HANDLER * 4)); // 18: addi x6, x6, HANDLER*4
    w(iType(0x73, 0, 6, 0x305, 0b010)); // 19: csrrs x0, mtvec, x6
    w(iType(0x13, 6, 0, 8));          // 20: addi x6, x0, 8        ⇒ 1<<3
    w(sType(0x23, 2, 1, 6, 0x0));     // 21: sw   x6, 0(x1)        ⇒ GPIO DIR ← 8
    w(sType(0x23, 2, 1, 6, 0x0c));    // 22: sw   x6, 0x0c(x1)     ⇒ TOGGLE（沿 1：0→8）
    w(iType(0x13, 12, 0, 0));         // 23: addi x12, x0, 0       ⇒ 阶段 0（期望源 3）
    w(uType(0x37, 14, 0xc0200));      // 24: lui  x14, 0xc0200     ⇒ x14 = 0xc020_0000（claim/complete 高）
    w(uType(0x37, 13, 0x1));          // 25: lui  x13, 0x1
    w(iType(0x13, 13, 13, 1904));     // 26: addi x13, x13, 1904   ⇒ 预算 6000（跨批次）
    w(iType(0x13, 13, 13, -1));       // 27: addi x13, x13, -1     （loop1）
    w(bType(0x63, 13, 0, 1, (LOOP1 - (LOOP1 + 1)) * 4)); // 28: bne x13, x0, loop1
    w(iType(0x13, 4, 0, 1));          // 29: addi x4, x0, 1
    w(sType(0x23, 2, 5, 4, 0x0c));    // 30: sw   x4, 0x0c(x5)     ⇒ 选择 1（gpio_in ⇒ 源 11）
    w(iType(0x13, 4, 0, 1));          // 31: addi x4, x0, 1        ⇒ priority 值
    w(sType(0x23, 2, 2, 4, 0x2c));    // 32: sw   x4, 0x2c(x2)     ⇒ PLIC 源 11 priority ← 1
    w(uType(0x37, 4, 0x1));           // 33: lui  x4, 0x1          ⇒ x4 = 0x1000
    w(iType(0x13, 4, 4, -2048));      // 34: addi x4, x4, -0x800   ⇒ x4 = 0x800（enable 位 11）
    w(sType(0x23, 2, 15, 4, 0x0));    // 35: sw   x4, 0(x15)       ⇒ PLIC enable[0] 位 11（x15 复用）
    w(sType(0x23, 2, 1, 6, 0x0c));    // 36: sw   x6, 0x0c(x1)     ⇒ TOGGLE（沿 2：8→0）
    w(sType(0x23, 2, 1, 6, 0x0c));    // 37: sw   x6, 0x0c(x1)     ⇒ TOGGLE（沿 3：0→8）
    w(iType(0x13, 12, 0, 1));         // 38: addi x12, x0, 1       ⇒ 阶段 1（期望源 11）
    w(uType(0x37, 13, 0x1));          // 39: lui  x13, 0x1
    w(iType(0x13, 13, 13, 1904));     // 40: addi x13, x13, 1904   ⇒ 预算 6000
    w(iType(0x13, 13, 13, -1));       // 41: addi x13, x13, -1     （loop2）
    w(bType(0x63, 13, 0, 1, (LOOP2 - (LOOP2 + 1)) * 4)); // 42: bne x13, x0, loop2
    w(jType(0x6f, 0, 0));             // 43: jal  x0, 0            ⇒ 主循环死循环（等中断）
    // ---- handler（44 起） ----
    w(0x7c015073);                    // 44: csrrwi x0, irq_clear, 2 ⇒ 清外部中断线（M-16 单级语义）
    w(iType(0x03, 10, 14, 4, 0b010)); // 45: lw   x10, 4(x14)     ⇒ claim
    w(bType(0x63, 10, 0, 0, (HANDLER - (HANDLER + 1)) * 4)); // 46: beq x10, x0, 45（自旋）
    w(bType(0x63, 12, 0, 1, (STAGE1 - 47) * 4));              // 47: bne x12, x0, STAGE1
    w(iType(0x13, 11, 0, 3));         // 48: addi x11, x0, 3      ⇒ 阶段 0 期望源 3
    w(bType(0x63, 10, 11, 1, (FAIL - 49) * 4));               // 49: bne x10, x11, FAIL
    w(jType(0x6f, 0, (PRINT_A3 - 50) * 4));                   // 50: jal x0, PRINT_A3
    // STAGE1 = 51
    w(iType(0x13, 11, 0, 11));        // 51: addi x11, x0, 11     ⇒ 阶段 1 期望源 11
    w(bType(0x63, 10, 11, 1, (FAIL - 52) * 4));               // 52: bne x10, x11, FAIL
    w(jType(0x6f, 0, (PRINT_B11 - 53) * 4));                  // 53: jal x0, PRINT_B11
    // PRINT_B11 = 54
    w(iType(0x13, 11, 0, 0x42));      // 54: addi x11, x0, 'B'
    w(sType(0x23, 0, 3, 11, 0x0));    // 55: sb   x11, 0(x3)
    w(iType(0x13, 11, 0, 0x31));      // 56: addi x11, x0, '1'
    w(sType(0x23, 0, 3, 11, 0x0));    // 57: sb   x11, 0(x3)
    w(iType(0x13, 11, 0, 0x31));      // 58: addi x11, x0, '1'
    w(sType(0x23, 0, 3, 11, 0x0));    // 59: sb   x11, 0(x3)
    w(sType(0x23, 2, 14, 10, 0x4));   // 60: sw   x10, 4(x14)    ⇒ complete
    w(0x30200073);                    // 61: mret
    // PRINT_A3 = 62
    w(iType(0x13, 11, 0, 0x41));      // 62: addi x11, x0, 'A'
    w(sType(0x23, 0, 3, 11, 0x0));    // 63: sb   x11, 0(x3)
    w(iType(0x13, 11, 0, 0x33));      // 64: addi x11, x0, '3'
    w(sType(0x23, 0, 3, 11, 0x0));    // 65: sb   x11, 0(x3)
    w(sType(0x23, 2, 14, 10, 0x4));   // 66: sw   x10, 4(x14)    ⇒ complete
    w(0x30200073);                    // 67: mret
    // FAIL = 68
    w(iType(0x13, 11, 0, 0x58));      // 68: addi x11, x0, 'X'
    w(sType(0x23, 0, 3, 11, 0x0));    // 69: sb   x11, 0(x3)
    w(jType(0x6f, 0, 0));             // 70: jal  x0, 0          ⇒ 死循环
    return WORDS;
}

export function buildPinmuxIrqElf() {
    return buildElfFromWords(buildPinmuxIrqWords(), 0x8000_0000);
}

/**
 * 把机器码装成一个**最小 RV32 ELF32**（一个 `PT_LOAD`）。
 *
 * ⚠️ **为什么抽出来**（v0.74 · STAGE-50）：第二份固件（引脚级中断）要用
 *   **同一套** ELF 形状 ⇒ ⇒ ⇒ **不写第二份**（⇒ ⇒ 形状漂了就完了）⇒ ⇒ ⇒
 *   **一个构造器** ⇒ ⇒ 两份固件**逐字同形**。
 *
 * ⚠️ **既有那份夹具的字节**（`buildUiImageElf`）**一个都不许变** ⇒ ⇒
 *   ⇒ STAGE-32 那一批的门禁结论**仍然成立**。
 *
 * @param {number[]} words
 * @param {number} paddr — 落址
 * @param {{e_flags?: number}} [options] — v0.75 · STAGE-51：`e_flags`（偏移 36）
 *   —— 内核 ISA 门禁（`E_PLT-20`）的判据 `EF_RISCV_RVC = 0x1` ⇒ ⇒ RVC 固件
 *   的夹具从这里造（默认 0 ⇒ 既有夹具字节**一个都不变**）。
 * @returns {Buffer}
 */
export function buildElfFromWords(words, paddr, options = {}) {
    const EHDR = 52;
    const PHDR = 32;
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
    out.writeUInt32LE(paddr, 24); // e_entry
    out.writeUInt32LE(EHDR, 28); // e_phoff
    out.writeUInt32LE(options.e_flags ?? 0, 36); // e_flags（RVC 判据 · v0.75）
    out.writeUInt16LE(EHDR, 40); // e_ehsize
    out.writeUInt16LE(PHDR, 42); // e_phentsize
    out.writeUInt16LE(1, 44); // e_phnum
    const p = EHDR;
    out.writeUInt32LE(1, p); // p_type = PT_LOAD
    out.writeUInt32LE(EHDR + PHDR, p + 4); // p_offset
    out.writeUInt32LE(paddr, p + 8); // p_vaddr
    out.writeUInt32LE(paddr, p + 12); // p_paddr
    out.writeUInt32LE(text.length, p + 16); // p_filesz
    out.writeUInt32LE(text.length, p + 20); // p_memsz
    out.writeUInt32LE(5, p + 24); // p_flags = PF_R | PF_X
    out.writeUInt32LE(4, p + 28); // p_align
    text.copy(out, EHDR + PHDR);
    return out;
}
