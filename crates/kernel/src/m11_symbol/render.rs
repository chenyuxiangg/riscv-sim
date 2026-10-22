//! ISA 助记符渲染器 —— 把 M-13 的 [`Decoded`] 变成**一行文本**。
//!
//! 落地规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`（冻结）
//! §2 裁决 A / C。
//!
//! ## 唯一的解码真相是 M-13（裁决 A）
//!
//! 本模块**不含任何「opcode → 助记符」的解码表**。指令身份由 M-13 的
//! 二级解码 [`crate::m13_iss::classify`] 给出（它读的就是 M-13 自己
//! 解出来的 `opcode` / `funct3` / `funct7`），本模块只提供一张
//! **「`Op` → 文本」的渲染表**——即「怎么写」而不是「是什么」。
//!
//! ⇒ 新增一条指令时：解码侧只动 M-13，渲染侧只动本文件的文本映射，
//! **两边不会各自长出一套语义**（这正是裁决 A 要消灭的漂移风险）。
//!
//! ## 本模块定下的口径（规格没写死、由本模块拍板的部分）
//!
//! 1. **寄存器名用 ABI 名**（`zero` `ra` `sp` `gp` `tp` `t0`…`t6`
//!    `s0`…`s11` `a0`…`a7`）。
//!    *理由*：黄金参照 `riscv-none-elf-objdump -d -M no-aliases` 默认就
//!    打 ABI 名 ⇒ 渲染结果与黄金参照**逐条可比**。
//!    *一致性*：`x0` 在 ISA 层面是硬连线零，本模块**一律**叫 `zero`
//!    （不会出现「一半 `x0` 一半 `zero`」）。
//! 2. **立即数按指令取来源**（见下面「立即数来源表」）。
//! 3. **分支 / 跳转目标打印成绝对地址**（`pc + 立即数`），**裸十六进制**
//!    ——与 objdump 的 `beq a0,a1,100ca` 形态一致。
//! 4. **压缩指令带 `c.` 前缀**，并按**压缩形态**打印操作数
//!    （`c.addi sp,-16` 而不是 `c.addi sp,sp,-16`）——裁决 C。
//! 5. **不做伪指令别名**（裁决 C）：`mv` / `li` / `ret` / `j` 一律还原成
//!    ISA 助记符（`c.mv` / `c.li` 是**压缩指令本身的名字**，不是别名）。
//! 6. **非法指令**打印成 `.illegal 0x....`（16 位补 4 位、32 位补 8 位）。
//! 7. 助记符与操作数之间**一个空格**，操作数之间用 `,`（无空格）。
//!
//! ## 立即数来源表（**本模块最容易错的地方**）
//!
//! [`Decoded::imm`] 不是「一个万能立即数」：M-13 的解码器按指令格式
//! 填它，而**有几个格式的立即数根本不在这个字段里**。
//!
//! | 指令族 | 立即数来源 | 语义 |
//! | --- | --- | --- |
//! | I 型（`addi` `slti` `sltiu` `xori` `ori` `andi`） | `imm` | 12 位**符号扩展** |
//! | S 型（`sb` `sh` `sw`） | `imm` | 12 位**符号扩展** |
//! | B 型（`beq` …） | `imm` | 13 位**符号扩展**，**字节偏移**（相对 `pc`） |
//! | U 型（`lui` `auipc`） | `(imm >> 12) & 0xFFFFF` | 20 位**无符号**字段 |
//! | J 型（`jal`） | `imm` | 21 位**符号扩展**，**字节偏移**（相对 `pc`） |
//! | 移位立即（`slli` `srli` `srai`） | `imm` | 5 位**移位量**（不是 12 位立即数） |
//! | CSR（`csrrw` `csrrs` `csrrc`） | CSR 号 = `(imm as u32) & 0xFFF` | 12 位**无符号** |
//! | CSR 立即（`csrrwi` `csrrsi` `csrrci`） | CSR 号同上；uimm = **`rs1`** | uimm 是 5 位**无符号**立即数，**编码在 rs1 字段位** |
//!
//! ⚠️ `csrrwi` 这类指令的 5 位立即数在**指令字的 `rs1` 字段位**上，
//! M-13 的解码器把它放进了 `Decoded::rs1`（`imm` 里放的是 CSR 号）。
//! 把它当成 `imm` 渲染 ⇒ **数值错**（而且是那种「看起来很合理」的错）。
//!
//! ## 压缩指令的形态
//!
//! 压缩指令在 M-13 里**已经展开成等价的 32 位字段**了（`compressed_raw`
//! 里另存着 16 位原码）。本模块**只按展开后的字段渲染**，再换成压缩
//! 指令自己的操作数形态——**不重新读编码**（那才是第二套解码）。
//!
//! ⚠️ **一个已知的取舍**：C.ADDI（`rd` = `sp`）与 C.ADDI16SP 展开后的
//! 字段**完全相同**（都是 `addi sp,sp,立即数`），仅凭字段分不开。本模块
//! 按「立即数是否落在 C.ADDI 的 6 位范围内」来判：C.ADDI 的立即数恒在
//! `[-32, 31]`，超出 ⇒ 只能是 C.ADDI16SP ⇒ 打印 `c.addi16sp`。
//! 落在 `[-32, 31]` 内的**一律**打印 `c.addi`。
//! ⇒ 与 objdump 的差别只出现在「一个真的是 C.ADDI16SP、但立即数恰好落在
//! `[-32, 31]`」的编码上（那正是 objdump 打 `c.addi16sp` 的场合）。
//! 这是「零第二套解码」的**有意取舍**：宁可少一个字，也不重开一套编码解释。

