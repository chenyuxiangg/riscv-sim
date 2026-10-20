// Demo 固件（第 1 阶段 + D-46）——RV32I 手写编码的 40 字节小程序。
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
//   0x10  0x0040006F   jal  x0, 0x14      ← 跳到 GPIO 设置段
//   0x14  0x400002B7   lui  x5, 0x40000   ← x5 = 0x4000_0000（GPIO 基址）
//   0x18  0x00100313   addi x6, x0, 1     ← x6 = 1
//   0x1c  0x0062A223   sw   x6, 4(x5)     ← GPIO0.OUT = 1 ← **探针边沿**
//   0x20  0x00100213   addi x4, x0, 1     ← 断点靶点
//   0x24  0x0000006F   jal  x0, 0         ← 自环
//
// ## 为什么固件要驱动 GPIO（D-46）
//
// 第 1 阶段结束时波形视图是**空的**：真实的探针管道（M-18 判沿 /
// M-19 环形缓冲）根本没接到 M-14 的总线 hook 上，台账 **D-46**。
//
// 管道接上之后，还差最后一步：**得有东西真的去动那个引脚**。否则
// 通道列表是空的（没有外设）、边沿也是空的（没有变化）——波形视图
// 仍然是空的，只是「空的原因」换了一个。
//
// 所以 demo 固件在 0x14..0x1c 这三格（原本是三�� `nop` 填充）里写
// GPIO0 的 `OUT` 寄存器（偏移 0x04）——M-15 的 GPIO 在**电平真变化**
// 时上报 `OUT` 信号，M-18 判沿，M-19 收边沿，宿主推成契约的
// `WaveformDelta`，前端把通道列出来。
//
// ## 编码订正（2026-10-21，第 1 阶段 brief 裁定）
//
// brief 最初给的是 `0x00302023`，那是 `sw x3, 0(x0)` —— S 型立即数的
// **imm[11:5]** 段漏了。`sw x3, 0x100(x0)` 的正确编码是 `0x10302023`
// （imm = 0x100 → imm[11:5] = 8 落在 bit[31:25]，imm[4:0] = 0 落在
// bit[11:7]），即第 4 个字的高字节是 `0x10` 而不是 `0x00`。前端
// 「读 0x100」的动作依赖这一条，所以按正确编码落地。门禁里有一条
// **解码级**断言（把 S 型立即数解回来比对 0x100），不是只比对字面值
// ——同类错误不会再靠人眼发现。
//
// 0x10 处的 `0x0040006F` 是 `jal x0, +0x4`（J 型立即数以半字为单位），
// 从 0x10 跳到 0x14 ——与表里写的目标地址 0x14 一致。

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
    // 0x10  jal  x0, +0x4        → 0x0040006F  (跳到 0x14)
    0x6f, 0x00, 0x40, 0x00,
    // 0x14  lui  x5, 0x40000     → 0x400002B7  (x5 = 0x4000_0000)
    0xb7, 0x02, 0x00, 0x40,
    // 0x18  addi x6, x0, 1       → 0x00100313
    0x13, 0x03, 0x10, 0x00,
    // 0x1c  sw   x6, 4(x5)       → 0x0062A223  (GPIO0.OUT = 1 → 探针边沿)
    0x23, 0xa2, 0x62, 0x00,
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

/**
 * Base address of the GPIO peripheral the demo firmware drives.
 *
 * Matches `kernel_host::DEFAULT_GPIO_BASE` (PL061 convention).
 *
 * @type {number}
 */
export const DEMO_FIRMWARE_GPIO_BASE = 0x4000_0000;

/**
 * Byte offset of the GPIO `OUT` register inside the peripheral window.
 *
 * @type {number}
 */
export const DEMO_FIRMWARE_GPIO_OUT_OFFSET = 0x04;
