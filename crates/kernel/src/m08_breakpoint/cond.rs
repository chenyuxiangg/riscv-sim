//! 条件断点表达式 —— **解析器 + 求值器**（v0.57 批 · STAGE-42 §4）。
//!
//! ## 语法（冻结 · STAGE-42 §4.1）
//!
//! ```ebnf
//! expr     = or_expr ;
//! or_expr  = and_expr , { "||" , and_expr } ;
//! and_expr = cmp_expr , { "&&" , cmp_expr } ;
//! cmp_expr = unary , [ relop , unary ] ;
//! relop    = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
//! unary    = "!" , unary | primary ;
//! primary  = "(" , expr , ")" | operand ;
//! operand  = number | reg ;
//! number   = dec | hex ;
//! reg      = "pc" | "x" , ( "0".."9" | "1".."2" , "0".."9" | "3" , "0".."1" ) ;
//! ```
//!
//! ⚠️ **ABI 别名**在 `reg` 的位置上等价于某个 `xN`
//! （`zero` · `ra` · `sp` · `gp` · `tp` · `t0`~`t2` · `s0` · `fp` · `s1` ·
//! `a0`~`a7` · `s2`~`s11` · `t3`~`t6`）⇒ ⇒ ⚠️ **`fp` 与 `s1` 都映射
//! `x9`** ⇒ ⇒ 是**同一个**寄存器的两个名字。
//!
//! ## 语义（冻结 · STAGE-42 §4.2）
//!
//! * **值类型**：单个 **u32**（RV32 的 GPR 与 PC 都是 u32）。
//! * **比较**：**无符号**（`<` / `<=` / `>` / `>=` 按 u32 比）⇒ ⇒ 没有
//!   有符号溢出语义。
//! * **真值**：数值 **≠ 0** 即真。
//! * **`&&` / `||`**：**短路**。
//! * **优先级**：`||` < `&&` < 比较 < `!` < 一元/括号。
//!
//! ## 两段式：**语法**（[`parse`]）与**绑定**（[`Condition::bind`]）
//!
//! ⚠️ 名字（`main` / `g_counter` / `sp` 之外的任何标识符）要查符号表 ⇒ ⇒
//! 而本模块**不该**依赖 M-11 ⇒ ⇒ 语法阶段只造一棵**可能含名字**的树，
//! 绑定阶段用一个窄接口（[`super::names::NameResolver`]）把名字换掉 ⇒ ⇒
//! 换完得到的 [`BoundCondition`] **结构上不可能**还含名字 ⇒ ⇒ 「忘了绑定
//! 就求值」这件事**编译期就发生不了**。
//!
//! ## 档 2（本批）能写什么
//!
//! * `main`（**函数名**）⇒ 函数地址常量（函数没有「值」）；
//! * `&main` / `&g_counter` ⇒ 符号的**地址**（不读内容）；
//! * `&[x1+8]` ⇒ 该地址本身（= `x1+8`，**不读**那个 word）；
//! * `&a0` / `&pc` / `&5` ⇒ **静态错**（`E_DBG-02`）：寄存器与字面量没有
//!   可寻址的地址。
//!
//! ## 档 3 才有的（本批**明确拒**，不是漏做）
//!
//! * `g_counter`（**变量名**）⇒ 变量的值 ⇒ **要读内存** ⇒ 档 3；
//! * `[x1+8]`（**读内存**）⇒ 档 3。
//!
//! ⇒ 本批对这两件事的答复是**一句明说**的 `E_DBG-02`（消息里带上「批 C
//! 起支持」与「要地址请写 `&名字`」），**不**静默求值成 0（那才是骗人）。
//!
//! ## `@…` 为什么不在条件文法里
//!
//! `@x2+4` 在**观察表达式**里成立，因为整条表达式就是一个地址；但在条件
//! 表达式里它**没有右边界**（`@x2+4==5` 到底是「地址 x2+4 处的值 == 5」
//! 还是「x2 减去 `4==5`」？）⇒ ⇒ 条件文法只留**带方括号**的写法。
//!
//! ## 解析失败 ⇒ **原样拒**（`E_DBG-02`）
//!
//! ⚠️ 错误消息必须**含**「哪一段不对」的线索（STAGE-42 §4.4）⇒ ⇒ 例如
//! `条件表达式无效：未知寄存器名 "foo"` ⇒ ⇒ 前端**原样透传**，**不吞**。

use std::fmt;

use super::names::{NameClass, NameResolver};
use super::watch_expr::{AddressBase, AddressSpec};

/// 条件表达式的**解析失败**。
///
/// ⚠️ 消息**自带**「哪一段不对」的线索 ⇒ ⇒ 调用方**原样**把它放进
/// `E_DBG_02` 的错误消息里 ⇒ ⇒ 前端能告诉用户「错在哪个记号上」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondError {
    message: String,
}

impl CondError {
    /// 造一个错误：前缀是固定的 `条件表达式无效：` ⇒ ⇒ **一个**消息形状。
    fn new(detail: impl fmt::Display) -> Self {
        Self {
            message: format!("条件表达式无效：{detail}"),
        }
    }

    /// 错误消息（**完整**的那句）。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CondError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CondError {}

/// 二元运算符（**私有**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// `||`
    Or,
    /// `&&`
    And,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl Op {
    /// 运算符的**文本**（错误消息里要引用它）。
    fn text(self) -> &'static str {
        match self {
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }
}

/// 记号（tokenizer 的产物）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// 标识符（寄存器名 / ABI 别名）。
    Ident(String),
    /// 数值字面量。
    Num(u32),
    /// 运算符。
    Op(Op),
    /// `!`（**一元**运算符 ⇒ ⇒ 不在 [`Op`] 里）。
    Not,
    /// `(`。
    LParen,
    /// `)`。
    RParen,
    /// `&`（**一元**取地址 ⇒ ⇒ 不在 [`Op`] 里；`&&` 仍是 [`Op::And`]）。
    Amp,
    /// `[地址]`（方括号内的**地址表达式原文**，如 `x1+8`）。
    Bracket(String),
}

impl Tok {
    /// 记号的**文本**（错误消息里要引用它）。
    fn text(&self) -> String {
        match self {
            Self::Ident(name) => name.clone(),
            Self::Num(value) => value.to_string(),
            Self::Op(op) => op.text().to_string(),
            Self::Not => "!".to_string(),
            Self::LParen => "(".to_string(),
            Self::RParen => ")".to_string(),
            Self::Amp => "&".to_string(),
            Self::Bracket(inner) => format!("[{inner}]"),
        }
    }
}