use riscv_sim_contract::model::DisasmEntryDto;

use crate::m13_iss::{classify, Decoded, Op};

/// ABI 寄存器名（`x0`..`x31`）。
///
/// 索引即寄存器号；`x0` 叫 `zero`（ISA 层面硬连线为零的那个）。
const ABI_NAMES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

/// ABI 名 → 寄存器号（`x0` → `"zero"`）。
///
/// 越界 / 非法名字 ⇒ `None`。
pub fn abi_to_index(name: &str) -> Option<u8> {
    ABI_NAMES.iter().position(|n| *n == name).map(|i| i as u8)
}

/// 寄存器号 → ABI 名。
///
/// 寄存器号只有 5 位（指令字里就那么多），所以这里**总是**返回一个
/// 名字：越界的号被截到 5 位（等价于「读寄存器文件时的索引规则」）。
pub fn reg_name(index: u8) -> &'static str {
    ABI_NAMES[(index & 0x1F) as usize]
}

/// 把一条已解码的指令渲染成**一行** ISA 助记符文本。
///
/// * 不含地址、不含机器码（要那两样用 [`render_entry`]）。
/// * 非法指令（保留编码，或 M-13 二级解码查不到）⇒ `.illegal 0x....`。
pub fn render_mnemonic(d: &Decoded) -> String {
    if d.illegal {
        return illegal_text(d);
    }
    // M-13 的二级解码：查不到 ⇒ 这条指令在 RV32IMC 里不存在。
    let Some(op) = classify(d) else {
        return illegal_text(d);
    };
    if d.compressed {
        render_compressed(d, op)
    } else {
        render_wide(d, op)
    }
}

/// 把一条已解码的指令渲染成契约层的反汇编条目。
///
/// * `addr` = 取指地址（`Decoded::pc`）。
/// * `code` = **原码**（`Decoded::raw`）——压缩指令是 **16 位原码**
///   （M-13 的 `raw` 对压缩指令本来就存 16 位原码，验收 #2）。
/// * `mnemonic` = [`render_mnemonic`] 的结果。
/// * `width` = 指令宽度（`Decoded::len`：2 或 4）。
pub fn render_entry(d: &Decoded) -> DisasmEntryDto {
    DisasmEntryDto {
        addr: d.pc,
        code: d.raw,
        mnemonic: render_mnemonic(d),
        width: d.len,
    }
}

/// 非法指令的文本。
///
/// 16 位编码补 4 位、32 位编码补 8 位——**宽度是判别信息**（一条非法
/// 的压缩指令和一条非法的 32 位指令不是一回事）。
fn illegal_text(d: &Decoded) -> String {
    if d.len == 2 {
        format!(".illegal 0x{:04x}", d.raw & 0xFFFF)
    } else {
        format!(".illegal 0x{:08x}", d.raw)
    }
}

