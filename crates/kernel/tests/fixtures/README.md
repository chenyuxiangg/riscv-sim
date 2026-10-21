# 端到端夹具 · 汇编源

`intc_fw.S` 是 `crates/kernel/tests/intc_end_to_end.rs` 里那份**固件字节**
的**汇编源**。

## 为什么它在仓库里

字节数组是**汇编器产出**的（`riscv-none-elf-as -march=rv32i_zicsr`），
不是手打的 —— 手打编码的用例一旦写错，会**假红**（失败点在夹具，不在被测
行为）。

⚠️ **仓库里没有 RISC-V 汇编器**：交叉工具链在**仓库外**
（`tools/fetch-golden-deps.sh` 装到 `$DEST/toolchain/bin/`）⇒ CI 里
**重建不了**这份字节 ⇒ **字节与汇编源之间没有自动一致性检查**。

⇒ 本文件是那份字节的**唯一可复核来源**：改固件时，**先改这里**，再重新
汇编，把字节同步回 `intc_end_to_end.rs` 的 `INTC_FIRMWARE`。

## 重新生成字节

```bash
TC=~/riscv-sim-golden/toolchain/bin
$TC/riscv-none-elf-as -march=rv32i_zicsr -mno-relax -o intc_fw.o intc_fw.S
$TC/riscv-none-elf-objdump -d intc_fw.o     # 逐条核对
$TC/riscv-none-elf-objcopy -O binary intc_fw.o intc_fw.bin
```

⚠️ **`-march=rv32i_zicsr`（不是 `rv32imc`）**：`-march=rv32imc` 会把
`jal` 压成 2 字节 ⇒ 固件布局**错位**。

⚠️ **地址一律用显式 `lui` + `addi`**：`la t0, handler` 这类伪指令在
`-mno-relax` 下产出 `auipc` + **偏移为 0**（错）。

## 固件做什么

```text
0x00 lui   t0, 0x0            ; t0 = 0
0x04 addi  t0, t0, 0x3c       ; t0 = 0x3c（handler 的地址）
0x08 sw    x0, 0x200(x0)      ; 清「交付计数」
0x0c csrrw x0, mtvec, t0      ; 向量表 = handler
0x10 lui   t1, 0x1
0x14 srli  t1, t1, 1          ; t1 = 0x800（mie 的 MEIE 位，bit 11）
0x18 ori   t1, t1, 0x80       ; t1 = 0x880（再加上 MTIE 位，bit 7）
0x1c csrrw x0, mie, t1
0x20 addi  t2, x0, 8          ; t2 = 8（mstatus 的 MIE 位，bit 3）
0x24 csrrs x0, mstatus, t2
0x28 jal   x0, 0x28           ; 自旋
0x2c..0x38 nop × 4
0x3c handler:
0x3c lw    t4, 0x200(x0)      ; 交付计数 += 1
0x40 addi  t4, t4, 1
0x44 sw    t4, 0x200(x0)
0x48 addi  t3, x0, 2          ; 线 1 的位掩码（bit L = 线 L）
0x4c csrrw x0, 0x7c0, t3      ; irq_clear：清掉线 1
0x50 mret                     ; 回到 mepc
```

⚠️ **两个位号空间**：平台的**中断线号**是 `1 / 2 / 3`，RISC-V 的 **`mie`
位号**是 `11`（MEIE）/ `7`（MTIE）——两套编号，由契约层的
`mie_bit_for_line` 唯一互译。把 `mie` 写成 `1` 会**一行都不使能**。

⚠️ **`irq_clear` 的掩码按线号索引**（bit L = 线 L）：清**线 1** 要写
`0b10` = 2。写 `1` 清的是**线 0**（那条线根本没被拉起）。