/// 一条**已解析**的条件表达式。
///
/// ⚠️ 这是**推导量** —— 断点表里存的是**它**，而条件的**文本**住在契约
/// DTO 的 `Breakpoint::condition` 里 ⇒ ⇒ 文本只有**一份** ⇒ ⇒ 不会
/// 「文本与 AST 漂移」。
///
/// ⚠️ **AST 本身不公开**（[`Expr`] 与 [`Op`] 都是**私有**的）⇒ ⇒ 外面只
/// 能「解析」与「求值」⇒ ⇒ 语法**只有**这一个落点 ⇒ ⇒ 不会有人绕过它
/// 自己拼一棵 AST。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    expr: Expr,
}

impl Condition {
    /// **绑定**：把名字换成地址，得到一个能求值的 [`BoundCondition`]。
    ///
    /// ⚠️ **注册期只绑一次** ⇒ ⇒ 求值路径上**没有**字符串处理，也没有
    /// 符号表查询。
    ///
    /// ⚠️ **静态错一律在这里报**（`E_DBG-02`）：
    /// * 未定义符号；
    /// * 变量名（要读内存 ⇒ 档 3，本批明确拒）；
    /// * 读内存的 `[地址]`（同上）。
    pub fn bind(&self, names: &dyn NameResolver) -> Result<BoundCondition, CondError> {
        Ok(BoundCondition {
            expr: bind_expr(&self.expr, names)?,
        })
    }
}

/// 绑定后的条件表达式 —— **唯一能求值的形态**。
///
/// ⚠️ 名字已经在绑定期换掉了 ⇒ ⇒ 这个类型里**没有**「名字」这一格 ⇒ ⇒
/// 结构上保证了「不会拿着一棵没解析的树去求值」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundCondition {
    expr: BoundExpr,
}

impl BoundCondition {
    /// 求值。
    ///
    /// * `gpr` —— 通用寄存器文件（`x0`..`x31`）；
    /// * `pc` —— 当前 PC。
    ///
    /// ⚠️ **求值时刻 = 取指之前** ⇒ ⇒ `gpr` / `pc` 里的值是**断点地址
    /// 那条指令执行前**的值（STAGE-42 §4.2 · 裁决 H）。
    ///
    /// ⚠️ **短路**：`&&` / `||` 的右操作数**可能不被求值** ⇒ ⇒ 求值顺序
    /// 是**定义的一部分**。
    pub fn eval(&self, gpr: &[u32; 32], pc: u32) -> u32 {
        self.expr.eval(gpr, pc)
    }
}

/// 绑定后的 AST。
#[derive(Debug, Clone, PartialEq, Eq)]
enum BoundExpr {
    /// 字面量 / 已解析的符号地址。
    Const(u32),
    /// 通用寄存器。
    Reg(u8),
    /// 程序计数器。
    Pc,
    /// `&[地址]`：**算出地址本身**（不读那个 word）。
    AddrOf(AddressSpec),
    /// 取反。
    Not(Box<BoundExpr>),
    /// 二元运算。
    Bin(Op, Box<BoundExpr>, Box<BoundExpr>),
}

impl BoundExpr {
    fn eval(&self, gpr: &[u32; 32], pc: u32) -> u32 {
        match self {
            Self::Const(value) => *value,
            Self::Reg(index) => gpr[*index as usize],
            Self::Pc => pc,
            Self::AddrOf(spec) => resolve_address(spec, gpr, pc),
            Self::Not(inner) => u32::from(inner.eval(gpr, pc) == 0),
            Self::Bin(op, left, right) => match op {
                // ⚠️ **短路**：左假 ⇒ 右**不求值**。
                Op::And => {
                    if left.eval(gpr, pc) == 0 {
                        0
                    } else {
                        u32::from(right.eval(gpr, pc) != 0)
                    }
                }
                // ⚠️ **短路**：左真 ⇒ 右**不求值**。
                Op::Or => {
                    if left.eval(gpr, pc) != 0 {
                        1
                    } else {
                        u32::from(right.eval(gpr, pc) != 0)
                    }
                }
                Op::Eq => u32::from(left.eval(gpr, pc) == right.eval(gpr, pc)),
                Op::Ne => u32::from(left.eval(gpr, pc) != right.eval(gpr, pc)),
                // ⚠️ **无符号**比较（STAGE-42 §4.2）。
                Op::Lt => u32::from(left.eval(gpr, pc) < right.eval(gpr, pc)),
                Op::Le => u32::from(left.eval(gpr, pc) <= right.eval(gpr, pc)),
                Op::Gt => u32::from(left.eval(gpr, pc) > right.eval(gpr, pc)),
                Op::Ge => u32::from(left.eval(gpr, pc) >= right.eval(gpr, pc)),
            },
        }
    }
}

/// 算一个地址（`基址 ± 偏移`）。
///
/// ⚠️ 绑定后的 [`AddressSpec`] 的基址**只会**是数字 / 寄存器 / pc ⇒ ⇒
/// 剩下的 [`AddressBase::Symbol`] 是内部没绑定 ⇒ ⇒ 不猜，按 0 处理（这
/// 条路走不到：绑定期已经把它换掉了）。
fn resolve_address(spec: &AddressSpec, gpr: &[u32; 32], pc: u32) -> u32 {
    let base = match &spec.base {
        AddressBase::Number(n) => *n,
        AddressBase::Register(r) => gpr[*r as usize],
        AddressBase::Pc => pc,
        AddressBase::Symbol(_) => 0,
    };
    base.wrapping_add(spec.offset as u32)
}