/// 32 位指令的渲染。
fn render_wide(d: &Decoded, op: Op) -> String {
    match op {
        // U 型：打印 20 位**无符号**字段（`imm` 存的是 <<12 之后的值）。
        Op::Lui | Op::Auipc => {
            format!("{} {},0x{:x}", wide_name(op), reg_name(d.rd), u20(d.imm))
        }
        // I 型：12 位符号扩展立即数。
        Op::Addi | Op::Slti | Op::Sltiu | Op::Xori | Op::Ori | Op::Andi => {
            format!(
                "{} {},{},{}",
                wide_name(op),
                reg_name(d.rd),
                reg_name(d.rs1),
                d.imm
            )
        }
        // 移位立即：5 位移位量（`imm` 里就是移位量，不是 12 位立即数）。
        Op::Slli | Op::Srli | Op::Srai => format!(
            "{} {},{},0x{:x}",
            wide_name(op),
            reg_name(d.rd),
            reg_name(d.rs1),
            d.imm as u32
        ),
        // R 型：三个寄存器。
        Op::Add
        | Op::Sub
        | Op::Sll
        | Op::Slt
        | Op::Sltu
        | Op::Xor
        | Op::Srl
        | Op::Sra
        | Op::Or
        | Op::And
        | Op::Mul
        | Op::Mulh
        | Op::Mulhsu
        | Op::Mulhu
        | Op::Div
        | Op::Divu
        | Op::Rem
        | Op::Remu => format!(
            "{} {},{},{}",
            wide_name(op),
            reg_name(d.rd),
            reg_name(d.rs1),
            reg_name(d.rs2)
        ),
        // 装载：`op rd, 偏移(基址)`。
        Op::Lb | Op::Lh | Op::Lw | Op::Lbu | Op::Lhu => format!(
            "{} {},{}({})",
            wide_name(op),
            reg_name(d.rd),
            d.imm,
            reg_name(d.rs1)
        ),
        // 存储：`op 源寄存器, 偏移(基址)`。
        Op::Sb | Op::Sh | Op::Sw => format!(
            "{} {},{}({})",
            wide_name(op),
            reg_name(d.rs2),
            d.imm,
            reg_name(d.rs1)
        ),
        // 分支：目标 = pc + 立即数（绝对地址）。
        Op::Beq | Op::Bne | Op::Blt | Op::Bge | Op::Bltu | Op::Bgeu => format!(
            "{} {},{},{}",
            wide_name(op),
            reg_name(d.rs1),
            reg_name(d.rs2),
            target(d)
        ),
        // 跳转。
        Op::Jal => format!("jal {},{}", reg_name(d.rd), target(d)),
        Op::Jalr => format!("jalr {},{}({})", reg_name(d.rd), d.imm, reg_name(d.rs1)),
        // 屏障：M-13 里是空操作 ⇒ 不打印 pred/succ（objdump 会打印
        // `fence iorw,iorw`；那是编码细节，不是语义）。
        Op::Fence => "fence".to_string(),
        Op::FenceI => "fence.i".to_string(),
        // 系统指令。
        Op::Ecall => "ecall".to_string(),
        Op::Ebreak => "ebreak".to_string(),
        Op::Mret => "mret".to_string(),
        // CSR：号 + 寄存器。
        Op::Csrrw | Op::Csrrs | Op::Csrrc => format!(
            "{} {},{},{}",
            wide_name(op),
            reg_name(d.rd),
            csr_name(csr_number(d)),
            reg_name(d.rs1)
        ),
        // CSR 立即：uimm 在 **rs1 字段位**（`Decoded::rs1`），不是 `imm`。
        Op::Csrrwi | Op::Csrrsi | Op::Csrrci => format!(
            "{} {},{},{}",
            wide_name(op),
            reg_name(d.rd),
            csr_name(csr_number(d)),
            d.rs1 as u32
        ),
    }
}

