    .section .text
    .globl _start
_start:
    lui   t0, 0x0
    addi  t0, t0, 0x3c
    sw    x0, 0x200(x0)
    csrrw x0, mtvec, t0
    lui   t1, 0x1
    srli  t1, t1, 1
    ori   t1, t1, 0x80
    csrrw x0, mie, t1
    addi  t2, x0, 8
    csrrs x0, mstatus, t2
    jal   x0, 0
    .align 2
    nop
    nop
    nop
    nop
handler:
    lw    t4, 0x200(x0)
    addi  t4, t4, 1
    sw    t4, 0x200(x0)
    addi  t3, x0, 2
    csrrw x0, 0x7c0, t3
    mret