/// **绑定**：把语法树里的名字换成地址。
fn bind_expr(expr: &Expr, names: &dyn NameResolver) -> Result<BoundExpr, CondError> {
    Ok(match expr {
        Expr::Const(value) => BoundExpr::Const(*value),
        Expr::Reg(index) => BoundExpr::Reg(*index),
        Expr::Pc => BoundExpr::Pc,
        // ⚠️ **函数名 ⇒ 地址常量**（函数没有「值」· 档 2）。
        Expr::Name(name) => match names.resolve(name) {
            Some(reference) if reference.class == NameClass::Function => {
                BoundExpr::Const(reference.addr)
            }
            // ⚠️ **变量名 ⇒ 变量的值 ⇒ 要读内存** ⇒ 档 3（批 C）⇒ ⇒ 本批
            //    明说拒，**不**静默当成地址。
            Some(_) => {
                return Err(CondError::new(format!(
                    "全局变量 \"{name}\" 的值要读内存（本批尚未支持，批 C 起支持）；要它的地址请写 &{name}"
                )))
            }
            None => return Err(undefined_name(name)),
        },
        // ⚠️ **取地址 ⇒ 不读内容**（档 2）。
        Expr::AddrOfName(name) => match names.resolve(name) {
            Some(reference) => BoundExpr::Const(reference.addr),
            None => return Err(undefined_name(name)),
        },
        // ⚠️ **读内存**（`[地址]`）⇒ 档 3（批 C）。
        Expr::Mem(spec) => {
            return Err(CondError::new(format!(
                "读内存 {} 要过总线（本批尚未支持，批 C 起支持）；要地址请写 &{}",
                show_spec(spec),
                show_spec(spec)
            )))
        }
        // ⚠️ **取地址** ⇒ 地址表达式 ⇒ 基址里的符号也要在绑定期定下来。
        Expr::AddrOfMem(spec) => BoundExpr::AddrOf(bind_spec(spec, names)?),
        Expr::Not(inner) => BoundExpr::Not(Box::new(bind_expr(inner, names)?)),
        Expr::Bin(op, left, right) => BoundExpr::Bin(
            *op,
            Box::new(bind_expr(left, names)?),
            Box::new(bind_expr(right, names)?),
        ),
    })
}

/// 把地址表达式里的符号基址换成数字基址。
fn bind_spec(spec: &AddressSpec, names: &dyn NameResolver) -> Result<AddressSpec, CondError> {
    let base = match &spec.base {
        AddressBase::Number(n) => AddressBase::Number(*n),
        AddressBase::Register(r) => AddressBase::Register(*r),
        AddressBase::Pc => AddressBase::Pc,
        AddressBase::Symbol(name) => match names.resolve(name) {
            Some(reference) => AddressBase::Number(reference.addr),
            None => return Err(undefined_name(name)),
        },
    };
    Ok(AddressSpec {
        base,
        offset: spec.offset,
    })
}

/// 地址表达式的**回显**（错误消息里用；`&[x1+8]` 的写法与用户输入同形）。
fn show_spec(spec: &AddressSpec) -> String {
    let base = match &spec.base {
        AddressBase::Number(n) => format!("0x{n:x}"),
        AddressBase::Register(r) => format!("x{r}"),
        AddressBase::Pc => "pc".to_string(),
        AddressBase::Symbol(name) => name.clone(),
    };
    if spec.offset == 0 {
        format!("[{base}]")
    } else if spec.offset > 0 {
        format!("[{base}+{}]", spec.offset)
    } else {
        format!("[{base}{}]", spec.offset)
    }
}

/// 「未定义符号」那一条静态错。
fn undefined_name(name: &str) -> CondError {
    CondError::new(format!("未定义符号 \"{name}\""))
}

/// 抽象语法树（**私有** ⇒ ⇒ 只有 [`parse`] 能造）。
///
/// ⚠️ 这是**语法**树：名字（[`Expr::Name`] / [`Expr::AddrOfName`]）还没换
/// 成地址 ⇒ ⇒ 求值**必须**先过 [`Condition::bind`]。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    /// 字面量。
    Const(u32),
    /// 通用寄存器 `x0`..`x31`。
    Reg(u8),
    /// 程序计数器。
    Pc,
    /// **名字**（函数 / 变量 / 其它符号）—— 绑定期才定得下来。
    Name(String),
    /// `&名字` —— 取符号的地址（不看它有没有值）。
    AddrOfName(String),
    /// `[地址]` —— **读**该地址的 word（档 3）。
    Mem(AddressSpec),
    /// `&[地址]` —— 该地址**本身**（档 2，不读内容）。
    AddrOfMem(AddressSpec),
    /// 取反（`!`）。
    Not(Box<Expr>),
    /// 二元运算。
    Bin(Op, Box<Expr>, Box<Expr>),
}

/// ABI 别名 → 寄存器号（STAGE-42 §4.1 的那张表）。
///
/// ⚠️ **这张表是唯一的落点** ⇒ ⇒ 别处**不**再写一份别名映射。
///
/// ⚠️ **`fp` 与 `s1` 都是 `x9`** ⇒ ⇒ 是**同一个**寄存器的两个名字。
fn abi_alias(name: &str) -> Option<u8> {
    Some(match name {
        "zero" => 0,
        "ra" => 1,
        "sp" => 2,
        "gp" => 3,
        "tp" => 4,
        "t0" => 5,
        "t1" => 6,
        "t2" => 7,
        "s0" => 8,
        "fp" => 9,
        "s1" => 9,
        "a0" => 10,
        "a1" => 11,
        "a2" => 12,
        "a3" => 13,
        "a4" => 14,
        "a5" => 15,
        "a6" => 16,
        "a7" => 17,
        "s2" => 18,
        "s3" => 19,
        "s4" => 20,
        "s5" => 21,
        "s6" => 22,
        "s7" => 23,
        "s8" => 24,
        "s9" => 25,
        "s10" => 26,
        "s11" => 27,
        "t3" => 28,
        "t4" => 29,
        "t5" => 30,
        "t6" => 31,
        _ => return None,
    })
}

/// 解析一条条件表达式。
///
/// ⚠️ **失败 ⇒ 原样拒**（`E_DBG_02`）⇒ ⇒ 错误消息带「哪一段不对」的线索。
///
/// ⚠️ **空表达式（`""` / 纯空白）也是非法**（STAGE-42 §4.4）⇒ ⇒ 前端
/// **不许**发空串（要发就发 `null` ⇒ 契约层就是 `None`）。
pub fn parse(text: &str) -> Result<Condition, CondError> {
    let tokens = tokenize(text)?;
    if tokens.is_empty() {
        return Err(CondError::new("表达式为空"));
    }
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_expr()?;
    if let Some(token) = parser.peek() {
        // ⚠️ 多出来的 `)` ⇒ ⇒ 报「括号不配对」（而不是「多余的记号」）
        //    ⇒ ⇒ 用户看得懂是哪一头错了。
        if *token == Tok::RParen {
            return Err(CondError::new("括号不配对：多余的 \")\""));
        }
        return Err(CondError::new(format!("多余的记号 \"{}\"", token.text())));
    }
    Ok(Condition { expr })
}