/// 压缩指令的渲染（**按展开后的字段**，裁决 C）。
///
/// 压缩形态与 32 位形态的差别只在**操作数的形状**上：压缩指令省掉了
/// 隐含的寄存器（`c.addi rd,imm` 的 `rs1` 就是 `rd`），所以这里不能
/// 直接复用 32 位的操作数列表。
fn render_compressed(d: &Decoded, op: Op) -> String {
    match op {
        // C.ADDI / C.NOP / C.LI / C.ADDI4SPN / C.ADDI16SP 都展开成
        // `addi`，靠**形状**区分。
        Op::Addi => {
            if d.rd == 0 && d.rs1 == 0 {
                // C.NOP —— `addi x0,x0,0`。
                "c.nop".to_string()
            } else if d.rs1 == 0 {
                // C.LI —— `addi rd,x0,imm`。
                format!("c.li {},{}", reg_name(d.rd), d.imm)
            } else if d.rd == d.rs1 {
                // C.ADDI —— `addi rd,rd,imm`。
                if d.rd == 2 && !(-32..=31).contains(&d.imm) {
                    // C.ADDI16SP —— 立即数超出 C.ADDI 的 6 位范围。
                    format!("c.addi16sp sp,{}", d.imm)
                } else {
                    format!("c.addi {},{}", reg_name(d.rd), d.imm)
                }
            } else {
                // C.ADDI4SPN —— `addi rd',sp,nzuimm`。
                format!("c.addi4spn {},sp,{}", reg_name(d.rd), d.imm)
            }
        }
        // C.MV / C.ADD 都展开成 `add`。
        Op::Add => {
            if d.rs1 == 0 {
                // C.MV —— `add rd,x0,rs2`。
                format!("c.mv {},{}", reg_name(d.rd), reg_name(d.rs2))
            } else {
                // C.ADD —— `add rd,rd,rs2`。
                format!("c.add {},{}", reg_name(d.rd), reg_name(d.rs2))
            }
        }
        // CA 组：`c.sub` / `c.xor` / `c.or` / `c.and`。
        Op::Sub => format!("c.sub {},{}", reg_name(d.rd), reg_name(d.rs2)),
        Op::Xor => format!("c.xor {},{}", reg_name(d.rd), reg_name(d.rs2)),
        Op::Or => format!("c.or {},{}", reg_name(d.rd), reg_name(d.rs2)),
        Op::And => format!("c.and {},{}", reg_name(d.rd), reg_name(d.rs2)),
        // CI 组的移位。
        Op::Slli => format!("c.slli {},0x{:x}", reg_name(d.rd), d.imm as u32),
        Op::Srli => format!("c.srli {},0x{:x}", reg_name(d.rd), d.imm as u32),
        Op::Srai => format!("c.srai {},0x{:x}", reg_name(d.rd), d.imm as u32),
        // C.ANDI。
        Op::Andi => format!("c.andi {},{}", reg_name(d.rd), d.imm),
        // C.LW / C.LWSP。
        Op::Lw => {
            if d.rs1 == 2 {
                format!("c.lwsp {},{}(sp)", reg_name(d.rd), d.imm)
            } else {
                format!("c.lw {},{}({})", reg_name(d.rd), d.imm, reg_name(d.rs1))
            }
        }
        // C.SW / C.SWSP。
        Op::Sw => {
            if d.rs1 == 2 {
                format!("c.swsp {},{}(sp)", reg_name(d.rs2), d.imm)
            } else {
                format!("c.sw {},{}({})", reg_name(d.rs2), d.imm, reg_name(d.rs1))
            }
        }
        // C.BEQZ / C.BNEZ。
        Op::Beq => format!("c.beqz {},{}", reg_name(d.rs1), target(d)),
        Op::Bne => format!("c.bnez {},{}", reg_name(d.rs1), target(d)),
        // C.J / C.JAL。
        Op::Jal => {
            if d.rd == 0 {
                format!("c.j {}", target(d))
            } else {
                format!("c.jal {}", target(d))
            }
        }
        // C.JR / C.JALR。
        Op::Jalr => {
            if d.rd == 0 {
                format!("c.jr {}", reg_name(d.rs1))
            } else {
                format!("c.jalr {}", reg_name(d.rs1))
            }
        }
        // C.LUI。
        Op::Lui => format!("c.lui {},0x{:x}", reg_name(d.rd), u20(d.imm)),
        // C.EBREAK。
        Op::Ebreak => "c.ebreak".to_string(),
        // 压缩编码不会展开成这些 op（兜底：万一将来 M-13 加了新指令）。
        _ => render_wide(d, op),
    }
}

/// 32 位助记符的**渲染表**（不是解码表：身份由 M-13 给出）。
fn wide_name(op: Op) -> &'static str {
    match op {
        Op::Lui => "lui",
        Op::Auipc => "auipc",
        Op::Addi => "addi",
        Op::Slti => "slti",
        Op::Sltiu => "sltiu",
        Op::Xori => "xori",
        Op::Ori => "ori",
        Op::Andi => "andi",
        Op::Slli => "slli",
        Op::Srli => "srli",
        Op::Srai => "srai",
        Op::Add => "add",
        Op::Sub => "sub",
        Op::Sll => "sll",
        Op::Slt => "slt",
        Op::Sltu => "sltu",
        Op::Xor => "xor",
        Op::Srl => "srl",
        Op::Sra => "sra",
        Op::Or => "or",
        Op::And => "and",
        Op::Mul => "mul",
        Op::Mulh => "mulh",
        Op::Mulhsu => "mulhsu",
        Op::Mulhu => "mulhu",
        Op::Div => "div",
        Op::Divu => "divu",
        Op::Rem => "rem",
        Op::Remu => "remu",
        Op::Lb => "lb",
        Op::Lh => "lh",
        Op::Lw => "lw",
        Op::Lbu => "lbu",
        Op::Lhu => "lhu",
        Op::Sb => "sb",
        Op::Sh => "sh",
        Op::Sw => "sw",
        Op::Beq => "beq",
        Op::Bne => "bne",
        Op::Blt => "blt",
        Op::Bge => "bge",
        Op::Bltu => "bltu",
        Op::Bgeu => "bgeu",
        Op::Jal => "jal",
        Op::Jalr => "jalr",
        Op::Fence => "fence",
        Op::FenceI => "fence.i",
        Op::Ecall => "ecall",
        Op::Ebreak => "ebreak",
        Op::Mret => "mret",
        Op::Csrrw => "csrrw",
        Op::Csrrs => "csrrs",
        Op::Csrrc => "csrrc",
        Op::Csrrwi => "csrrwi",
        Op::Csrrsi => "csrrsi",
        Op::Csrrci => "csrrci",
    }
}

