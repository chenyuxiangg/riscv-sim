// Demo 固件（第 1 阶段）——RV32I 手写编码的 40 字节小程序。
//
// 这是喂给**真内核**的镜像（`front/dev/mock-project.js` 把它挂成
// `RawHex` 镜像，内核按 spec §3.4 把它落在基址 0 = 复位向量）。
//
// 逐条反汇编（小端序：每个 32 位字的**低字节在前**）：
//
//   addr  word          instruction
//   0x00  0x02A00093   addi x1, x0, 42
//   0x04  0x00700113   addi x2, x0, 7
//   0x08  0x002081B3   add  x3, x1, x2
//   0x0c  0x10302023   sw   x3, 0x100(x0)
//   0x10  0x0100006F   jal  x0, 0x20      ← 跳到断点靶点
//   0x14  0x00000013   nop
//   0x18  0x00000013   nop
//   0x1c  0x00000013   nop
//   0x20  0x00100213   addi x4, x0, 1    ← 断点靶点
//   0x24  0x0000006F   jal  x0, 0        ← 自环
//
// ⚠️ **编码订正（2026-10-21，第 1 阶段 brief 裁定）**：brief 最初给的是
// `0x00302023`，那是 `sw x3, 0(x0)` —— S 型立即数的 **imm[11:5]** 段漏了。
// `sw x3, 0x100(x0)` 的正确编码是 `0x10302023`（imm = 0x100 → imm[11:5] = 8
// 落在 bit[31:25]，imm[4:0] = 0 落在 bit[11:7]），即第 4 个字的高字节是
// `0x10` 而不是 `0x00`。前端「读 0x100」的动作依赖这一条，所以按正确
// 编码落地。门禁里有一条**解码级**断言（把 S 型立即数解回来比对
// 0x100），不是只比对字面值——同类错误不会再靠人眼发现。
//
// 0x10 处的 `0x0100006F` 是 `jal x0, +0x10`（J 型立即数以半字为单位），
// 从 0x10 跳到 0x20 —— 与表里写的目标地址 0x20 一致。

/**
 * The demo firmware bytes (RV32I, hand-assembled).
 *
 * Little-endian: the low byte of every 32-bit word comes first.
 *
 * @type {ReadonlyArray<number>}
 */
export const DEMO_FIRMWARE_BYTES = Object.freeze([
    // 0x00  addi x1, x0, 42      → 0x02A00093
    0x93, 0x00, 0xa0, 0x02,
    // 0x04  addi x2, x0, 7       → 0x00700113
    0x13, 0x01, 0x70, 0x00,
    // 0x08  add  x3, x1, x2      → 0x002081B3
    0xb3, 0x81, 0x20, 0x00,
    // 0x0c  sw   x3, 0x100(x0)  → 0x10302023
    0x23, 0x20, 0x30, 0x10,
    // 0x10  jal  x0, +0x10       → 0x0100006F  (跳到 0x20)
    0x6f, 0x00, 0x00, 0x01,
    // 0x14  nop                  → 0x00000013
    0x13, 0x00, 0x00, 0x00,
    // 0x18  nop                  → 0x00000013
    0x13, 0x00, 0x00, 0x00,
    // 0x1c  nop                  → 0x00000013
    0x13, 0x00, 0x00, 0x00,
    // 0x20  addi x4, x0, 1       → 0x00100213  (断点靶点)
    0x13, 0x02, 0x10, 0x00,
    // 0x24  jal  x0, 0           → 0x0000006F  (自环)
    0x6f, 0x00, 0x00, 0x00,
]);

/**
 * Address of the breakpoint target — the `addi x4, x0, 1` at 0x20.
 *
 * @type {number}
 */
export const DEMO_FIRMWARE_BREAKPOINT_ADDR = 0x20;