/// 词法分析。
fn tokenize(text: &str) -> Result<Vec<Tok>, CondError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        match ch {
            '(' => {
                out.push(Tok::LParen);
                index += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                index += 1;
            }
            '!' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(Tok::Op(Op::Ne));
                    index += 2;
                } else {
                    out.push(Tok::Not);
                    index += 1;
                }
            }
            '=' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(Tok::Op(Op::Eq));
                    index += 2;
                } else {
                    // ⚠️ `=` **不是**运算符（STAGE-42 §4.4 的 `x1 = 5`）。
                    return Err(CondError::new("非法运算符 \"=\"（应当是 \"==\"）"));
                }
            }
            '<' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(Tok::Op(Op::Le));
                    index += 2;
                } else {
                    out.push(Tok::Op(Op::Lt));
                    index += 1;
                }
            }
            '>' => {
                if chars.get(index + 1) == Some(&'=') {
                    out.push(Tok::Op(Op::Ge));
                    index += 2;
                } else {
                    out.push(Tok::Op(Op::Gt));
                    index += 1;
                }
            }
            '&' => {
                if chars.get(index + 1) == Some(&'&') {
                    out.push(Tok::Op(Op::And));
                    index += 2;
                } else {
                    // ⚠️ **单个 `&` 是取地址**（档 2）⇒ ⇒ 不再是「非法运算符」。
                    out.push(Tok::Amp);
                    index += 1;
                }
            }
            // ⚠️ **方括号内的地址原文**：整段当一个记号（`[x1+8]` 里的
            //   `+` 不在条件文法里，它属于**地址**那套文法）⇒ ⇒ 交给
            //   `watch_expr` 的地址解析。
            '[' => {
                let start = index + 1;
                let mut end = start;
                while end < chars.len() && chars[end] != ']' {
                    end += 1;
                }
                if end >= chars.len() {
                    return Err(CondError::new("括号不配对：缺少 \"]\""));
                }
                let inner: String = chars[start..end].iter().collect();
                out.push(Tok::Bracket(inner));
                index = end + 1;
            }
            '|' => {
                if chars.get(index + 1) == Some(&'|') {
                    out.push(Tok::Op(Op::Or));
                    index += 2;
                } else {
                    return Err(CondError::new("非法运算符 \"|\"（应当是 \"||\"）"));
                }
            }
            // ⚠️ **以数字开头** ⇒ 一律当作**一个字面量** ⇒ ⇒ `123abc` 与
            //   `0x` 都是「非法字面量」（STAGE-42 §4.4），**不是**「两个
            //   记号」。
            ch if ch.is_ascii_digit() => {
                let start = index;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                let literal: String = chars[start..index].iter().collect();
                out.push(Tok::Num(parse_number(&literal)?));
            }
            ch if ch.is_ascii_alphabetic() || ch == '_' => {
                let start = index;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                out.push(Tok::Ident(chars[start..index].iter().collect()));
            }
            other => {
                return Err(CondError::new(format!("非法字符 \"{other}\"")));
            }
        }
    }
    Ok(out)
}

/// 解析一个字面量（十进制 / `0x` 十六进制）。
fn parse_number(literal: &str) -> Result<u32, CondError> {
    if let Some(hex) = literal.strip_prefix("0x") {
        if hex.is_empty() || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(CondError::new(format!("非法字面量 \"{literal}\"")));
        }
        return u32::from_str_radix(hex, 16)
            .map_err(|_| CondError::new(format!("字面量 \"{literal}\" 超出 u32 范围")));
    }
    if literal.chars().all(|ch| ch.is_ascii_digit()) {
        return literal
            .parse::<u32>()
            .map_err(|_| CondError::new(format!("字面量 \"{literal}\" 超出 u32 范围")));
    }
    Err(CondError::new(format!("非法字面量 \"{literal}\"")))
}

