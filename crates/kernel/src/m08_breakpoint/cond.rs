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
//! ## 档 3（批 C）：变量值与 `[地址]` 读内存
//!
//! * `g_counter`（**变量名**）⇒ 变量的**值** ⇒ 读它地址处的那个 word；
//! * `[x1+8]` / `[main+4]` ⇒ 读该地址的那个 word。
//!
//! ⚠️ **读内存要过总线** ⇒ ⇒ 求值需要一个 `MemReader`（宿主喂 M-14 总线）
//! ⇒ ⇒ 求值结果变成 `Option`：**读失败 = 求值失败 = 本条未命中**（不是
//! 「求值为假」——那会把「读不到」伪装成一个结论）。
//!
//! ⚠️ **含内存读的条目只在 store 边界求值**（表级
//! `hit_check_pred_at_store`）：store 不改 GPR ⇒ ⇒ 没有 store 时内存里的值
//! 不会变 ⇒ ⇒ 在取指前读一遍纯属白读。
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

use super::names::{LocalSlot, NameClass, NameRef, NameResolver, NameTarget, ResolveError};

use self::addr::{AddressBase, AddressSpec};

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
    /// 符号查询（**全局视角**：不看当前帧）。
    ///
    /// ⚠️ **静态错一律在这里报**（`E_DBG-02`）：未定义符号、同名局部变量
    /// 分不清、寄存器里的变量被取地址……
    pub fn bind(&self, names: &dyn NameResolver) -> Result<BoundCondition, CondError> {
        self.bind_at(names, None)
    }

    /// 带**帧上下文**的绑定（批 D）。
    ///
    /// * `scope = Some(addr)` ⇒ 地址类断点知道自己站在哪个函数里 ⇒ ⇒ 局部
    ///   变量按**那个函数**解析（同名不歧义）；
    /// * `scope = None` ⇒ 无地址的条件断点 ⇒ ⇒ 局部变量只凭名字（多个函数
    ///   都有 ⇒ 报「在多个函数里都有」，**不**随手挑一个）。
    pub fn bind_at(
        &self,
        names: &dyn NameResolver,
        scope: Option<u32>,
    ) -> Result<BoundCondition, CondError> {
        Ok(BoundCondition {
            expr: bind_expr(&self.expr, names, scope)?,
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
    pub fn eval(&self, gpr: &[u32; 32], pc: u32, mem: &dyn MemReader) -> Option<u32> {
        self.expr.eval(gpr, pc, mem)
    }

    /// 这条谓词**要不要读内存**（决定它在哪个时刻求值）。
    pub fn reads_memory(&self) -> bool {
        self.expr.reads_memory()
    }

    /// 这条谓词**引用了 pc 吗**。
    ///
    /// ⚠️ store 提交后 `pc` 已推进 ⇒ ⇒ 引用 `pc` 的条目**不能**做区间过滤
    /// （过滤的前提「不碰这块内存，谓词读到的每个值都不变」对 `pc` 不成立）。
    pub fn reads_pc(&self) -> bool {
        self.expr.reads_pc()
    }

    /// 这条谓词**读哪些地址区间**（`(起始地址, 长度)`，长度恒为 4）。
    ///
    /// ⚠️ 算地址**只读 GPR / pc，不过总线** ⇒ ⇒ 连「先算地址再判」都不产生
    /// 内存访问（STAGE-55 表 6）。
    pub fn load_intervals(&self, gpr: &[u32; 32], pc: u32) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        self.expr.collect_loads(gpr, pc, &mut out);
        out
    }
}

/// **读内存**的窄接口（条件里的变量值 / `[地址]` 要用）。
///
/// ⚠️ **只读一个 word**：值类型是 u32 ⇒ ⇒ 4 字节与文法一致（读半个 word
/// 的语法不存在，别为不存在的东西留接口）。
///
/// ⚠️ **失败给 `None`**：地址未映射 / 总线报错都算「读不到」⇒ ⇒ 上层把
/// 求值失败当**未命中**。
pub trait MemReader {
    /// 读一个 32 位 word。
    fn read_word(&self, addr: u32) -> Option<u32>;
}

/// **一个 word 也读不到**的读者（没有内存上下文的路径：表级单元测试、
/// 老端口 `set_breakpoint(addr, enabled)`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoMem;

impl MemReader for NoMem {
    fn read_word(&self, _addr: u32) -> Option<u32> {
        None
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
    /// `[地址]` / 变量名：**读该地址的那个 word**（档 3）。
    Load(AddressSpec),
    /// 取反。
    Not(Box<BoundExpr>),
    /// 二元运算。
    Bin(Op, Box<BoundExpr>, Box<BoundExpr>),
}

impl BoundExpr {
    fn eval(&self, gpr: &[u32; 32], pc: u32, mem: &dyn MemReader) -> Option<u32> {
        match self {
            Self::Const(value) => Some(*value),
            Self::Reg(index) => Some(gpr[*index as usize]),
            Self::Pc => Some(pc),
            Self::AddrOf(spec) => Some(resolve_address(spec, gpr, pc)),
            // ⚠️ **读不到 ⇒ 求值失败**（`?` 把 `None` 往上抛）。
            Self::Load(spec) => mem.read_word(resolve_address(spec, gpr, pc)),
            Self::Not(inner) => Some(u32::from(inner.eval(gpr, pc, mem)? == 0)),
            Self::Bin(op, left, right) => match op {
                // ⚠️ **短路**：左假 ⇒ 右**不求值**（右侧的内存读也不发生）。
                Op::And => {
                    if left.eval(gpr, pc, mem)? == 0 {
                        Some(0)
                    } else {
                        Some(u32::from(right.eval(gpr, pc, mem)? != 0))
                    }
                }
                // ⚠️ **短路**：左真 ⇒ 右**不求值**。
                Op::Or => {
                    if left.eval(gpr, pc, mem)? != 0 {
                        Some(1)
                    } else {
                        Some(u32::from(right.eval(gpr, pc, mem)? != 0))
                    }
                }
                Op::Eq => Some(u32::from(
                    left.eval(gpr, pc, mem)? == right.eval(gpr, pc, mem)?,
                )),
                Op::Ne => Some(u32::from(
                    left.eval(gpr, pc, mem)? != right.eval(gpr, pc, mem)?,
                )),
                // ⚠️ **无符号**比较（STAGE-42 §4.2）。
                Op::Lt => Some(u32::from(
                    left.eval(gpr, pc, mem)? < right.eval(gpr, pc, mem)?,
                )),
                Op::Le => Some(u32::from(
                    left.eval(gpr, pc, mem)? <= right.eval(gpr, pc, mem)?,
                )),
                Op::Gt => Some(u32::from(
                    left.eval(gpr, pc, mem)? > right.eval(gpr, pc, mem)?,
                )),
                Op::Ge => Some(u32::from(
                    left.eval(gpr, pc, mem)? >= right.eval(gpr, pc, mem)?,
                )),
            },
        }
    }

    /// **要不要读内存**（任一子式是 `Load` 即真）。
    fn reads_memory(&self) -> bool {
        match self {
            Self::Const(_) | Self::Reg(_) | Self::Pc => false,
            // `&[地址]` 只算地址 ⇒ ⇒ 不读。
            Self::AddrOf(_) => false,
            Self::Load(_) => true,
            Self::Not(inner) => inner.reads_memory(),
            Self::Bin(_, left, right) => left.reads_memory() || right.reads_memory(),
        }
    }

    /// **引用 pc 吗**（`pc` 子式，或地址基址是 `pc`）。
    fn reads_pc(&self) -> bool {
        match self {
            Self::Const(_) | Self::Reg(_) => false,
            Self::Pc => true,
            Self::AddrOf(spec) | Self::Load(spec) => spec.base == AddressBase::Pc,
            Self::Not(inner) => inner.reads_pc(),
            Self::Bin(_, left, right) => left.reads_pc() || right.reads_pc(),
        }
    }

    /// 收集**读内存**的地址区间（`(起始地址, 4)`）。
    fn collect_loads(&self, gpr: &[u32; 32], pc: u32, out: &mut Vec<(u32, u32)>) {
        match self {
            Self::Const(_) | Self::Reg(_) | Self::Pc | Self::AddrOf(_) => {}
            Self::Load(spec) => out.push((resolve_address(spec, gpr, pc), 4)),
            Self::Not(inner) => inner.collect_loads(gpr, pc, out),
            Self::Bin(_, left, right) => {
                left.collect_loads(gpr, pc, out);
                right.collect_loads(gpr, pc, out);
            }
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

/// **绑定**：把语法树里的名字换成「地址 / 寄存器位 / 帧偏移」。
fn bind_expr(
    expr: &Expr,
    names: &dyn NameResolver,
    scope: Option<u32>,
) -> Result<BoundExpr, CondError> {
    Ok(match expr {
        Expr::Const(value) => BoundExpr::Const(*value),
        Expr::Reg(index) => BoundExpr::Reg(*index),
        Expr::Pc => BoundExpr::Pc,
        Expr::Name(name) => {
            let reference = resolve_in(name, names, scope)?;
            match reference.class {
                // ⚠️ **函数名 ⇒ 地址常量**（函数没有「值」· 档 2）。
                NameClass::Function => BoundExpr::Const(need_addr(name, &reference)?),
                // ⚠️ **变量名 ⇒ 变量的值**（档 3 / 档 4）。
                //     ⚠️ 读的是 **word**：目前没有变量类型的宽度信息 ⇒ ⇒ 1 / 2
                //     字节的变量也按 4 字节读（与「值类型 = u32」的冻结口径一致；
                //     窄类型语义留到有 DWARF 类型信息时再谈）。
                NameClass::Object => BoundExpr::Load(AddressSpec {
                    base: AddressBase::Number(need_addr(name, &reference)?),
                    offset: 0,
                }),
                NameClass::Local => match reference.target {
                    // ⚠️ 在寄存器里 ⇒ ⇒ 名字就是**寄存器值**（不用读内存 ⇒
                    //    这条谓词在取指前就能求值）。
                    NameTarget::Local(LocalSlot::Reg(index)) => BoundExpr::Reg(index),
                    // ⚠️ 在栈上 ⇒ 地址 = `帧基址寄存器 + 偏移` ⇒ ⇒ 与 `[x1+8]`
                    //    形状完全一样 ⇒ 不用给 AST 加新变体。
                    NameTarget::Local(LocalSlot::Frame { base, offset }) => {
                        BoundExpr::Load(AddressSpec {
                            base: AddressBase::Register(base),
                            offset,
                        })
                    }
                    NameTarget::Addr(_) => {
                        return Err(CondError::new(format!(
                            "内部不一致：局部变量 \"{name}\" 被当成地址常量"
                        )))
                    }
                },
                // ⚠️ **没分类的符号（`STT_NOTYPE` 等）没有「值」的语义** ⇒ ⇒
                //     只许 `&名字`。
                NameClass::Other => {
                    return Err(CondError::new(format!(
                        "符号 \"{name}\" 不是已知的变量（它没有确定的值）；要它的地址请写 &{name}"
                    )))
                }
            }
        }
        // ⚠️ **取地址 ⇒ 不读内容**（档 2 / 批 D 的栈变量）。
        Expr::AddrOfName(name) => {
            let reference = resolve_in(name, names, scope)?;
            match reference.target {
                NameTarget::Addr(addr) => BoundExpr::Const(addr),
                // 栈上的变量**有地址**（帧基址 + 偏移）⇒ ⇒ 不读内容就是它。
                NameTarget::Local(LocalSlot::Frame { base, offset }) => {
                    BoundExpr::AddrOf(AddressSpec {
                        base: AddressBase::Register(base),
                        offset,
                    })
                }
                // ⚠️ 寄存器没有可寻址的地址（与批 B 的 `&a0` 同一条口径）。
                NameTarget::Local(LocalSlot::Reg(_)) => {
                    return Err(CondError::new(format!(
                        "寄存器里的局部变量 \"{name}\" 没有地址；要它的值请直接写 {name}"
                    )))
                }
            }
        }
        // ⚠️ **读内存**（`[地址]`）⇒ 档 3（批 C）：基址在绑定期定下来，
        //    真正的总线读在**求值期**（内存里的值随时会变）。
        Expr::Mem(spec) => BoundExpr::Load(bind_spec(spec, names, scope)?),
        // ⚠️ **取地址** ⇒ 地址表达式 ⇒ 基址里的符号也要在绑定期定下来。
        Expr::AddrOfMem(spec) => BoundExpr::AddrOf(bind_spec(spec, names, scope)?),
        Expr::Not(inner) => BoundExpr::Not(Box::new(bind_expr(inner, names, scope)?)),
        Expr::Bin(op, left, right) => BoundExpr::Bin(
            *op,
            Box::new(bind_expr(left, names, scope)?),
            Box::new(bind_expr(right, names, scope)?),
        ),
    })
}

/// 把地址表达式里的符号基址换成**真的基址**（数字 / 寄存器 / 帧偏移）。
fn bind_spec(
    spec: &AddressSpec,
    names: &dyn NameResolver,
    scope: Option<u32>,
) -> Result<AddressSpec, CondError> {
    let (base, extra) = match &spec.base {
        AddressBase::Number(n) => (AddressBase::Number(*n), 0),
        AddressBase::Register(r) => (AddressBase::Register(*r), 0),
        AddressBase::Pc => (AddressBase::Pc, 0),
        AddressBase::Symbol(name) => {
            let reference = resolve_in(name, names, scope).map_err(|_| {
                CondError::new(format!(
                    "未定义符号 \"{name}\"（地址表达式 {}）",
                    show_spec(spec)
                ))
            })?;
            match reference.target {
                NameTarget::Addr(addr) => (AddressBase::Number(addr), 0),
                // 栈上的局部变量：它的**地址**就是 `帧基址 + 偏移` ⇒ ⇒ 恰好
                // 是 `[x1+8]` 那种形状（偏移要与表达式里本来写的那个**相加**）。
                NameTarget::Local(LocalSlot::Frame { base, offset }) => {
                    (AddressBase::Register(base), offset)
                }
                // 寄存器里的局部变量：`[x]` 是「读 x 里那个指针指向的字」，
                // 与 `[a0]` 同一个语义 ⇒ ⇒ 基址就是那个寄存器。
                NameTarget::Local(LocalSlot::Reg(index)) => (AddressBase::Register(index), 0),
            }
        }
    };
    Ok(AddressSpec {
        base,
        offset: extra.wrapping_add(spec.offset),
    })
}

/// 按**帧上下文**解析一个名字，把失败原因翻成人话（`E_DBG-02`）。
fn resolve_in(
    name: &str,
    names: &dyn NameResolver,
    scope: Option<u32>,
) -> Result<NameRef, CondError> {
    names.resolve_at(name, scope).map_err(|error| match error {
        // ⚠️ **同名局部变量分不清** ⇒ ⇒ 告诉用户两条**可执行的出路**
        //    （换个带地址的断点 / 换个不重名的名字），而不是只说「未定义」。
        ResolveError::Ambiguous(functions) => CondError::new(format!(
            "局部变量 \"{name}\" 在多个函数里都有（{}）⇒ ⇒ 请在带地址的断点上用它，或换一个不重名的名字",
            functions.join(" / ")
        )),
        ResolveError::Undefined => undefined_name(name),
        // ⚠️ **没有调试信息**：不要报成「未定义符号」—— 用户会以为名字打错了。
        ResolveError::NoDebugInfo => CondError::new(format!(
            "\"{name}\" 不是已知的符号，这份固件也没有 DWARF 调试信息（局部变量名用不了）⇒ ⇒ 可改用寄存器或地址写法"
        )),
        // ⚠️ **有调试信息但当前帧没有**：点名函数，或说清「不在任何已知函数里」。
        ResolveError::NotInFrame(Some(function)) => CondError::new(format!(
            "\"{name}\" 不在 {function} 里（这个函数没有名为 {name} 的变量）⇒ ⇒ 换个变量名，或把断点放到定义了它的函数上"
        )),
        ResolveError::NotInFrame(None) => CondError::new(format!(
            "\"{name}\" 不在任何已知函数里（也不是全局符号）⇒ ⇒ 换个变量名，或把断点放到定义了它的函数上"
        )),
    })
}

/// 名字必须带地址时用（函数 / 全局变量）。
fn need_addr(name: &str, reference: &NameRef) -> Result<u32, CondError> {
    reference.addr().ok_or_else(|| {
        CondError::new(format!(
            "符号 \"{name}\" 没有地址（它随帧变化）；要它的值请直接写 {name}"
        ))
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
            //   本文件的 `addr` 模块解析。
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
    match addr::parse_address_syntax(&text) {
        Ok(spec) => Ok(spec),
        // ⚠️ 只搬**细节**（`AddrParseError` 自带「地址表达式无效：」前缀，而
        //    `CondError` 也会加「条件表达式无效：」⇒ ⇒ 直接搬 message 会出现
        //    两层前缀）。v0.137 顺手订正。
        Err(err) => Err(CondError::new(err.detail())),
    }
}

// ── 地址文法：`基址 ± 偏移`（条件断点的方括号内层）──────────────────────
//
// ⚠️ 这段原本独立成 `watch_expr.rs`（契约 v14 · 观察表达式）。但其中「文本
// → 地址」这一半**只有条件文法在用**（`[s0-24]` / `[main+4]` 的方括号内层）
// ⇒ ⇒ 单独开一个模块会让「谁拥有这份文法」含糊 ⇒ ⇒ v0.137 并回本文件，
// 地址解析**只有一个落点**。
//
// ⚠️ v0.138 起观察表达式**整块下线**（`watch_expr.rs` 已删，契约 v18 撤掉
//   那 3 条命令 + 1 条事件 + 2 个 DTO）⇒ ⇒ 这份文法的唯一用户就是条件文法，
//   「带名字解析」的那两条入口也随之删除（`Symbol` 变体留给绑定期的条件文法用）。
pub(crate) mod addr {
    //! 地址文法：把「基址 ± 偏移」的**文本**变成 [`AddressSpec`]。
    //!
    //! ⚠️ 类型写 `pub`、模块本身 `pub(crate)` ⇒ ⇒ 实际可见性仍限 crate 内。
    //!
    //! ⚠️ 只做**形状**，不查任何表：符号名原样留成 [`AddressBase::Symbol`]，
    //!    由条件文法在**绑定期**换成数字（它要按断点 pc 所在的函数选局部变量）。

    use std::fmt;

    /// 地址文法的**解析失败**。
    ///
    /// ⚠️ 前缀只说「地址表达式」：同一段文法被条件文法与地址表达式共用，
    ///    前缀描述的是**这一段**，不冒充调用方的整条表达式。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AddrParseError {
        /// 不含前缀的细节（给**自带前缀**的调用方包消息用）。
        detail: String,
    }

    impl AddrParseError {
        /// 造一个错误（本模块内唯一构造入口）。
        pub(crate) fn new(detail: impl fmt::Display) -> Self {
            Self {
                detail: format!("{detail}"),
            }
        }

        /// 只有细节（不含前缀）—— `CondError` 自带「条件表达式无效：」，
        /// 直接搬完整消息会拼出两层前缀。
        pub(crate) fn detail(&self) -> &str {
            &self.detail
        }
    }

    impl fmt::Display for AddrParseError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            // ⚠️ 前缀只说「地址表达式」：这段文法描述的是**一段**，
            //    不冒充调用方的整条表达式。
            write!(f, "地址表达式无效：{}", self.detail)
        }
    }

    impl std::error::Error for AddrParseError {}

    /// 地址基址。
    ///
    /// ⚠️ **不是 `Copy`** —— 为了 [`AddressBase::Symbol`] 那个名字（`String`）。
    /// 这是有意的：符号基址**只在解析期**存在一瞬（[`parse_address_with`] 会
    /// 把它换成 [`AddressBase::Number`]），但文法层要先表达得出来。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum AddressBase {
        /// 立即数地址。
        Number(u32),
        /// 通用寄存器。
        Register(u8),
        /// 当前 PC。
        Pc,
        /// **符号名**（`@main+4` / `[g_counter+2]`）：解析期换成
        /// [`AddressBase::Number`]。
        Symbol(String),
    }

    /// 内存地址 = 基址 + 有符号偏移。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AddressSpec {
        /// 基址。
        pub base: AddressBase,
        /// 偏移（可正可负）。
        pub offset: i32,
    }

    /// 解析「基址 ± 偏移」的**语法**：符号名**保留**成
    /// [`AddressBase::Symbol`]，不查任何表。
    ///
    /// ⚠️ 条件文法用它（`[main+4]` 的方括号内层）—— 名字要等到**绑定**期才
    /// 定得下来（同一名字在不同函数里可能是不同的局部变量）。
    pub(crate) fn parse_address_syntax(text: &str) -> Result<AddressSpec, AddrParseError> {
        let (base_text, offset) = split_address(text)?;
        let base = parse_base_syntax(base_text)?;
        Ok(AddressSpec { base, offset })
    }

    /// 把「基址 ± 偏移」拆成两段。
    fn split_address(text: &str) -> Result<(&str, i32), AddrParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AddrParseError::new("内存表达式缺少地址"));
        }
        let split = text
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '+' || *c == '-');
        match split {
            Some((i, _)) => {
                let base = &text[..i];
                let off = &text[i..];
                let offset = off
                    .parse::<i32>()
                    .map_err(|_| AddrParseError::new(format!("偏移不是整数：{off}")))?;
                Ok((base, offset))
            }
            None => Ok((text, 0)),
        }
    }

    /// 解析一个基址记号，**符号名原样留住**。
    fn parse_base_syntax(text: &str) -> Result<AddressBase, AddrParseError> {
        let t = text.trim().trim_start_matches('$');
        if t.is_empty() {
            return Err(AddrParseError::new("缺少基址"));
        }
        if t.eq_ignore_ascii_case("pc") {
            return Ok(AddressBase::Pc);
        }
        if let Some(n) = parse_number(t) {
            return Ok(AddressBase::Number(n));
        }
        if let Some(r) = parse_register(t) {
            return Ok(AddressBase::Register(r));
        }
        // ⚠️ 剩下的当**名字**（它到底是不是符号、是函数还是变量，等绑定/解析
        //   那一层定）⇒ ⇒ 这里只做形状判断：标识符形状才收。
        let shape_ok = t
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
            && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if shape_ok {
            return Ok(AddressBase::Symbol(t.to_string()));
        }
        Err(AddrParseError::new(format!("未知寄存器名或地址 \"{t}\"")))
    }

    fn parse_number(text: &str) -> Option<u32> {
        let t = text.trim();
        if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            return u32::from_str_radix(hex, 16).ok();
        }
        t.parse::<u32>().ok()
    }

    fn parse_register(text: &str) -> Option<u8> {
        let t = text.trim().to_ascii_lowercase();
        if let Some(num) = t.strip_prefix('x') {
            if let Ok(n) = num.parse::<u8>() {
                return (n < 32).then_some(n);
            }
        }
        let idx = match t.as_str() {
            "zero" => 0,
            "ra" => 1,
            "sp" => 2,
            "gp" => 3,
            "tp" => 4,
            "t0" => 5,
            "t1" => 6,
            "t2" => 7,
            "s0" | "fp" => 8,
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
        };
        Some(idx)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// **语法期只认形状**：符号名原样留着（不查表），偏移带符号。
        #[test]
        fn the_syntax_keeps_symbol_names_for_the_binder() {
            assert_eq!(
                parse_address_syntax("main+4").expect("合法形状"),
                AddressSpec {
                    base: AddressBase::Symbol("main".to_string()),
                    offset: 4,
                }
            );
            assert_eq!(
                parse_address_syntax("x2-8").expect("合法形状"),
                AddressSpec {
                    base: AddressBase::Register(2),
                    offset: -8,
                }
            );
            let err = parse_address_syntax("").expect_err("空地址必须拒");
            let message = err.to_string();
            assert!(
                message.contains("缺少地址"),
                "错误消息要说清缺什么：{message}"
            );
            assert!(
                message.starts_with("地址表达式无效："),
                "前缀只说这一段：{message}"
            );
        }
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
            .eval(gpr, pc, &NoMem)
            .expect("求值不该失败（这些用例都不读内存）")
    }

    /// 带**一张小符号表**的解析 + 绑定 + 求值。
    fn eval_with(expr: &str, names: &dyn NameResolver, gpr: &[u32; 32], pc: u32) -> u32 {
        parse(expr)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能解析：{e}"))
            .bind(names)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能绑定：{e}"))
            .eval(gpr, pc, &NoMem)
            .expect("求值不该失败（这些用例都不读内存）")
    }

    /// **假内存**：`addr → word`（表外地址 ⇒ 读不到 ⇒ 求值失败）。
    struct FakeMem(Vec<(u32, u32)>);

    impl MemReader for FakeMem {
        fn read_word(&self, addr: u32) -> Option<u32> {
            self.0
                .iter()
                .find(|(candidate, _)| *candidate == addr)
                .map(|(_, value)| *value)
        }
    }

    /// 带**符号表 + 内存**的解析 + 绑定 + 求值。
    fn eval_mem(
        expr: &str,
        names: &dyn NameResolver,
        gpr: &[u32; 32],
        pc: u32,
        mem: &dyn MemReader,
    ) -> Option<u32> {
        parse(expr)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能解析：{e}"))
            .bind(names)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能绑定：{e}"))
            .eval(gpr, pc, mem)
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
                    target: crate::m08_breakpoint::names::NameTarget::Addr(*addr),
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

    /// **变量名 = 变量的值**（档 3）：读它地址处的那个 word。
    #[test]
    fn a_variable_name_is_its_value_in_memory() {
        // `g_counter` 在 0x2000，那里存着 7。
        let mem = FakeMem(vec![(0x2000, 7)]);
        assert_eq!(
            eval_mem("g_counter == 7", &names(), &gpr(&[]), 0, &mem),
            Some(1)
        );
        assert_eq!(
            eval_mem("g_counter == 8", &names(), &gpr(&[]), 0, &mem),
            Some(0)
        );
        // ⚠️ **值 ≠ 地址**：`g_counter` 读到的是 7，不是 0x2000。
        assert_eq!(
            eval_mem("g_counter == 0x2000", &names(), &gpr(&[]), 0, &mem),
            Some(0),
            "变量名是变量的值，不是它的地址"
        );
    }

    /// **`[地址]` 读内存**（档 3）：读该地址处的那个 word。
    #[test]
    fn a_bracket_reads_the_word_at_that_address() {
        let mem = FakeMem(vec![(0x1008, 5), (0x2000, 9)]);
        assert_eq!(
            eval_mem("[x1+8] == 5", &names(), &gpr(&[(1, 0x1000)]), 0, &mem),
            Some(1)
        );
        // 符号基址一样能读。
        assert_eq!(
            eval_mem("[g_counter] == 9", &names(), &gpr(&[]), 0, &mem),
            Some(1)
        );
        // `&[...]` 仍然**只算地址**（不读 ⇒ 这张表里没有 0x1009 也不影响）。
        assert_eq!(
            eval_mem("&[x1+8] == 0x1008", &names(), &gpr(&[(1, 0x1000)]), 0, &mem),
            Some(1)
        );
    }

    /// **读不到 ⇒ 求值失败**（`None`），不是「为假」。
    #[test]
    fn an_unreadable_address_is_an_evaluation_failure() {
        let empty = FakeMem(vec![]);
        assert_eq!(
            eval_mem("g_counter == 0", &names(), &gpr(&[]), 0, &empty),
            None,
            "读不到内存表达式就求不出值 ⇒ 不能折成 0"
        );
        // 用 `NoMem` 也一样（它就是「一个 word 也读不到」）。
        assert_eq!(
            eval_mem("g_counter == 0", &names(), &gpr(&[]), 0, &NoMem),
            None
        );
        // 不含内存读的表达式用 `NoMem` 照常求值（它根本不碰内存）。
        assert_eq!(
            eval_mem("main == 0x1000", &names(), &gpr(&[]), 0, &NoMem),
            Some(1)
        );
    }

    /// **短路**：右操作数里的内存读**不会发生** ⇒ ⇒ 左假时读不到也不影响
    /// 结果（少一次白读，也少一次「求值失败」）。
    #[test]
    fn a_short_circuited_memory_read_never_happens() {
        let empty = FakeMem(vec![]);
        // `x2 == 0` 为真 ⇒ `||` 右**不求值** ⇒ 读不到也没关系。
        assert_eq!(
            eval_mem(
                "x2 == 0 || [x1] == 1",
                &names(),
                &gpr(&[(1, 0x1000), (2, 0)]),
                0,
                &empty
            ),
            Some(1)
        );
        // `x2 != 0` 为假 ⇒ `&&` 右**不求值** ⇒ 同上。
        assert_eq!(
            eval_mem(
                "x2 != 0 && [x1] == 1",
                &names(),
                &gpr(&[(1, 0x1000), (2, 0)]),
                0,
                &empty
            ),
            Some(0)
        );
        // 反过来（右真的要被求值）⇒ 读不到 ⇒ 求值失败。
        assert_eq!(
            eval_mem(
                "x2 == 0 || [x1] == 1",
                &names(),
                &gpr(&[(1, 0x1000), (2, 1)]),
                0,
                &empty
            ),
            None
        );
    }

    /// **`reads_memory` / `reads_pc` / `load_intervals`**（表级判定要用）。
    #[test]
    fn the_shape_predicates_describe_the_bound_tree() {
        let names = names();
        // 只读寄存器 ⇒ 不读内存、不碰 pc。
        let cond = parse("x1 == 3").expect("语法").bind(&names).expect("绑定");
        assert!(!cond.reads_memory());
        assert!(!cond.reads_pc());
        assert!(cond.load_intervals(&gpr(&[(1, 0x1000)]), 0).is_empty());

        // 变量值 ⇒ 读内存，区间 = 变量地址处的 4 字节。
        let cond = parse("g_counter == 3")
            .expect("语法")
            .bind(&names)
            .expect("绑定");
        assert!(cond.reads_memory());
        assert!(!cond.reads_pc());
        assert_eq!(cond.load_intervals(&gpr(&[]), 0), vec![(0x2000, 4)]);

        // `&g_counter` 只算地址 ⇒ 不读。
        let cond = parse("&g_counter == 0x2000")
            .expect("语法")
            .bind(&names)
            .expect("绑定");
        assert!(!cond.reads_memory());

        // 引用 pc ⇒ 不许做区间过滤。
        let cond = parse("pc == 0x1000")
            .expect("语法")
            .bind(&names)
            .expect("绑定");
        assert!(cond.reads_pc());

        // 地址基址是 pc 的读也算「引用 pc」。
        let cond = parse("[pc] == 1")
            .expect("语法")
            .bind(&names)
            .expect("绑定");
        assert!(cond.reads_memory());
        assert!(cond.reads_pc());

        // 两个读 ⇒ 两条区间（按遍历顺序）。
        let cond = parse("[x1] == 1 && [x2] == 2")
            .expect("语法")
            .bind(&names)
            .expect("绑定");
        assert_eq!(
            cond.load_intervals(&gpr(&[(1, 0x1000), (2, 0x2000)]), 0),
            vec![(0x1000, 4), (0x2000, 4)]
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

    /// **裸数字仍是字面常量**（读内存必须显式写 `[…]` / `@…`）。
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
        let a = bound.eval(&gpr(&[]), 0, &NoMem);
        let b = cond
            .bind(&names())
            .expect("能绑定")
            .eval(&gpr(&[]), 0, &NoMem);
        assert_eq!(a, b, "绑定后求值不该再依赖符号表");
        assert_eq!(a, Some(0), "0x1000 == 0x2000 为假");
    }

    // ================= 批 D · 局部变量（档 4） =================

    /// 一条**局部变量**记录：`(名字, 所在函数名, [low, high), 位置)`。
    /// 夹具里「栈帧基址寄存器」用的就是 `x8`（S0）——
    /// ⚠️ 这只是**测试数据的喂法**：生产路径上的基址由 `.debug_frame` 算。
    const FRAME_POINTER: u8 = 8;

    type LocalRec = (&'static str, &'static str, u32, u32, LocalSlot);

    /// 一张**带帧**的假符号表：全局符号 + 若干函数里的局部变量。
    struct FrameNames {
        globals: Vec<(&'static str, NameClass, u32)>,
        locals: Vec<LocalRec>,
    }

    impl FrameNames {
        /// 真工具链夹具那份形状：`main` 在 `0x38..0x78`，`helper` 在 `0x0..0x38`。
        fn fixture() -> Self {
            Self {
                globals: vec![("g_counter", NameClass::Object, 0x1078)],
                locals: vec![
                    // helper: `int a`（形参，fbreg -36）、`int t`（fbreg -20）
                    (
                        "a",
                        "helper",
                        0x00,
                        0x38,
                        LocalSlot::Frame {
                            base: FRAME_POINTER,
                            offset: -36,
                        },
                    ),
                    (
                        "t",
                        "helper",
                        0x00,
                        0x38,
                        LocalSlot::Frame {
                            base: FRAME_POINTER,
                            offset: -20,
                        },
                    ),
                    // main: `int x`（fbreg -24）、`int y`（fbreg -20）
                    (
                        "x",
                        "main",
                        0x38,
                        0x78,
                        LocalSlot::Frame {
                            base: FRAME_POINTER,
                            offset: -24,
                        },
                    ),
                    (
                        "y",
                        "main",
                        0x38,
                        0x78,
                        LocalSlot::Frame {
                            base: FRAME_POINTER,
                            offset: -20,
                        },
                    ),
                ],
            }
        }

        /// 加一个**寄存器位置**的局部变量（两个函数里同名，位置不同）。
        fn with_register_locals(mut self) -> Self {
            self.locals
                .push(("counter", "helper", 0x00, 0x38, LocalSlot::Reg(11)));
            self.locals
                .push(("counter", "main", 0x38, 0x78, LocalSlot::Reg(12)));
            self
        }
    }

    impl NameResolver for FrameNames {
        fn resolve(&self, name: &str) -> Option<NameRef> {
            self.globals
                .iter()
                .find(|(candidate, _, _)| *candidate == name)
                .map(|(_, class, addr)| NameRef {
                    class: *class,
                    target: NameTarget::Addr(*addr),
                })
        }

        fn resolve_at(&self, name: &str, pc: Option<u32>) -> Result<NameRef, ResolveError> {
            let hits: Vec<&LocalRec> = self
                .locals
                .iter()
                .filter(|(candidate, ..)| *candidate == name)
                .collect();
            if hits.is_empty() {
                return self.resolve(name).ok_or(ResolveError::Undefined);
            }
            // 1. 有帧上下文 ⇒ 先挑**当前 pc 所在函数**里的那一个。
            if let Some(pc) = pc {
                if let Some(hit) = hits
                    .iter()
                    .find(|(_, _, low, high, _)| (*low..*high).contains(&pc))
                {
                    return Ok(NameRef {
                        class: NameClass::Local,
                        target: NameTarget::Local(hit.4),
                    });
                }
            }
            // 2. 没有帧上下文（或 pc 不落在任何函数里）⇒ 只凭名字：
            //    位置相同的（不同函数同形栈槽）可以合并，不同才算歧义。
            let mut distinct: Vec<(&str, LocalSlot)> = Vec::new();
            for hit in &hits {
                if !distinct.iter().any(|(_, slot)| *slot == hit.4) {
                    distinct.push((hit.1, hit.4));
                }
            }
            if distinct.len() == 1 {
                return Ok(NameRef {
                    class: NameClass::Local,
                    target: NameTarget::Local(distinct[0].1),
                });
            }
            Err(ResolveError::Ambiguous(
                distinct
                    .iter()
                    .map(|(function, _)| (*function).to_string())
                    .collect(),
            ))
        }
    }

    /// 解析 + **按帧绑定** + 求值（带内存）。
    fn eval_scoped(
        expr: &str,
        names: &dyn NameResolver,
        scope: Option<u32>,
        gpr: &[u32; 32],
        mem: &dyn MemReader,
    ) -> u32 {
        parse(expr)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能解析：{e}"))
            .bind_at(names, scope)
            .unwrap_or_else(|e| panic!("表达式 {expr:?} 应当能绑定：{e}"))
            .eval(gpr, scope.unwrap_or(0), mem)
            .expect("求值不该失败")
    }

    /// **栈上的局部变量 ⇒ `Load(帧基址 + 偏移)`**：不用给 AST 加新变体。
    #[test]
    fn a_stack_local_binds_to_the_frame_register_plus_offset() {
        let names = FrameNames::fixture();
        let cond = parse("x == 42")
            .expect("语法合法")
            .bind_at(&names, Some(0x38))
            .expect("带地址的断点上局部变量应当能绑定");
        // ⚠️ 栈上变量**要读内存** ⇒ ⇒ 它落在 store 边界求值那一档。
        assert!(cond.reads_memory(), "栈上的局部变量读内存");
        // s0 = 0x2000 ⇒ `x` 在 0x2000 - 24 = 0x1fe8。
        let mem = FakeMem(vec![(0x1fe8, 42)]);
        assert_eq!(
            cond.eval(&gpr(&[(FRAME_POINTER as usize, 0x2000)]), 0x38, &mem),
            Some(1),
            "x == 42 应为真"
        );
        let mem = FakeMem(vec![(0x1fe8, 41)]);
        assert_eq!(
            cond.eval(&gpr(&[(FRAME_POINTER as usize, 0x2000)]), 0x38, &mem),
            Some(0),
            "x == 41 应为假"
        );
    }

    /// **寄存器里的局部变量 ⇒ 寄存器值**：取指前就能求值，不读内存。
    #[test]
    fn a_register_local_binds_to_that_register() {
        let names = FrameNames::fixture().with_register_locals();
        let cond = parse("counter >= 3")
            .expect("语法合法")
            .bind_at(&names, Some(0x40))
            .expect("能绑定");
        assert!(!cond.reads_memory(), "寄存器位置不读内存");
        assert_eq!(cond.eval(&gpr(&[(12, 3)]), 0x40, &NoMem), Some(1));
        assert_eq!(cond.eval(&gpr(&[(12, 2)]), 0x40, &NoMem), Some(0));
    }

    /// **帧上下文挑对函数**：同名局部变量按断点地址落在哪个函数来解。
    #[test]
    fn the_frame_context_selects_the_function_that_contains_the_pc() {
        let names = FrameNames::fixture().with_register_locals();
        // pc 在 helper ⇒ counter 是 x11
        let in_helper = parse("counter == 1")
            .expect("语法合法")
            .bind_at(&names, Some(0x10))
            .expect("helper 里能绑定");
        assert_eq!(
            in_helper.eval(&gpr(&[(11, 1), (12, 0)]), 0x10, &NoMem),
            Some(1)
        );
        // pc 在 main ⇒ counter 是 x12
        let in_main = parse("counter == 1")
            .expect("语法合法")
            .bind_at(&names, Some(0x40))
            .expect("main 里能绑定");
        assert_eq!(
            in_main.eval(&gpr(&[(11, 0), (12, 1)]), 0x40, &NoMem),
            Some(1)
        );
    }

    /// **没有帧上下文 + 同名不同位置 ⇒ 歧义**，且错误消息给出两条出路。
    #[test]
    fn a_shared_local_name_without_a_frame_is_ambiguous() {
        let names = FrameNames::fixture().with_register_locals();
        let err = parse("counter == 1")
            .expect("语法合法")
            .bind_at(&names, None)
            .expect_err("不同位置的同名局部变量不许凭名字绑定");
        assert!(
            err.message.contains("在多个函数里都有")
                && err.message.contains("main")
                && err.message.contains("helper")
                && err.message.contains("带地址的断点"),
            "错误消息要说明歧义并给出出路：{}",
            err.message
        );
    }

    /// **没有帧上下文 + 位置相同** ⇒ 直接可解（不同函数同一栈槽不是歧义）。
    #[test]
    fn an_identical_slot_across_functions_is_not_ambiguous() {
        let names = FrameNames::fixture();
        // `y` 在 main 是 fbreg -20，`t` 在 helper 是 fbreg -20 —— 但名字不同，
        // 这里用 `y` 自己：只有 main 有 ⇒ 唯一。
        assert_eq!(
            eval_scoped(
                "y == 7",
                &names,
                Some(0x40),
                &gpr(&[(8, 0x1000)]),
                &FakeMem(vec![(0x0fec, 7)])
            ),
            1
        );
        // 同名同位置的 `y` 若在另一个函数也出现 ⇒ 合并成一条，不报歧义。
        let mut merged = FrameNames::fixture();
        merged.locals.push((
            "y",
            "helper",
            0x00,
            0x38,
            LocalSlot::Frame {
                base: FRAME_POINTER,
                offset: -20,
            },
        ));
        parse("y == 7")
            .expect("语法合法")
            .bind_at(&merged, None)
            .expect("同位置同名不歧义");
    }

    /// **栈上局部的地址**（`&y`）⇒ 帧基址 + 偏移。
    #[test]
    fn taking_the_address_of_a_stack_local_yields_the_frame_address() {
        let names = FrameNames::fixture();
        assert_eq!(
            eval_scoped(
                "&y == 0x0fec",
                &names,
                Some(0x40),
                &gpr(&[(8, 0x1000)]),
                &NoMem
            ),
            1,
            "s0 = 0x1000 ⇒ &y = 0x1000 - 20"
        );
    }

    /// **寄存器里的局部变量没有地址** ⇒ `&name` 必须被拒。
    #[test]
    fn taking_the_address_of_a_register_local_is_rejected() {
        let names = FrameNames::fixture().with_register_locals();
        let err = parse("&counter == 0")
            .expect("语法合法")
            .bind_at(&names, Some(0x40))
            .expect_err("寄存器位置没有地址");
        assert!(
            err.message.contains("没有地址") && err.message.contains("直接写 counter"),
            "错误消息要说清并给直接写法：{}",
            err.message
        );
    }

    /// 一张**固定返回某个错误**的解析器（钉三态文案用）。
    struct ErrorNames(ResolveError);

    impl NameResolver for ErrorNames {
        fn resolve(&self, _name: &str) -> Option<NameRef> {
            None
        }

        fn resolve_at(&self, _name: &str, _pc: Option<u32>) -> Result<NameRef, ResolveError> {
            Err(self.0.clone())
        }
    }

    /// **三态文案**（收口批 · 轻档）：每种「查不到」都要给**不同的下一步**。
    #[test]
    fn each_resolve_failure_gets_its_own_advice() {
        // ① 没有调试信息 ⇒ 换写法（寄存器 / 地址），不是「名字打错」
        let err = parse("x == 1")
            .expect("语法合法")
            .bind_at(&ErrorNames(ResolveError::NoDebugInfo), None)
            .expect_err("必须被拒");
        assert!(
            err.message.contains("DWARF") && err.message.contains("寄存器或地址"),
            "无 DWARF 要说清原因与替代写法：{}",
            err.message
        );
        assert!(
            !err.message.contains("未定义符号"),
            "无 DWARF 不许说成「未定义符号」：{}",
            err.message
        );
        // ② 名字在、但不在当前帧 ⇒ 点名函数 + 换地址
        let err = parse("x == 1")
            .expect("语法合法")
            .bind_at(
                &ErrorNames(ResolveError::NotInFrame(Some("helper".to_string()))),
                Some(0x10),
            )
            .expect_err("必须被拒");
        assert!(
            err.message.contains("不在 helper 里") && err.message.contains("换个"),
            "要点名函数并给出下一步：{}",
            err.message
        );
        // ③ 有调试信息但地址不在任何函数里 ⇒ 不点名
        let err = parse("x == 1")
            .expect("语法合法")
            .bind_at(&ErrorNames(ResolveError::NotInFrame(None)), Some(0x9000))
            .expect_err("必须被拒");
        assert!(
            err.message.contains("不在任何已知函数里"),
            "不知道函数时不许编一个名字出来：{}",
            err.message
        );
    }

    /// **地址文法里的局部变量基址**（`[x+4]`）：帧基址 + (偏移 + 写的偏移)。
    #[test]
    fn a_stack_local_base_in_a_bracket_expression_uses_the_frame_register() {
        let names = FrameNames::fixture();
        // `[x+4]` ⇒ base = s0、offset = -24 + 4 = -20 ⇒ 0x1000 - 20 = 0x0fec。
        assert_eq!(
            eval_scoped(
                "[x+4] == 9",
                &names,
                Some(0x40),
                &gpr(&[(8, 0x1000)]),
                &FakeMem(vec![(0x0fec, 9)])
            ),
            1
        );
        // 负偏移要真的**相加**（不是覆盖）：`[x-4]` ⇒ -24 - 4 = -28。
        assert_eq!(
            eval_scoped(
                "[x-4] == 1",
                &names,
                Some(0x40),
                &gpr(&[(8, 0x1000)]),
                &FakeMem(vec![(0x0fe4, 1)])
            ),
            1
        );
    }

    /// **局部变量遮住同名全局变量**（解析顺序第 2 条先于第 3 条）。
    #[test]
    fn a_local_shadows_a_global_of_the_same_name() {
        let mut names = FrameNames::fixture();
        names.globals.push(("x", NameClass::Object, 0x3000));
        // 有帧 ⇒ 取局部（s0 - 24 = 0x0fe8）
        assert_eq!(
            eval_scoped(
                "x == 42",
                &names,
                Some(0x40),
                &gpr(&[(8, 0x1000)]),
                &FakeMem(vec![(0x0fe8, 42)])
            ),
            1,
            "局部优先"
        );
        // 没有帧 ⇒ **仍然**是局部（唯一名，解析顺序第 2 条先于第 3 条）⇒
        //   ⇒ 全局 `x` 不会被「借」来当局部变量的替身。
        assert_eq!(
            eval_scoped(
                "x == 42",
                &names,
                None,
                &gpr(&[(8, 0x1000)]),
                &FakeMem(vec![(0x0fe8, 42)])
            ),
            1,
            "没有帧上下文时也先认局部（同名全局不会被借来用）"
        );
        // 没有同名局部时才走全局（`g_counter` 读 0x1078）。
        assert_eq!(
            eval_scoped(
                "g_counter == 7",
                &names,
                Some(0x40),
                &gpr(&[]),
                &FakeMem(vec![(0x1078, 7)])
            ),
            1,
            "没有同名局部 ⇒ 全局符号"
        );
    }

    /// **pc 不落在任何函数区间里** ⇒ 退化成「只凭名字」（唯一名仍可解）。
    #[test]
    fn a_pc_outside_every_function_falls_back_to_the_bare_name() {
        let names = FrameNames::fixture();
        // 0x0100 不在 0x00..0x38 / 0x38..0x78 里 ⇒ ⇒ 只凭名字，`x` 唯一。
        let cond = parse("x == 42")
            .expect("语法合法")
            .bind_at(&names, Some(0x0100))
            .expect("唯一名仍可解");
        assert_eq!(
            cond.eval(&gpr(&[(8, 0x1000)]), 0x0100, &FakeMem(vec![(0x0fe8, 42)])),
            Some(1)
        );
    }
}