/// U 型立即数的 **20 位无符号字段**。
///
/// `Decoded::imm` 存的是「`imm20 << 12`」之后的值（含符号位）⇒ 要
/// 打印 objdump 那种 `lui a0,0x40000`，得先把 12 位移回来，再掩到
/// 20 位（掩掉的是**移位带来的高位**，不是信息）。
fn u20(imm: i32) -> u32 {
    ((imm >> 12) & 0xF_FFFF) as u32
}

/// 分支 / 跳转的**绝对目标地址**（`pc + 立即数`）。
///
/// 打印成**裸十六进制**（与 objdump 一致：`beq a0,a1,100ca`）。
fn target(d: &Decoded) -> String {
    format!("{:x}", d.pc.wrapping_add(d.imm as u32))
}

/// CSR 号（12 位**无符号**）。
///
/// M-13 把 `inst[31:20]` 符号扩展后放进 `imm` ⇒ 掩回 12 位就是原值
/// （高位为 1 的 CSR 号也不会变成负数）。
fn csr_number(d: &Decoded) -> u32 {
    (d.imm as u32) & 0xFFF
}

/// CSR 的**名字**（架构常量表，不是解码表）。
///
/// 名字只是「打印出来的字」——**不影响任何语义**（CSR 号仍是
/// [`csr_number`] 给出的那个数）。表外的 CSR 一律打印成 `0x<hex>`。
fn csr_name(csr: u32) -> String {
    let name = match csr {
        0x001 => "fflags",
        0x002 => "frm",
        0x003 => "fcsr",
        0x300 => "mstatus",
        0x301 => "misa",
        0x302 => "medeleg",
        0x303 => "mideleg",
        0x304 => "mie",
        0x305 => "mtvec",
        0x306 => "mcounteren",
        0x30a => "menvcfg",
        0x310 => "mstatush",
        0x320 => "mcountinhibit",
        0x340 => "mscratch",
        0x341 => "mepc",
        0x342 => "mcause",
        0x343 => "mtval",
        0x344 => "mip",
        0x747 => "mseccfg",
        0xb00 => "mcycle",
        0xb02 => "minstret",
        0xb80 => "mcycleh",
        0xb82 => "minstreth",
        0xc00 => "cycle",
        0xc01 => "time",
        0xc02 => "instret",
        0xc80 => "cycleh",
        0xc81 => "timeh",
        0xc82 => "instreth",
        0xf11 => "mvendorid",
        0xf12 => "marchid",
        0xf13 => "mimpid",
        0xf14 => "mhartid",
        _ => return format!("0x{:x}", csr),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m13_iss::Decoder;

    // ------------------------------------------------------------------
    // 测试用的**编码**器（不是解码器）：把指令的字段拼成指令字。
    // ------------------------------------------------------------------

    fn r_type(funct7: u32, rs2: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
        (funct7 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
    }

    fn i_type(imm: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
        ((imm & 0xFFF) << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
    }

    fn s_type(imm: u32, rs2: u32, rs1: u32, funct3: u32, opcode: u32) -> u32 {
        (((imm >> 5) & 0x7F) << 25)
            | (rs2 << 20)
            | (rs1 << 15)
            | (funct3 << 12)
            | ((imm & 0x1F) << 7)
            | opcode
    }

    fn b_type(imm: u32, rs2: u32, rs1: u32, funct3: u32, opcode: u32) -> u32 {
        (((imm >> 12) & 0x1) << 31)
            | (((imm >> 5) & 0x3F) << 25)
            | (rs2 << 20)
            | (rs1 << 15)
            | (funct3 << 12)
            | (((imm >> 1) & 0xF) << 8)
            | (((imm >> 11) & 0x1) << 7)
            | opcode
    }

    fn u_type(imm20: u32, rd: u32, opcode: u32) -> u32 {
        ((imm20 & 0xF_FFFF) << 12) | (rd << 7) | opcode
    }

    fn j_type(imm: u32, rd: u32, opcode: u32) -> u32 {
        (((imm >> 20) & 0x1) << 31)
            | (((imm >> 1) & 0x3FF) << 21)
            | (((imm >> 11) & 0x1) << 20)
            | (((imm >> 12) & 0xFF) << 12)
            | (rd << 7)
            | opcode
    }

    /// 解码一条 32 位指令并渲染。
    fn render32(raw: u32, pc: u32) -> String {
        let d = Decoder::default().decode(raw, pc);
        assert!(!d.compressed, "fixture is not a compressed encoding");
        render_mnemonic(&d)
    }

    /// 解码一条 16 位压缩指令并渲染。
    fn render16(raw: u16, pc: u32) -> String {
        let d = Decoder::default().decode(raw as u32, pc);
        assert!(d.compressed, "fixture is not a compressed encoding");
        render_mnemonic(&d)
    }

    // ------------------------------------------------------------------
    // 1. 逐类指令各一条（断言**具体文本**）
    // ------------------------------------------------------------------

    #[test]
    fn renders_r_type_add() {
        // add a0,a1,a2
        let raw = r_type(0, 12, 11, 0b000, 10, 0b011_0011);
        assert_eq!(render32(raw, 0x1000), "add a0,a1,a2");
    }

    #[test]
    fn renders_i_type_addi() {
        // addi a0,a1,42
        let raw = i_type(42, 11, 0b000, 10, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "addi a0,a1,42");
    }

    #[test]
    fn renders_s_type_sw() {
        // sw a1,4(a0)
        let raw = s_type(4, 11, 10, 0b010, 0b010_0011);
        assert_eq!(render32(raw, 0x1000), "sw a1,4(a0)");
    }

    #[test]
    fn renders_b_type_beq_with_absolute_target() {
        // beq a0,a1,+4 @0x1000 ⇒ 目标 0x1004（裸十六进制）
        let raw = b_type(4, 11, 10, 0b000, 0b110_0011);
        assert_eq!(render32(raw, 0x1000), "beq a0,a1,1004");
    }

    #[test]
    fn renders_u_type_lui_and_auipc() {
        // lui a0,0x40000
        let raw = u_type(0x4_0000, 10, 0b011_0111);
        assert_eq!(render32(raw, 0x1000), "lui a0,0x40000");
        // auipc a0,0x2
        let raw = u_type(0x2, 10, 0b001_0111);
        assert_eq!(render32(raw, 0x1000), "auipc a0,0x2");
    }

    #[test]
    fn renders_j_type_jal_with_absolute_target() {
        // jal ra,+0x34 @0x1000 ⇒ 目标 0x1034
        let raw = j_type(0x34, 1, 0b110_1111);
        assert_eq!(render32(raw, 0x1000), "jal ra,1034");
    }

    #[test]
    fn renders_m_extension() {
        // mul a0,a1,a2
        let raw = r_type(1, 12, 11, 0b000, 10, 0b011_0011);
        assert_eq!(render32(raw, 0x1000), "mul a0,a1,a2");
        // div a0,a1,a2
        let raw = r_type(1, 12, 11, 0b100, 10, 0b011_0011);
        assert_eq!(render32(raw, 0x1000), "div a0,a1,a2");
        // remu a0,a1,a2
        let raw = r_type(1, 12, 11, 0b111, 10, 0b011_0011);
        assert_eq!(render32(raw, 0x1000), "remu a0,a1,a2");
    }

    #[test]
    fn renders_csr_register_forms() {
        // csrrw a0,mscratch,a1
        let raw = i_type(0x340, 11, 0b001, 10, 0b111_0011);
        assert_eq!(render32(raw, 0x1000), "csrrw a0,mscratch,a1");
        // csrrs a0,mscratch,a1
        let raw = i_type(0x340, 11, 0b010, 10, 0b111_0011);
        assert_eq!(render32(raw, 0x1000), "csrrs a0,mscratch,a1");
        // csrrc a0,mscratch,a1
        let raw = i_type(0x340, 11, 0b011, 10, 0b111_0011);
        assert_eq!(render32(raw, 0x1000), "csrrc a0,mscratch,a1");
        // 表外的 CSR 打印成 0x<hex>
        let raw = i_type(0x7c0, 11, 0b010, 10, 0b111_0011);
        assert_eq!(render32(raw, 0x1000), "csrrs a0,0x7c0,a1");
    }

    #[test]
    fn renders_csr_immediate_form_takes_uimm_from_rs1_field() {
        // csrrwi a0,mscratch,3 —— uimm 在 rs1 字段位（=3），不是 imm。
        let raw = i_type(0x340, 3, 0b101, 10, 0b111_0011);
        assert_eq!(render32(raw, 0x1000), "csrrwi a0,mscratch,3");
        // csrrsi a0,0x7c0,1
        let raw = i_type(0x7c0, 1, 0b110, 10, 0b111_0011);
        assert_eq!(render32(raw, 0x1000), "csrrsi a0,0x7c0,1");
    }

    #[test]
    fn renders_compressed_instructions_with_c_prefix() {
        // c.addi sp,-16
        assert_eq!(render16(0x1141, 0x1000), "c.addi sp,-16");
        // c.li a1,0
        assert_eq!(render16(0x4581, 0x1000), "c.li a1,0");
        // c.mv s0,a0
        assert_eq!(render16(0x842a, 0x1000), "c.mv s0,a0");
        // c.nop
        assert_eq!(render16(0x0001, 0x1000), "c.nop");
        // c.ebreak
        assert_eq!(render16(0x9002, 0x1000), "c.ebreak");
        // c.jr ra
        assert_eq!(render16(0x8082, 0x1000), "c.jr ra");
        // c.jalr a5
        assert_eq!(render16(0x9782, 0x1000), "c.jalr a5");
        // c.add a2,a1
        assert_eq!(render16(0x962e, 0x1000), "c.add a2,a1");
        // c.sub a2,a0
        assert_eq!(render16(0x8e09, 0x1000), "c.sub a2,a0");
        // c.lui a5,0x10
        assert_eq!(render16(0x67c1, 0x1000), "c.lui a5,0x10");
        // c.srli a1,0x14
        assert_eq!(render16(0x81d1, 0x1000), "c.srli a1,0x14");
        // c.addi4spn a1,sp,4
        assert_eq!(render16(0x004c, 0x1000), "c.addi4spn a1,sp,4");
        // c.lwsp a0,0(sp)
        assert_eq!(render16(0x4502, 0x1000), "c.lwsp a0,0(sp)");
        // c.swsp s0,8(sp)
        assert_eq!(render16(0xc422, 0x1000), "c.swsp s0,8(sp)");
        // c.beqz a5,+4 @0x100c6 ⇒ 目标 0x100ca
        assert_eq!(render16(0xc391, 0x100c6), "c.beqz a5,100ca");
        // c.j +0x114 @0x100de ⇒ 目标 0x101f2
        assert_eq!(render16(0xaa11, 0x100de), "c.j 101f2");
        // c.jal +0x434 @0x10108 ⇒ 目标 0x1053c
        assert_eq!(render16(0x2915, 0x10108), "c.jal 1053c");
        // c.j -0x70 @0x10124 ⇒ 目标 0x100b4（负偏移）
        assert_eq!(render16(0xbf41, 0x10124), "c.j 100b4");
    }

    #[test]
    fn compressed_addi16sp_is_recognised_by_its_immediate_range() {
        // C.ADDI16SP，偏移 -64（C.ADDI 的 6 位范围装不下）⇒ c.addi16sp。
        // 编码：quadrant **1**, funct3=011, rd=2, nzimm=-64（10 位补码
        // 0b1111000000 ⇒ nzimm[9]=inst[12], nzimm[8:7]=inst[4:3],
        // nzimm[6]=inst[5], nzimm[5]=inst[2], nzimm[4]=inst[6]）。
        let inst: u16 = (0b011 << 13) | (1 << 12) | (2 << 7) | (1 << 5) | (0b11 << 3) | 0b01;
        assert_eq!(render16(inst, 0x1000), "c.addi16sp sp,-64");
    }

    // ------------------------------------------------------------------
    // 2. 立即数语义：每种来源各一条（含符号扩展）
    // ------------------------------------------------------------------

    #[test]
    fn i_type_immediate_is_sign_extended() {
        // addi a0,a1,-16（12 位立即数 0xFF0 ⇒ 符号扩展成 -16）
        let raw = i_type(0xFF0, 11, 0b000, 10, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "addi a0,a1,-16");
        // addi a0,a1,-2048（最小值）
        let raw = i_type(0x800, 11, 0b000, 10, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "addi a0,a1,-2048");
        // addi a0,a1,2047（最大值）
        let raw = i_type(0x7FF, 11, 0b000, 10, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "addi a0,a1,2047");
    }

    #[test]
    fn s_type_immediate_is_sign_extended() {
        // sw a1,-4(a0)
        let raw = s_type((-4i32 as u32) & 0xFFF, 11, 10, 0b010, 0b010_0011);
        assert_eq!(render32(raw, 0x1000), "sw a1,-4(a0)");
    }

    #[test]
    fn b_type_immediate_is_a_signed_byte_offset() {
        // beq a0,a1,-8 @0x1000 ⇒ 目标 0x0ff8
        let raw = b_type((-8i32 as u32) & 0x1FFF, 11, 10, 0b000, 0b110_0011);
        assert_eq!(render32(raw, 0x1000), "beq a0,a1,ff8");
        // 负偏移要**符号扩展**成 13 位（否则会变成一个巨大的正数）
        let raw = b_type((-4i32 as u32) & 0x1FFF, 11, 10, 0b000, 0b110_0011);
        assert_eq!(render32(raw, 0x1000), "beq a0,a1,ffc");
    }

    #[test]
    fn u_type_immediate_is_the_unsigned_20_bit_field() {
        // lui a0,0xfffff —— 20 位字段全 1（**不是** -1）
        let raw = u_type(0xF_FFFF, 10, 0b011_0111);
        assert_eq!(render32(raw, 0x1000), "lui a0,0xfffff");
    }

    #[test]
    fn j_type_immediate_is_a_signed_byte_offset() {
        // jal ra,-4 @0x1000 ⇒ 目标 0x0ffc
        let raw = j_type((-4i32 as u32) & 0x1F_FFFF, 1, 0b110_1111);
        assert_eq!(render32(raw, 0x1000), "jal ra,ffc");
    }

    #[test]
    fn shift_amount_is_not_a_twelve_bit_immediate() {
        // slli a0,a1,0x2 —— 移位量只有 5 位
        let raw = i_type(2, 11, 0b001, 10, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "slli a0,a1,0x2");
        // srli a0,a1,0x1f
        let raw = i_type(31, 11, 0b101, 10, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "srli a0,a1,0x1f");
        // srai a0,a1,0x2（funct7 = 0b0100000）
        let raw = i_type(0x400 | 2, 11, 0b101, 10, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "srai a0,a1,0x2");
    }

    // ------------------------------------------------------------------
    // 3. 非法指令
    // ------------------------------------------------------------------

    #[test]
    fn renders_illegal_instruction_explicitly() {
        // 保留的压缩编码（0x0000）⇒ M-13 判非法。
        let text = render16(0x0000, 0x1000);
        assert_eq!(text, ".illegal 0x0000");
        // 未知的 32 位 opcode（0x7f）⇒ 二级解码查不到 ⇒ 非法。
        let text = render32(0x0000_007f, 0x1000);
        assert_eq!(text, ".illegal 0x0000007f");
        // 非法文本必须**明确含「非法」**，且带宽度信息。
        assert!(text.contains("illegal"));
    }

    // ------------------------------------------------------------------
    // 4. 渲染入口（契约层 DTO）
    // ------------------------------------------------------------------

    #[test]
    fn render_entry_carries_addr_code_and_width() {
        let d = Decoder::default().decode(r_type(0, 12, 11, 0, 10, 0b011_0011), 0x1000);
        let e = render_entry(&d);
        assert_eq!(e.addr, 0x1000);
        assert_eq!(e.code, 0x00C5_8533);
        assert_eq!(e.mnemonic, "add a0,a1,a2");
        assert_eq!(e.width, 4);

        // 压缩指令：**16 位原码** + width = 2。
        let d = Decoder::default().decode(0x1141, 0x1000);
        let e = render_entry(&d);
        assert_eq!(e.addr, 0x1000);
        assert_eq!(e.code, 0x1141);
        assert_eq!(e.mnemonic, "c.addi sp,-16");
        assert_eq!(e.width, 2);
    }

    #[test]
    fn register_names_are_abi_names_and_x0_is_always_zero() {
        assert_eq!(reg_name(0), "zero");
        assert_eq!(reg_name(1), "ra");
        assert_eq!(reg_name(2), "sp");
        assert_eq!(reg_name(10), "a0");
        assert_eq!(reg_name(28), "t3");
        assert_eq!(abi_to_index("zero"), Some(0));
        assert_eq!(abi_to_index("sp"), Some(2));
        assert_eq!(abi_to_index("nope"), None);
        // x0 的两种写法不能混用：一律 `zero`。
        let raw = i_type(0, 0, 0b000, 0, 0b001_0011);
        assert_eq!(render32(raw, 0x1000), "addi zero,zero,0");
    }
}
