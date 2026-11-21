//! console / UART 方向：TX 捕获环取字节（backlog 攒取 / 订阅事件）与 RX FIFO 注入。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。

use super::{KernelHost, CONSOLE_BACKLOG_MAX, TOPIC_CONSOLE};
use crate::m15_peripheral::uart::Uart16550;
use riscv_sim_contract::protocol::EventPayload;
use std::cell::RefCell;
use std::rc::Rc;

impl KernelHost {
    /// 取走 UART0 **已经发出**的字节（console 输出）。
    ///
    /// ⚠️ **这是「guest 写进捕获环」的字节**，不是「平台写出去的字节」——
    /// 平台的 UART 是**捕获环**模型（Lane B），没有真正的串口线。
    ///
    /// ⚠️ **取走即清空**：调用方拿走的就是全部，顺序不变、不丢不重。
    ///
    /// ⚠️ **它先 `collect_console()` 再取 backlog** —— 因为 UART 的字节
    /// 可能已经被 [`Self::collect_console`] 收进 backlog 了（那是「及时
    /// 取走、免得捕获环满了丢字节」的要求）。只读 UART 的话会拿到空。
    pub fn drain_console(&mut self) -> Vec<u8> {
        self.collect_console();
        std::mem::take(&mut self.console_backlog)
    }

    /// **console 角色**的 UART 句柄（**第一个** UART —— 裁决 D：逐字不变）。
    ///
    /// ⚠️ **角色 = id，句柄 = 从实例表里查出来的**（裁决 B）⇒ ⇒ 表是
    /// **唯一真相源** ⇒ ⇒ 「console 是哪个」这件事**只由角色 id 决定**。
    fn console_uart(&self) -> Option<Rc<RefCell<Uart16550>>> {
        // ⚠️ **console 角色**（STAGE-49 · D-177 的 C）：描述里声明了**它**
        // ⇒ ⇒ 就是它；没声明 ⇒ ⇒ **第一个 UART**（今天的口径，逐字不变）。
        //
        // ⚠️ ⚠️ **只走 `resolve_roles` 那一个落点**（v0.73 施工期订正）：这里
        //   **原来自己**又做了一遍「缺省 ⇒ 第一个 UART」的回落 ⇒ ⇒ ⇒
        //   「缺省行为不变」这条性质被**两道**护栏保证（`resolve_roles` 一道
        //   + 这里一道）⇒ ⇒ ⇒ **结构上不可证伪** ⇒ ⇒ ⇒ 变异 T-4（「roles
        //   缺省时**不**回落」）**实测零红**（⇒ ⇒ 零判别力）⇒ ⇒ ⇒
        //   ⇒ 现在这里**不**再自己回落 ⇒ ⇒ ⇒ 「回落」这条**只有一处** ⇒ ⇒
        //   ⇒ 「角色解析」这条约束**只有一个落点**（一个约束一个落点）。
        let id = self.roles.console.as_deref()?;
        self.devices.uart(id)
    }

    /// **直接从 UART 的捕获环**取走字节（**不碰** backlog）。
    ///
    /// ⚠️ **为什么必须单独一个函数**：`collect_console`（收进 backlog）与
    /// `drain_console`（收走 backlog）都需要「从 UART 取字节」这一步。
    /// 如果两者都直接调 `drain_console`，而 `drain_console` 又要
    /// `collect_console` ⇒ **无限递归 ⇒ 栈溢出**。
    ///
    /// ⚠️ 这个坑是**真的踩过**的：接线用例写完、跑绿之后才把
    /// `drain_console` 改成「先 collect 再取 backlog」，release 构建当场
    /// 栈溢出（debug 构建的栈帧更大，反而先崩在别处）。
    fn drain_uart(&mut self) -> Vec<u8> {
        let Some(uart) = self.console_uart() else {
            return Vec::new();
        };
        // ⚠️ 绑到局部变量：`match` 的临时值会活到块尾，而 `uart` 在那
        // 之前就被 drop 了 ⇒ 借用检查器不通过。
        let out = match uart.try_borrow_mut() {
            Ok(mut u) => u.drain_tx(),
            // ⚠️ 争用 ⇒ **不 panic**，返回空并让调用方下一轮再取。
            Err(_) => Vec::new(),
        };
        out
    }