/// 递归下降解析器。
struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    /// 看下一个记号（**不**消费）。
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    /// 消费下一个记号。
    fn bump(&mut self) -> Option<Tok> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    /// `expr = or_expr`
    fn parse_expr(&mut self) -> Result<Expr, CondError> {
        self.parse_or()
    }

    /// `or_expr = and_expr , { "||" , and_expr }`
    fn parse_or(&mut self) -> Result<Expr, CondError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Tok::Op(Op::Or))) {
            self.pos += 1;
            let right = self.parse_and()?;
            left = Expr::Bin(Op::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `and_expr = cmp_expr , { "&&" , cmp_expr }`
    fn parse_and(&mut self) -> Result<Expr, CondError> {
        let mut left = self.parse_cmp()?;
        while matches!(self.peek(), Some(Tok::Op(Op::And))) {
            self.pos += 1;
            let right = self.parse_cmp()?;
            left = Expr::Bin(Op::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `cmp_expr = unary , [ relop , unary ]`
    fn parse_cmp(&mut self) -> Result<Expr, CondError> {
        let left = self.parse_unary()?;
        let op = match self.peek() {
            Some(Tok::Op(op @ (Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge))) => *op,
            _ => return Ok(left),
        };
        self.pos += 1;
        // ⚠️ 缺右操作数（STAGE-42 §4.4 的 `x1 ==`）⇒ ⇒ **单独**一条消息
        //    ⇒ ⇒ 不被下面的通用「缺少操作数」盖掉。
        if self.peek().is_none() {
            return Err(CondError::new("缺少右操作数"));
        }
        let right = self.parse_unary()?;
        Ok(Expr::Bin(op, Box::new(left), Box::new(right)))
    }

    /// `unary = "!" , unary | "&" , addr | primary`
    ///
    /// ⚠️ **`&` 只作用于「名字」与「[地址]」** —— 它们是仅有的两种「有地址
    /// 的东西」⇒ ⇒ `&a0`（寄存器）/ `&5`（字面量）/ `&pc` 在这里就成
    /// **静态错**（`E_DBG-02`），不用等到绑定。
    fn parse_unary(&mut self) -> Result<Expr, CondError> {
        if matches!(self.peek(), Some(Tok::Not)) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        if matches!(self.peek(), Some(Tok::Amp)) {
            self.pos += 1;
            return match self.bump() {
                Some(Tok::Ident(name)) => {
                    // ⚠️ 寄存器 / `pc` / `xN` **没有可寻址的地址**（STAGE-55
                    //   表 8 的 `&a0` 那一行）。
                    if name == "pc" || abi_alias(&name).is_some() || is_register_name(&name) {
                        return Err(CondError::new(format!(
                            "取地址 \"&{name}\" 无效：寄存器没有可寻址的地址"
                        )));
                    }
                    Ok(Expr::AddrOfName(name))
                }
                Some(Tok::Bracket(inner)) => Ok(Expr::AddrOfMem(parse_bracket(&inner)?)),
                Some(other) => Err(CondError::new(format!(
                    "\"&\" 只能作用于符号名或 [地址]，这里接的是 \"{}\"",
                    other.text()
                ))),
                None => Err(CondError::new("\"&\" 后面缺少操作数")),
            };
        }
        self.parse_primary()
    }

    /// `primary = "(" , expr , ")" | operand`
    fn parse_primary(&mut self) -> Result<Expr, CondError> {
        match self.bump() {
            Some(Tok::LParen) => {
                let inner = self.parse_expr()?;
                match self.bump() {
                    Some(Tok::RParen) => Ok(inner),
                    // ⚠️ 括号不配对（STAGE-42 §4.4 的 `(x1 == 5`）。
                    _ => Err(CondError::new("括号不配对：缺少 \")\"")),
                }
            }
            Some(Tok::Num(value)) => Ok(Expr::Const(value)),
            // ⚠️ **`[地址]` = 读内存**（档 3）⇒ ⇒ 语法上认它，绑定期明说拒。
            Some(Tok::Bracket(inner)) => Ok(Expr::Mem(parse_bracket(&inner)?)),
            Some(Tok::Ident(name)) => Ok(operand_from_ident(&name)?),
            Some(Tok::RParen) => Err(CondError::new("括号不配对：多余的 \")\"")),
            // ⚠️ `&` 已在 `parse_unary` 里被吃掉了 ⇒ ⇒ 走到这里是**右操作数
            //   缺在 `&` 后面**那一类（不该发生，兜底）。
            Some(Tok::Amp) => Err(CondError::new("\"&\" 前面缺少操作数")),
            Some(Tok::Not) => Err(CondError::new("运算符 \"!\" 前面缺少操作数")),
            Some(Tok::Op(op)) => Err(CondError::new(format!(
                "运算符 \"{}\" 前面缺少操作数",
                op.text()
            ))),
            None => Err(CondError::new("缺少操作数")),
        }
    }
}

/// 标识符 → 操作数（`pc` / `xN` / ABI 别名）。
fn operand_from_ident(name: &str) -> Result<Expr, CondError> {
    if name == "pc" {
        return Ok(Expr::Pc);
    }
    if let Some(rest) = name.strip_prefix('x') {
        // ⚠️ `x` 后面**必须**是纯数字 ⇒ ⇒ `x1x` 落到「未知寄存器名」。
        if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) {
            return match rest.parse::<u32>() {
                // ⚠️ 寄存器号越界（STAGE-42 §4.4 的 `x32`）：只有 `x0`..`x31`。
                Ok(index) if index <= 31 => Ok(Expr::Reg(index as u8)),
                _ => Err(CondError::new(format!(
                    "寄存器号越界 \"{name}\"（只有 x0..x31）"
                ))),
            };
        }
    }
    if let Some(index) = abi_alias(name) {
        return Ok(Expr::Reg(index));
    }
    // ⚠️ 不是寄存器 ⇒ ⇒ **一个名字**（函数 / 变量 / 其它符号）⇒ ⇒ 由绑定
    //   期查符号表定夺（STAGE-42 时代这里直接报「未知寄存器名」，档 2 起
    //   名字是合法语法）。
    Ok(Expr::Name(name.to_string()))
}

/// 这个名字是不是 `xN` 写法（`x0`..`x31`）。
fn is_register_name(name: &str) -> bool {
    match name.strip_prefix('x') {
        Some(rest) if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) => rest
            .parse::<u32>()
            .map(|index| index <= 31)
            .unwrap_or(false),
        _ => false,
    }
}

/// 解析方括号里的地址原文（**去掉空白**，`[x1 + 8]` 与 `[x1+8]` 等价）。
///
/// ⚠️ 符号基址**先原样留着**（[`AddressBase::Symbol`]）⇒ ⇒ 绑定期才换成
/// 数字 ⇒ ⇒ 「未定义符号」这条静态错因此落在绑定期。
fn parse_bracket(inner: &str) -> Result<AddressSpec, CondError> {
    let text: String = inner.chars().filter(|ch| !ch.is_whitespace()).collect();
    if text.is_empty() {
        return Err(CondError::new("方括号里缺少地址"));
    }
    match super::watch_expr::parse_address_syntax(&text) {
        Ok(spec) => Ok(spec),
        Err(err) => Err(CondError::new(err.message())),
    }
}

#[cfg(test)]
mod tests {
    use super::super::names::NoNames;
    use super::*;

    /// 造一份寄存器文件：`gpr[i] = value`。
    fn gpr(values: &[(usize, u32)]) -> [u32; 32] {
        let mut out = [0u32; 32];
        for (index, value) in values {
            out[*index] = *value;
        }
        out
    }

    /// 解析 + **绑定（无符号上下文）** + 求值 ⇒ 值（真值口径：≠ 0 即真）。
    fn eval(expr: &str, gpr: &[u32; 32], pc: u32) -> u32 {
        parse(expr)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能解析：{e}"))
            .bind(&NoNames)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能绑定：{e}"))
            .eval(gpr, pc)
    }

    /// 带**一张小符号表**的解析 + 绑定 + 求值。
    fn eval_with(expr: &str, names: &dyn NameResolver, gpr: &[u32; 32], pc: u32) -> u32 {
        parse(expr)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能解析：{e}"))
            .bind(names)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能绑定：{e}"))
            .eval(gpr, pc)
    }

    /// 一张**假的符号表**：`(名字, 分类, 地址)`。
    struct FakeNames(Vec<(&'static str, NameClass, u32)>);

    impl NameResolver for FakeNames {
        fn resolve(&self, name: &str) -> Option<crate::m08_breakpoint::names::NameRef> {
            self.0
                .iter()
                .find(|(candidate, _, _)| *candidate == name)
                .map(|(_, class, addr)| crate::m08_breakpoint::names::NameRef {
                    class: *class,
                    addr: *addr,
                })
        }
    }

    /// 解析一条表达式（测试用）⇒ ⇒ 拿到**私有**的 AST。
    fn ast(expr: &str) -> Expr {
        parse(expr)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能解析：{e}"))
            .expr
    }

    // -- §4.4 的每一行都是**一条红**（STAGE-42 §6.2 · K-9） ------------------

    /// **空表达式**（`""` / 纯空白）。
    #[test]
    fn k9_an_empty_expression_is_rejected() {
        for text in ["", "   ", "\t\n"] {
            let err = parse(text).expect_err("空表达式必须被拒");
            assert!(
                err.message().contains("空"),
                "错误消息要指出「空」：{}",
                err.message()
            );
        }
    }

    /// **寄存器号越界**（`x32`）。
    #[test]
    fn k9_a_register_number_out_of_range_is_rejected() {
        let err = parse("x32").expect_err("x32 必须被拒");
        assert!(
            err.message().contains("x32") && err.message().contains("越界"),
            "错误消息要指出「哪个寄存器号越界」：{}",
            err.message()
        );
    }

    /// **未定义符号**（`foo`）—— 档 2 起，标识符是**合法语法**（可能是
    /// 符号名）⇒ ⇒ 这条错落在**绑定**期（`: `parse` 只认形状）。
    #[test]
    fn k9_an_undefined_name_is_rejected_at_bind_time() {
        let cond = parse("foo").expect("标识符是合法语法（可能是符号名）");
        let err = cond.bind(&NoNames).expect_err("没有符号表时 foo 必须被拒");
        assert!(
            err.message().contains("未定义符号") && err.message().contains("foo"),
            "错误消息要指出「哪个名字不认识」：{}",
            err.message()
        );
    }

    /// **缺右操作数**（`x1 ==`）。
    #[test]
    fn k9_a_missing_right_operand_is_rejected() {
        let err = parse("x1 ==").expect_err("x1 == 必须被拒");
        assert!(
            err.message().contains("缺少右操作数"),
            "错误消息要指出「缺右操作数」：{}",
            err.message()
        );
    }

    /// **非法运算符**（`x1 = 5`）。
    #[test]
    fn k9_a_single_equals_sign_is_rejected() {
        let err = parse("x1 = 5").expect_err("x1 = 5 必须被拒");
        assert!(
            err.message().contains("非法运算符") && err.message().contains("="),
            "错误消息要指出「哪个运算符不合法」：{}",
            err.message()
        );
    }

    /// **括号不配对**（`(x1 == 5`）。
    #[test]
    fn k9_unbalanced_parentheses_are_rejected() {
        let err = parse("(x1 == 5").expect_err("(x1 == 5 必须被拒");
        assert!(
            err.message().contains("括号不配对"),
            "错误消息要指出「括号不配对」：{}",
            err.message()
        );
        // 多出来的那个也一样。
        let err = parse("x1 == 5)").expect_err("x1 == 5) 必须被拒");
        assert!(
            err.message().contains("括号不配对"),
            "错误消息要指出「括号不配对」：{}",
            err.message()
        );
    }

    /// **非法字面量**（`0x` / `123abc`）。
    #[test]
    fn k9_illegal_literals_are_rejected() {
        for text in ["0x", "123abc"] {
            let err = parse(text).expect_err("非法字面量必须被拒");
            assert!(
                err.message().contains("非法字面量") && err.message().contains(text),
                "错误消息要指出「哪个字面量不合法」：{}",
                err.message()
            );
        }
    }

    // -- §4.3 的每一行都是**一条红**（STAGE-42 §6.2 · K-10） -----------------

    /// `x1 == 5` —— x1 等于 5 时停。
    #[test]
    fn k10_x1_equals_five() {
        assert_eq!(eval("x1 == 5", &gpr(&[(1, 5)]), 0), 1);
        assert_eq!(eval("x1 == 5", &gpr(&[(1, 6)]), 0), 0);
    }

    /// `a0 != 0` —— ABI 别名。
    #[test]
    fn k10_a0_not_equals_zero_uses_the_abi_alias() {
        assert_eq!(eval("a0 != 0", &gpr(&[(10, 7)]), 0), 1);
        assert_eq!(eval("a0 != 0", &gpr(&[(10, 0)]), 0), 0);
    }

    /// `pc >= 0x80001000` —— 十六进制字面量 + PC。
    #[test]
    fn k10_pc_at_least_a_hex_address() {
        assert_eq!(eval("pc >= 0x80001000", &gpr(&[]), 0x8000_1000), 1);
        assert_eq!(eval("pc >= 0x80001000", &gpr(&[]), 0x8000_0FFF), 0);
    }

    /// `x1 == 5 && x2 != 0` —— 两条同时成立才停。
    #[test]
    fn k10_and_requires_both_sides() {
        assert_eq!(eval("x1 == 5 && x2 != 0", &gpr(&[(1, 5), (2, 3)]), 0), 1);
        assert_eq!(eval("x1 == 5 && x2 != 0", &gpr(&[(1, 5), (2, 0)]), 0), 0);
        assert_eq!(eval("x1 == 5 && x2 != 0", &gpr(&[(1, 4), (2, 3)]), 0), 0);
    }

    /// `x1 == 5 || x2 == 7` —— 任一成立就停。
    #[test]
    fn k10_or_requires_only_one_side() {
        assert_eq!(eval("x1 == 5 || x2 == 7", &gpr(&[(1, 5), (2, 0)]), 0), 1);
        assert_eq!(eval("x1 == 5 || x2 == 7", &gpr(&[(1, 0), (2, 7)]), 0), 1);
        assert_eq!(eval("x1 == 5 || x2 == 7", &gpr(&[(1, 0), (2, 0)]), 0), 0);
    }

    /// `!(x1 == 0)` —— 取反。
    #[test]
    fn k10_not_inverts() {
        // x1 == 0 **成立** ⇒ 取反 ⇒ 假（0）。
        assert_eq!(eval("!(x1 == 0)", &gpr(&[(1, 0)]), 0), 0);
        // x1 == 0 **不成立** ⇒ 取反 ⇒ 真（1）。
        assert_eq!(eval("!(x1 == 0)", &gpr(&[(1, 9)]), 0), 1);
    }

    /// `s0 == sp` —— 两个寄存器比。
    #[test]
    fn k10_two_registers_compare() {
        assert_eq!(eval("s0 == sp", &gpr(&[(8, 4), (2, 4)]), 0), 1);
        assert_eq!(eval("s0 == sp", &gpr(&[(8, 4), (2, 5)]), 0), 0);
    }

    // -- 优先级与括号 -------------------------------------------------------

    /// **`&&` 比 `||` 紧**：`a || b && c` 解析成 `a || (b && c)`。
    #[test]
    fn the_and_operator_binds_tighter_than_or() {
        // x1 = 0 ⇒ 若解析成 `(x1 || x2) && x3` ⇒ 结果 0（x3 = 0）；
        // 若解析成 `x1 || (x2 && x3)` ⇒ 结果 0（x1 = 0 ⇒ 短路）。
        // ⇒ 换一组能区分的值：x1 = 1、x2 = 0、x3 = 0。
        // `x1 || (x2 && x3)` ⇒ 1；`(x1 || x2) && x3` ⇒ 0。
        assert_eq!(
            eval("x1 || x2 && x3", &gpr(&[(1, 1), (2, 0), (3, 0)]), 0),
            1
        );
    }

    /// **比较比 `&&` 紧**：`x1 == 5 && x2 != 0` 不是 `(x1 == (5 && x2)) != 0`。
    #[test]
    fn the_comparison_binds_tighter_than_and() {
        assert_eq!(eval("x1 == 5 && x2 != 0", &gpr(&[(1, 5), (2, 1)]), 0), 1);
    }

    /// **括号可以覆盖优先级**。
    #[test]
    fn parentheses_override_the_precedence() {
        // `(x1 || x2) && x3` ⇒ x1 = 1 ⇒ 左真 ⇒ 再看 x3 = 0 ⇒ 0。
        assert_eq!(
            eval("(x1 || x2) && x3", &gpr(&[(1, 1), (2, 0), (3, 0)]), 0),
            0
        );
    }

    /// **`!` 比比较紧**：`!x1 == 0` 解析成 `(!x1) == 0` ⇒ x1 = 0 ⇒
    /// `!0` = 1 ⇒ `1 == 0` = 0。
    #[test]
    fn the_not_operator_binds_tighter_than_the_comparison() {
        assert_eq!(eval("!x1 == 0", &gpr(&[(1, 0)]), 0), 0);
        assert_eq!(eval("!x1 == 0", &gpr(&[(1, 3)]), 0), 1);
    }

    // -- 别名表 -------------------------------------------------------------

    /// **ABI 别名表逐条**对上（STAGE-42 §4.1）。
    #[test]
    fn every_abi_alias_maps_to_the_right_register() {
        let cases: &[(&str, usize)] = &[
            ("zero", 0),
            ("ra", 1),
            ("sp", 2),
            ("gp", 3),
            ("tp", 4),
            ("t0", 5),
            ("t1", 6),
            ("t2", 7),
            ("s0", 8),
            ("fp", 9),
            ("s1", 9),
            ("a0", 10),
            ("a1", 11),
            ("a2", 12),
            ("a3", 13),
            ("a4", 14),
            ("a5", 15),
            ("a6", 16),
            ("a7", 17),
            ("s2", 18),
            ("s3", 19),
            ("s4", 20),
            ("s5", 21),
            ("s6", 22),
            ("s7", 23),
            ("s8", 24),
            ("s9", 25),
            ("s10", 26),
            ("s11", 27),
            ("t3", 28),
            ("t4", 29),
            ("t5", 30),
            ("t6", 31),
        ];
        for (alias, index) in cases {
            assert_eq!(ast(alias), Expr::Reg(*index as u8), "别名 {alias} 映射错了");
        }
    }

    /// **`fp` 与 `s1` 是同一个寄存器**（`x9`）。
    #[test]
    fn fp_and_s1_are_the_same_register() {
        assert_eq!(ast("fp"), ast("s1"));
        assert_eq!(ast("fp"), Expr::Reg(9));
    }

    // -- 求值口径 -----------------------------------------------------------

    /// **比较是无符号的**（`0xFFFFFFFF > 0` 为真）。
    #[test]
    fn the_comparison_is_unsigned() {
        assert_eq!(eval("x1 > 0", &gpr(&[(1, 0xFFFF_FFFF)]), 0), 1);
        assert_eq!(eval("x1 < 0", &gpr(&[(1, 0xFFFF_FFFF)]), 0), 0);
    }

    /// **真值 = 值 ≠ 0**。
    ///
    /// ⚠️ 表达式的**值**是一个 u32（**不**被折成 0/1）⇒ ⇒ 「真」这件事
    ///    由**调用方**按「≠ 0」判定（断点表就是这么判的）。
    #[test]
    fn the_truth_value_is_not_zero() {
        assert_eq!(eval("x1", &gpr(&[(1, 1)]), 0), 1);
        assert_eq!(eval("x1", &gpr(&[(1, 0)]), 0), 0);
        // ⚠️ `0xFFFFFFFF` **不等于 0** ⇒ ⇒ 它是**真**的（尽管不是 1）。
        assert_eq!(eval("x1", &gpr(&[(1, 0xFFFF_FFFF)]), 0), 0xFFFF_FFFF);
        assert_ne!(eval("x1", &gpr(&[(1, 0xFFFF_FFFF)]), 0), 0);
    }

    /// **`&&` / `||` 短路**（右操作数不被求值 ⇒ 结果与右边的值无关）。
    #[test]
    fn the_logical_operators_short_circuit() {
        // `x1 && x2`：x1 = 0 ⇒ 右**不求值** ⇒ 结果 0（哪怕 x2 = 9）。
        assert_eq!(eval("x1 && x2", &gpr(&[(1, 0), (2, 9)]), 0), 0);
        // `x1 || x2`：x1 = 1 ⇒ 右**不求值** ⇒ 结果 1（哪怕 x2 = 0）。
        assert_eq!(eval("x1 || x2", &gpr(&[(1, 1), (2, 0)]), 0), 1);
    }

    /// **`mem[...]` 仍不是语法**（裁决 G 未变：没有「mem」这种前缀写法）
    /// ⇒ ⇒ 它落成**名字** `mem` 后面跟一个方括号 ⇒ ⇒ 绑定期的未定义符号。
    #[test]
    fn mem_bracket_syntax_is_still_not_a_thing() {
        let err = parse("mem[0x1000] == 5").expect_err("`mem[...]` 不是文法");
        assert!(
            err.message().contains("条件表达式无效"),
            "错误消息要有统一前缀：{}",
            err.message()
        );
    }

    // -- 档 2：名字 / 取地址 / 方括号 -----------------------------------------

    /// 一张小符号表：`main`（函数）、`g_counter`（对象）、`_start`（其它）。
    fn names() -> FakeNames {
        FakeNames(vec![
            ("main", NameClass::Function, 0x1000),
            ("g_counter", NameClass::Object, 0x2000),
            ("_start", NameClass::Other, 0x0100),
        ])
    }

    /// **函数名 = 地址常量**（函数没有「值」）。
    #[test]
    fn a_function_name_is_its_address_constant() {
        assert_eq!(eval_with("main == 0x1000", &names(), &gpr(&[]), 0), 1);
        assert_eq!(eval_with("main == 0x1001", &names(), &gpr(&[]), 0), 0);
        // 和寄存器一起用也行。
        assert_eq!(
            eval_with("pc == main && x1 == 3", &names(), &gpr(&[(1, 3)]), 0x1000),
            1
        );
    }

    /// **`&名字` = 符号的地址**，不读内容（对象也一样）。
    #[test]
    fn address_of_a_name_is_the_symbol_address() {
        assert_eq!(eval_with("&g_counter == 0x2000", &names(), &gpr(&[]), 0), 1);
        assert_eq!(eval_with("&main == 0x1000", &names(), &gpr(&[]), 0), 1);
        // ⚠️ **取地址不读内存** ⇒ ⇒ 对象里存什么完全不影响这条比较。
        assert_eq!(eval_with("&g_counter != 0", &names(), &gpr(&[]), 0), 1);
    }

    /// **寄存器取地址被拒**（`&a0` / `&pc` / `&x1`）。
    #[test]
    fn address_of_a_register_is_rejected() {
        for text in ["&a0 == 1", "&pc == 1", "&x1 == 1", "&sp == 1"] {
            let err = parse(text).expect_err("寄存器没有可寻址的地址");
            assert!(
                err.message().contains("取地址") && err.message().contains("寄存器"),
                "错误消息要指出「寄存器取不到地址」：{}",
                err.message()
            );
        }
    }

    /// **字面量取地址被拒**（`&5`）。
    #[test]
    fn address_of_a_literal_is_rejected() {
        let err = parse("&5 == 1").expect_err("字面量没有地址");
        assert!(
            err.message().contains("&") && err.message().contains("只能作用于"),
            "错误消息要说明 & 的适用范围：{}",
            err.message()
        );
    }

    /// **变量名 = 变量的值 ⇒ 要读内存 ⇒ 档 3 才有** ⇒ ⇒ 本批明说拒。
    #[test]
    fn a_variable_name_is_rejected_as_unimplemented_read() {
        let cond = parse("g_counter == 3").expect("语法合法");
        let err = cond.bind(&names()).expect_err("变量名要读内存，本批不支持");
        assert!(
            err.message().contains("读内存") && err.message().contains("g_counter"),
            "错误消息要指出「要读内存」以及是哪个名字：{}",
            err.message()
        );
        // ⚠️ 顺手给出**可用的替代写法**（消息里那句「要它的地址请写 &…」）。
        assert!(
            err.message().contains("&g_counter"),
            "错误消息要给替代写法：{}",
            err.message()
        );
    }

    /// **`[地址]` 读内存**同样明说拒（档 3）。
    #[test]
    fn a_memory_read_is_rejected_as_unimplemented() {
        let cond = parse("[x1+8] == 5").expect("方括号是合法语法");
        let err = cond.bind(&names()).expect_err("读内存本批不支持");
        assert!(
            err.message().contains("读内存") && err.message().contains("[x1+8]"),
            "错误消息要回显那个地址表达式：{}",
            err.message()
        );
    }

    /// **`&[地址]` = 地址本身**（档 2，不读那个 word）。
    #[test]
    fn address_of_a_bracket_is_the_raw_address() {
        // x1 = 0x1000 ⇒ `&[x1+8]` = 0x1008。
        assert_eq!(
            eval_with("&[x1+8] == 0x1008", &names(), &gpr(&[(1, 0x1000)]), 0),
            1
        );
        assert_eq!(
            eval_with("&[x1+8] == 0x1009", &names(), &gpr(&[(1, 0x1000)]), 0),
            0
        );
        // 方括号里的空白不影响。
        assert_eq!(
            eval_with("&[ x1 + 8 ] == 0x1008", &names(), &gpr(&[(1, 0x1000)]), 0),
            1
        );
    }

    /// **方括号里的符号基址**（`&[main+4]`）。
    #[test]
    fn a_symbol_base_inside_a_bracket_resolves() {
        assert_eq!(eval_with("&[main+4] == 0x1004", &names(), &gpr(&[]), 0), 1);
        // 未定义 ⇒ 静态错。
        let cond = parse("&[nope+4] == 0").expect("语法合法");
        let err = cond.bind(&names()).expect_err("未定义符号必须被拒");
        assert!(
            err.message().contains("未定义符号") && err.message().contains("nope"),
            "错误消息要指出哪个名字没定义：{}",
            err.message()
        );
    }

    /// **`STT_NOTYPE` 这类符号只许取地址**（不许当函数名用）。
    #[test]
    fn an_other_class_symbol_is_only_addressable() {
        let cond = parse("_start == 0x100").expect("语法合法");
        assert!(
            cond.bind(&names()).is_err(),
            "未分类符号不许当值用（它没有值的语义）"
        );
        assert_eq!(eval_with("&_start == 0x100", &names(), &gpr(&[]), 0), 1);
    }

    /// **裸数字仍是字面常量**（不与 watch_expr 的「裸数字=地址」混淆）。
    #[test]
    fn a_bare_number_stays_a_literal_even_with_symbols() {
        assert_eq!(eval_with("0x1000 == 0x1000", &names(), &gpr(&[]), 0), 1);
        assert_eq!(eval_with("0x2000 == &g_counter", &names(), &gpr(&[]), 0), 1);
    }

    /// **绑定后的树里没有名字**（`&名字` 也已经换成了常量）。
    #[test]
    fn binding_removes_every_name() {
        let cond = parse("main == &g_counter").expect("语法合法");
        let bound = cond.bind(&names()).expect("能绑定");
        // ⚠️ 求值与**符号表无关**：换一张空表来求值，结果一样。
        let a = bound.eval(&gpr(&[]), 0);
        let b = cond.bind(&names()).expect("能绑定").eval(&gpr(&[]), 0);
        assert_eq!(a, b, "绑定后求值不该再依赖符号表");
        assert_eq!(a, 0, "0x1000 == 0x2000 为假");
    }
}