    /// console 方向**已经发出**但还没被取走的字节数。
    pub fn console_pending(&self) -> usize {
        let Some(uart) = self.console_uart() else {
            return 0;
        };
        let n = match uart.try_borrow() {
            Ok(u) => u.tx_pending(),
            Err(_) => 0,
        };
        n
    }

    /// 把 UART 的 TX 捕获环**取空**，攒进宿主的 backlog。
    ///
    /// ⚠️ **必须**及时取走：捕获环（4 KiB）满了会**丢最新的**字节 ⇒
    /// 不取走就是静默丢 console 内容。
    pub fn collect_console(&mut self) {
        let chunk = self.drain_uart();
        if chunk.is_empty() {
            return;
        }
        self.console_backlog.extend_from_slice(&chunk);
        if self.console_backlog.len() > CONSOLE_BACKLOG_MAX {
            // 丢**最旧**的（最近发生的事更有用）。
            let excess = self.console_backlog.len() - CONSOLE_BACKLOG_MAX;
            self.console_backlog.drain(0..excess);
        }
    }

    /// 造一个 `Console` 事件（**订阅了** console topic 才有）。
    ///
    /// ⚠️ **没订阅就返回 `None`** —— console 是**流**，不是命令的直接
    /// 回复；没订阅就不推（与波形事件同一条口径）。
    pub fn console_event(&mut self) -> Option<EventPayload> {
        if !self.topics.contains(TOPIC_CONSOLE) {
            return None;
        }
        // ⚠️ 先把 UART 里**还没收走**的字节收进 backlog —— 否则订阅的
        // 那一刻正好赶在「跑完那一批」与「收走」之间，就会漏掉。
        self.collect_console();
        if self.console_backlog.is_empty() {
            return None;
        }
        let bytes = std::mem::take(&mut self.console_backlog);
        Some(EventPayload::Console { bytes })
    }

    /// 往 UART0 的**接收** FIFO 塞一个字节（宿主侧的 console 输入）。
    ///
    /// ⚠️ 平台的 UART 是**捕获环**模型：TX 方向是「guest 写进来、
    /// 宿主取走」；RX 方向是「宿主塞进去、guest 取走」。
    pub fn uart_receive(&mut self, byte: u8) {
        let Some(uart) = self.console_uart() else {
            return;
        };
        let done = match uart.try_borrow_mut() {
            Ok(mut u) => {
                u.receive(byte);
                true
            }
            Err(_) => false,
        };
        let _ = done;
    }

    /// 往**指定 id 的 UART** 的**接收** FIFO 塞一个字节（STAGE-48 · D-177 B）。
    ///
    /// ⚠️ **为什么必须有这一条**：console 角色**仍是「第一个 UART」**（角色
    /// 显式化是 D-177 **C**，第 57 批）⇒ ⇒ [`Self::uart_receive`] **只能**
    /// 往第一个 UART 塞字节 ⇒ ⇒ **第二个 UART 的 RX FIFO 永远空** ⇒ ⇒
    /// 它的 `rx_ready` **永远为假** ⇒ ⇒ **它的连线测不到** ⇒ ⇒ 「第二个
    /// UART 的连线也真的通电」这件事**根本证明不了**。
    ///
    /// ⚠️ **只增不减**：console 那条路（[`Self::uart_receive`]）**逐字不变**
    /// ⇒ ⇒ 存量行为**不受影响** ⇒ ⇒ 这**不是**「把角色改成第二个 UART」。
    ///
    /// @param id — 器件 id（描述里的那个 id）
    /// @param byte — 塞进去的那个字节
    /// @returns `true` ⇒ 那个 id **确实**是一台 UART 且注入**成功**
    pub fn uart_receive_on(&mut self, id: &str, byte: u8) -> bool {
        let Some(uart) = self.devices.uart(id) else {
            return false;
        };
        let done = match uart.try_borrow_mut() {
            Ok(mut u) => {
                u.receive(byte);
                true
            }
            Err(_) => false,
        };
        done
    }
}
