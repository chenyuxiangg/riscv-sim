//! 会话表与专属线程（M-24 · R-02 / §5.5）。
//!
//! ## 为什么要「一会话一线程」
//!
//! `KernelHost` 内部是 `Rc` + `RefCell` ⇒ ⇒ **不是 `Send`** ⇒ ⇒ 它不能跨线程
//! 移动、也不能被两处同时借用。于是只有两种选择：
//!
//! 1. `tokio::Mutex` 持锁跨 `await` ⇒ ⇒ 编译不过（`MutexGuard` 不是 `Send`）；
//! 2. `spawn_blocking` ⇒ ⇒ 状态留在**某条池线程**里，下一次请求换个线程就找
//!    不到了。
//!
//! ⇒ ⇒ 本设计的做法：**每会话一条专属 `std::thread`**，`KernelHost` 从生到死
//! 只属于这条线程；外面的世界只能通过**消息**碰它。所谓「零共享」不是口号，
//! 是所有权决定的。
//!
//! ## 命令面与控制面分开
//!
//! - **命令面**（装载 / 推进 / 读状态）：走 `mpsc`，**串行**执行，带回执；
//!   会话忙时**立即**回 `409 + E_SESS-02`，不排队 —— 排队会让延迟不可预测，
//!   也会让「暂停」抢不到前面。
//! - **控制面**（暂停 / 取消）：走 `Arc<AtomicU8>` 的位标记，**不进队列**；
//!   推进者在自己下一个窗口边界看到位就停（≤ 1 个量子）。

use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, Project, ProjectVersion, Watchpoint,
};
use riscv_sim_contract::platform::PlatformDescription;
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, HaltReason, Message};
use riscv_sim_contract::session::{CheckpointMetaDto, SessionSnapshotDto};
use riscv_sim_contract::SCHEMA_VERSION;
use riscv_sim_kernel::kernel_host::{KernelHost, RunStop};

use crate::error::ApiError;

/// 控制位：暂停。推进者在窗口边界读到它就停，不回滚已完成的窗口。
pub const CTRL_PAUSE: u8 = 1 << 0;
/// 控制位：取消（删除 / TTL 回收 / 关进程）。置上后线程不再接新活。
pub const CTRL_CANCEL: u8 = 1 << 1;

/// 「刚收到回执、位还没清」的宽限窗口（见 [`SessionHandle::submit`]）。
const BUSY_GRACE: Duration = Duration::from_millis(3);
/// 宽限窗口的轮询步长。
const BUSY_GRACE_STEP: Duration = Duration::from_millis(1);

/// 一次作业的回执上限。超了 ⇒ `504 + E_SESS-06`（R-11：不许无限等）。
///
/// ⚠️ 这是**作业级**的兜底，不是 `run` 的 `timeout_ms`（那个由内核的
/// 窗口判定，见 P0-4）⇒ ⇒ 两层各管一件事：内核管「跑多久」，这里管
/// 「回执多久没来」。
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// 会话的 **console 字节环**（R-06）。
///
/// ⚠️ **环形裁尾 + 记丢弃区间**（不是「静默丢」）：`drop` 之前的偏移读不到
/// 就回 `409 + E_SESS-04`，并把 `base` 一起给出去 ⇒ ⇒ 客户端能重新对齐，
/// 而不是以为「console 突然变空了」。
#[derive(Debug)]
struct ConsoleRing {
    buf: VecDeque<u8>,
    /// 环里第一个字节的**绝对**偏移。
    base: u64,
    /// 从会话开始到现在写进环的总字节数（含已被裁掉的）。
    total: u64,
    cap: usize,
}

impl ConsoleRing {
    fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            base: 0,
            total: 0,
            cap: cap.max(1),
        }
    }

    /// 追加字节；超出上限时**从尾部裁**（新字节一定留住）。
    fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.total += bytes.len() as u64;
        self.buf.extend(bytes.iter().copied());
        if self.buf.len() > self.cap {
            let drop = self.buf.len() - self.cap;
            self.buf.drain(..drop);
            self.base += drop as u64;
        }
    }

    /// 清内容、**不**归零偏移（D-3：换固件时用）。
    ///
    /// ⚠️ **只 `clear` 不 `reset`**：`total` 继续记账、`base` 推到 `total`
    /// ⇒ ⇒ 拿着旧 `offset` 的客户端落在窗口之前 ⇒ ⇒ 回 `Err(base)` 让它
    /// **重新对齐**，而不是把新固件的输出当成旧固件的延续（那才是真的
    /// 「console 突然变空」）。
    fn clear(&mut self) {
        self.buf.clear();
        self.base = self.total;
    }

    /// 读 `[offset, offset + limit)`。
    ///
    /// 早于 `base` ⇒ `Err(base)`（窗口丢了，告诉客户端从哪重新读）；
    /// 晚于 `total` ⇒ 空（只是还没产生）。
    fn read(&self, offset: u64, limit: usize) -> Result<Vec<u8>, u64> {
        if offset < self.base {
            return Err(self.base);
        }
        if offset >= self.total {
            return Ok(Vec::new());
        }
        let skip = (offset - self.base) as usize;
        let take = limit.min(self.buf.len().saturating_sub(skip));
        Ok(self.buf.iter().skip(skip).take(take).copied().collect())
    }
}

/// 事件流订阅的主题（与内核 `kernel_host::topics::TOPIC_STATE_DELTA` 逐字相同）。
const TOPIC_STATE_DELTA: &str = "sim.state.delta";

/// 一条会话的事件环容量（条）。
const EVENT_RING_CAP: usize = 4096;

/// 会话**事件环**（P1 的 WS 事件流）。
///
/// 与 console 环同一套账法：`seq` 是**会话内**单调的条目号，环满从**旧**
/// 一端栽（新事件一定留住）⇒ ⇒ 客户端拿着 `seq` 续读，丢窗口时会收到
/// `lost`（只能从 `next` 重新对齐）。
#[derive(Debug)]
struct EventRing {
    frames: VecDeque<(u64, serde_json::Value)>,
    /// 下一条要分配的 seq。
    next: u64,
    cap: usize,
    /// 这些帧属于哪条会话（M-07 帧的 `sessionId`）。
    session_id: String,
    /// **不进环**的应答帧（`Tick`）用哪下一个号。
    ///
    /// ⚠️ 与环的 seq **分两个号段**：环从 0 起，这里从
    ///    [`UNRINGED_SEQ_BASE`] 起 ⇒ ⇒ 永远不会与环里的帧撞号。撞号的后
    ///    果不是「显示错一个数字」：前端按「同会话同一 seq 只应用一次」去重
    ///    （契约命令的 HTTP 回体与 WS 推流是同一份事件的**两条路**）⇒ ⇒
    ///    撞号会把一条真事件当成重复丢掉。
    unringed_next: u64,
}

/// 不进环的应答帧的 seq 起点（2^40）。
///
/// 环的 seq 是 `u64` 自增，实际用量远达不到 2^40；而 JS 的 `Number` 到
/// 2^53 还是精确的 ⇒ ⇒ 这个号段既不会撞号，也不会在前端丢精度。
const UNRINGED_SEQ_BASE: u64 = 1 << 40;

impl EventRing {
    fn new(cap: usize, session_id: &str) -> Self {
        Self {
            frames: VecDeque::new(),
            next: 0,
            cap: cap.max(1),
            session_id: session_id.to_string(),
            unringed_next: UNRINGED_SEQ_BASE,
        }
    }

    /// 把一批事件压进环（一条事件 = 一帧 M-07 传输帧）。
    ///
    /// ⚠️ 帧形**就是** `front/lib/frame.js` 的那一层（R-13 逐字要求复用
    ///   M-07 帧，不另造封套）：`{type, seq, sessionId, role, reqId, payload}`
    ///   ⇒ ⇒ `payload` 是**完整的**契约 `Message`（`{schema_version,
    ///   direction, seq, payload}`）——前端 `decodeMessage` 认的是这一层。
    fn push(&mut self, events: &[EventPayload]) {
        for event in events {
            self.store(event);
        }
    }

    /// 存一条事件进环；`Tick`（心跳）不存 ⇒ 返回 `None`。
    fn store(&mut self, event: &EventPayload) -> Option<(u64, serde_json::Value)> {
        // ⚠️ `Tick` 是心跳，**不进流**：它每个窗口一条，对客户端没有信息量，
        //   却会把环里的真事件挤掉。
        if matches!(event, EventPayload::Tick { .. }) {
            return None;
        }
        let seq = self.next;
        let message = Message::event(seq, event.clone());
        let Ok(value) = serde_json::to_value(&message) else {
            return None;
        };
        self.frames.push_back((seq, value.clone()));
        self.next += 1;
        while self.frames.len() > self.cap {
            self.frames.pop_front();
        }
        Some((seq, value))
    }

    /// 契约命令（v1.11）的**应答帧**：只有**第一条**事件是应答（与前端
    /// Worker 通道逐字相同：`front/kernel-worker.js` 的 "the first event
    /// answers the command (M-05 matches it by reqId)"）。
    ///
    /// ⚠️ 应答帧**不进环**：调用方从 HTTP 回体拿它，观察者从事件流拿**其余**
    ///    事件 ⇒ ⇒ 同一份事件只有**一条**路，前端不需要去重（M-05 一个字
    ///    不动）。
    ///
    /// 为什么敢把应答排除在环外：内核每条命令的第一个事件是 `Tick`（心跳）
    /// 或 `Error`（本命令的失败）；真正对观察者有意义的状态增量 / console /
    /// 波形都在**后面**，它们照旧进环 ⇒ ⇒ 观察端不会瞎。
    fn push_answer(
        &mut self,
        events: &[EventPayload],
        req_id: Option<u64>,
    ) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        for (index, event) in events.iter().enumerate() {
            if index == 0 {
                out.push(self.answer_frame(event, req_id));
            } else {
                self.store(event);
            }
        }
        out
    }

    /// 一条 M-07 传输帧的**唯一**构造点。
    fn frame(
        &self,
        seq: u64,
        message: &serde_json::Value,
        req_id: Option<u64>,
    ) -> serde_json::Value {
        serde_json::json!({
            "type": "event",
            "seq": seq,
            "sessionId": self.session_id,
            "role": serde_json::Value::Null,
            "reqId": req_id,
            "payload": message,
        })
    }

    /// 不进环的**应答帧**：号取自保留段（[`UNRINGED_SEQ_BASE`] 起）⇒ ⇒
    /// 一眼能看出「这不是事件流里的那一条」。
    fn answer_frame(&mut self, event: &EventPayload, req_id: Option<u64>) -> serde_json::Value {
        let seq = self.unringed_next;
        self.unringed_next += 1;
        let message = serde_json::to_value(Message::event(seq, event.clone()))
            .unwrap_or(serde_json::Value::Null);
        self.frame(seq, &message, req_id)
    }

    /// 取 `seq >= since` 的帧（最多 `limit` 条）⇒ `(frames, next, lost)`。
    fn since(&self, since: u64, limit: usize) -> (Vec<serde_json::Value>, u64, bool) {
        let oldest = self
            .frames
            .front()
            .map(|(seq, _)| *seq)
            .unwrap_or(self.next);
        let lost = since < oldest;
        let frames: Vec<serde_json::Value> = self
            .frames
            .iter()
            .filter(|(seq, _)| *seq >= since)
            .take(limit.max(1))
            .map(|(seq, message)| {
                // ⚠️ **`payload` 是完整的契约 `Message`**（`{schema_version,
                // direction, seq, payload}`），**不是**它里面的那一层
                // `{"Event":{…}}`。
                //
                // 这条判据是前端定的（`front/services/m05_commandrouter.js`
                // 的 `handleFrame` 直接 `decodeMessage(frame.payload)`，
                // 而 `decodeMessage` 第一步就要 `schema_version`）——与本仓的
                // Worker 通道同形（`front/kernel-worker.js` 的
                // `payload: eventMessage(...)`）。
                //
                // ⚠️ **曾经的错法**：这里只取 `message["payload"]`，等于把
                // 契约信封拆掉 ⇒ ⇒ 前端每一帧都解不开、全部落成
                // `E_PROTO-08`，而 Node 侧只看「帧类型 + 内层键」的冒烟**照样
                // 绿**（假绿）。真浏览器一开就发现寄存器 / console 都是空的。
                self.frame(*seq, message, None)
            })
            .collect();
        (frames, self.next, lost)
    }
}

/// 会话板的来源。**必须记下来**：同一份「板 id」可能来自启动默认或请求，
/// 事后追「这块板从哪来」只能靠它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardSource {
    /// 来自 `--board` 的进程默认值。
    StartupDefault,
    /// 来自 `POST /api/sessions` 的请求体。
    Request,
    /// 请求与启动参数都没给 ⇒ 会话**尚无板** ⇒ 装载时会回 `E_PLT-22`。
    None,
}

impl BoardSource {
    /// 线上拼写（与状态文档一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StartupDefault => "startup-default",
            Self::Request => "request",
            Self::None => "none",
        }
    }
}

/// `run` 的**until** 取值（R-05：`until?:breakpoint|halt`）。
///
/// ⚠️ 两个取值都落到内核**同一个**判据（「继续到下一个停点」）：内核的
///   `run_until(None, …)` 只有一种「继续」语义，停因由它自己给
///   （断点 / 观察点 / trap / 结束）⇒ ⇒ 服务端**不**在这里再分叉出一个
///   第二套停因表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunUntil {
    /// 只认断点（观察点 / 单步这类停点继续跑）。
    Breakpoint,
    /// 继续到停机。
    Halt,
}

impl RunUntil {
    /// 从线上拼写解析；未知取值 ⇒ `None`（由调用方回 400）。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "breakpoint" => Some(Self::Breakpoint),
            "halt" => Some(Self::Halt),
            _ => None,
        }
    }

    /// 这个停因**算不算**这次的 `until` 目标。
    ///
    /// `halt` ⇒ 任何停点都算；`breakpoint` ⇒ 只认断点（观察点 / 单步
    /// 这类「停一下但不该收工」的停因继续跑）。
    fn accepts(self, label: &str) -> bool {
        match self {
            Self::Halt => true,
            Self::Breakpoint => label == "breakpoint",
        }
    }
}

/// 一次 `run` 的**完整停点描述**（R-05）。
///
/// ⚠️ 同步 `run` / `step` / 异步 `run` 共用这一份 ⇒ ⇒ 折算规则只有一处
///   （`api::plan_run`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunSpec {
    /// 指令上限（`step` 就是 `Some(n)`）。
    pub steps: Option<u32>,
    /// 墙钟上限（毫秒；`None` ⇒ 用服务端的 `--max-run-ms`）。
    pub timeout_ms: Option<u64>,
    /// 跑到 console 里出现这个字节串就停（R-05 的 `prefix`）。
    ///
    /// ⚠️ 判据在**服务端**：内核 `run_until` 不认识 console 内容 ⇒ ⇒ 按
    ///   小窗口推进、每个窗口扫一遍新增的 console 字节（与
    ///   `POST /api/boot` **同一套**做法）。停因仍由内核给，前缀只是
    ///   「服务端发现条件已满足就收工」。
    pub prefix: Option<String>,
    /// 继续到下一个停点（`until`）。
    pub until: Option<RunUntil>,
}

/// 一个会话的对外描述（列表与详情共用）。
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// 会话 id。
    pub id: String,
    /// 生效的板模板 id（`None` ⇒ 尚未配板）。
    pub board: Option<String>,
    /// 板从哪来。
    pub board_source: BoardSource,
    /// 这条会话的内核宿主所理解的 schema 版本。
    pub schema_version: u32,
    /// 已存活毫秒数。
    pub age_ms: u64,
    /// 距上次活动毫秒数（TTL 判据）。
    pub idle_ms: u64,
    /// 是否正在执行一条命令（只作展示；拒绝由 `ping` 现场再查一次）。
    pub busy: bool,
}

/// 命令面的作业。**一个作业 = 一次串行执行的活**。
///
/// ⚠️ 允许「变体大小不均」：作业是**一次性**的（造一个、跑一个、丢一个），
///    队列深度只有 1 ⇒ ⇒ Box 化只会多一次分配，换不来任何内存收益。
#[allow(clippy::large_enum_variant)]
enum Job {
    /// 探活：证明「线程还在、状态还能取」。
    Ping {
        /// 回执通道。线程侧只发一次。
        reply: Sender<PingReply>,
    },
    /// 配板（R-03）：内核侧的 `SetMemoryMap`，回执是「当前生效的图」。
    SetBoard {
        /// 板描述（**已经**由调用方从模板取好 / 从请求体验过）。
        description: Box<PlatformDescription>,
        /// 乐观并发的期望修订号；`None` ⇒ 不检查。
        expected_revision: Option<u64>,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 跑到停点（R-05）：内核侧的 `run_until`（**唯一**停因判定），回执带
    /// 停因、本次条数、pc、虚拟时间与本次新增的 console 字节。
    ///
    /// ⚠️ **停点描述是一整份 [`RunSpec`]**（`steps` / `timeout_ms` /
    /// `prefix` / `until`）⇒ ⇒ 同步与异步走**同一个**执行体，不各写一套。
    Run {
        /// 停点描述。
        spec: RunSpec,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 异步 run（P1）：跑完把结果写进 `slot`，**没有**回执通道。
    RunAsync {
        /// 停点描述（与同步 `Run` 逐字相同）。
        spec: RunSpec,
        /// 结果落点（HTTP 侧拿 `job_id` 轮询它）。
        slot: Arc<AsyncSlot>,
    },
    /// 复位（R-14）：回到**当前工程的装载起点** —— 用会话记下的那份镜像
    /// 重新走一遍 `LoadProject`（同一块板、同一订阅）。
    ///
    /// ⚠️ **没有镜像就没什么可复位** ⇒ ⇒ `409 + E_SESS-02`（不是 200 空跑）。
    Reset {
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 列出观察点（R-08）：内核没有 `ListWatchpoints` 命令 ⇒ ⇒ 走宿主新增的
    /// 只读访问器（[`KernelHost::list_watchpoints`]）。
    Watchpoints {
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 设置观察点（R-08）：`SetWatchpoint` 的即时回执只有 `Tick`
    /// ⇒ ⇒ 回体用宿主的**真实表**（不猜）。
    SetWatchpoint {
        /// 观察点（id 由 API 层分配或由调用方显式给）。
        watchpoint: Watchpoint,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 清除观察点（R-08）：`ClearWatchpoint` 幂等（id 不存在也不报错）。
    ClearWatchpoint {
        /// 观察点 id。
        id: u32,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 契约命令（v1.11）：**整条**契约 `Message` 原样交给内核
    /// `handle_message`，回体是这条命令产生的全部事件（M-07 帧）。
    ///
    /// ⚠️ 与 [`Job::Debug`] 的差别只有「输入是整条消息、输出是帧数组」：
    ///    执行体是**同一个**会话线程、同一条作业队列 ⇒ ⇒ 不存在第二套执行
    ///    路径，也不许有第二份判据（错误仍由内核的 `Error` 事件表达）。
    ///
    /// ⚠️ 契约路径**不代订**任何 topic：`LoadProject` 会清空内核订阅，页面
    ///    自己按本地模式的习惯重订。REST 装载路径的「代订
    ///    `sim.state.delta`」是给不认识契约的客户端用的，两条不混。
    Contract {
        /// 冻结的契约消息（`direction` 已由 API 层验过是 `Command`）。
        message: Box<Message>,
        /// M-07 帧的配对号：回贴到**第一条**应答帧上。
        req_id: Option<u64>,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 读事件流（P1）：`seq >= since` 的帧。
    Events {
        /// 起始序号。
        since: u64,
        /// 最多多少帧。
        limit: usize,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 采集会话快照（P1）：走宿主 `capture_session`（六段齐全）。
    Snapshot {
        /// 检查点元数据（`t_virt` 由宿主按引擎虚拟时间**代填**）。
        meta: CheckpointMetaDto,
        /// 回执通道（一整个 `SessionSnapshotDto` 的 JSON）。
        reply: Sender<JobReply>,
    },
    /// 还原会话快照（P1）：走宿主 `restore_session`（校验先于应用）。
    Restore {
        /// 会话载荷。
        payload: Box<SessionSnapshotDto>,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 读状态（R-07）：pc / 32 个通用寄存器 / 已执行条数。
    State {
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 读符号表（R-07）：宿主侧已有索引，**不重解析**镜像。
    Symbols {
        /// 最多多少条。
        limit: usize,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 调试命令（R-07）：`DumpMemory` / `Disasm` / 断点族**原样透传**给内核，
    /// 事件再翻成 JSON。
    ///
    /// ⚠️ 走一条通用作业而不是四个专用作业：这四条命令的**判据全在内核**
    /// （`E_DBG-03` 的越界、`E_DBG-02` 的名字 / 表达式）⇒ 服务端再造一层
    /// 判据就是第二个真相。
    Debug {
        /// 要发给内核的命令。
        command: CommandPayload,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 读 console（R-06）：从会话环里取 `[offset, offset+limit)`。
    Console {
        /// 起始绝对偏移。
        offset: u64,
        /// 最多多少字节。
        limit: usize,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
    /// 装载镜像（R-04）：内核侧的 `LoadProject`，回执是装载回执 + 落点。
    Load {
        /// 镜像名（回显给客户端）。
        name: String,
        /// 镜像种类（契约 `ImageKind`）。
        kind: ImageKind,
        /// 镜像字节（**上传的**或白名单路径读出来的，此处已无来源差别）。
        bytes: Vec<u8>,
        /// 回执通道。
        reply: Sender<JobReply>,
    },
}

/// 内核侧失败：`EventPayload::Error { code, message }` 的搬运行李。
///
/// ⚠️ **状态码不在这里**：同一个码在不同发生点状态不同（`E_PLT-22` 在
/// 「未知模板」是 404、在「无板装载」是 409）⇒ ⇒ 状态由**API 层**按
/// 发生点给（见 `error.rs` 的模块注释）。
#[derive(Debug, Clone)]
struct KernelFailure {
    /// 契约场景码。
    code: String,
    /// 内核给的定位信息（**原样**，不改写）。
    message: String,
}

/// 一条作业的回执：成功是 JSON 体，失败是内核的原话。
type JobReply = Result<serde_json::Value, KernelFailure>;

/// 一条**异步**作业的落点（P1：`run?async`）。
///
/// 为什么会存在：`run` 可能要跑几秒到十几秒，把 HTTP 连接一直占着对
/// Agent / CI 都不友好 ⇒ ⇒ 提交后立刻回 `job_id`，由调用方轮询。
///
/// ⚠️ 结果**只能**由会话线程写（宿主 `!Send`）⇒ ⇒ 这里是一个
/// `Arc<Mutex<_>>` 的中转格，HTTP 只读不写。
#[derive(Debug)]
struct AsyncSlot {
    /// 这套记录的可读状态与结果。
    inner: Mutex<AsyncState>,
}

/// 异步作业的对外状态。
#[derive(Debug, Clone)]
struct AsyncState {
    /// `run`（未来可能有 `boot` 等）。
    kind: &'static str,
    /// 什么时候结束的（毫秒，相对提交）；`None` ⇒ 还在跑。
    elapsed_ms: Option<u64>,
    /// 结果或失败（与同步 `run` 的回执同形）。
    outcome: Option<JobReply>,
}

impl AsyncSlot {
    fn new(kind: &'static str) -> Self {
        Self {
            inner: Mutex::new(AsyncState {
                kind,
                elapsed_ms: None,
                outcome: None,
            }),
        }
    }

    /// 会话线程侧：写结果。**只写一次**（重复写不会覆盖，避免两个值两个真相）。
    fn finish(&self, outcome: JobReply, elapsed_ms: u64) {
        if let Ok(mut state) = self.inner.lock() {
            if state.outcome.is_none() {
                state.elapsed_ms = Some(elapsed_ms);
                state.outcome = Some(outcome);
            }
        }
    }

    /// HTTP 侧：读一份快照。
    fn read(&self, id: &str) -> serde_json::Value {
        let Ok(state) = self.inner.lock() else {
            return serde_json::json!({ "job_id": id, "state": "unreadable" });
        };
        match &state.outcome {
            None => serde_json::json!({
                "job_id": id,
                "kind": state.kind,
                "state": "running",
            }),
            Some(Ok(body)) => serde_json::json!({
                "job_id": id,
                "kind": state.kind,
                "state": "done",
                "elapsed_ms": state.elapsed_ms,
                "report": body,
            }),
            Some(Err(failure)) => serde_json::json!({
                "job_id": id,
                "kind": state.kind,
                "state": "failed",
                "elapsed_ms": state.elapsed_ms,
                "code": failure.code,
                "detail": failure.message,
            }),
        }
    }
}

/// 一条会话的作业表。上限 [`ASYNC_JOB_CAP`]：满了就丢**最旧的已结束**作业。
#[derive(Debug, Default)]
struct JobTable {
    next: u64,
    order: Vec<String>,
    map: HashMap<String, Arc<AsyncSlot>>,
}

/// 每会话保留的异步作业条数。
const ASYNC_JOB_CAP: usize = 32;

/// 探活回执。
struct PingReply {
    /// 宿主当前 pc（P0-2 里尚无镜像 ⇒ 恒为 0）。
    pc: u32,
    /// 宿主已执行的指令条数。
    executed: u64,
}

/// 一条会话的把手。**不含** `KernelHost` —— 宿主在对面那条线程里。
pub struct SessionHandle {
    /// 会话 id。
    pub id: String,
    /// 这条会话属于哪个账号（M-25）。兼容模式一律 `"legacy"`。
    pub owner: String,
    /// 当前板模板 id。**可写**：`PUT …/board` 改了板之后，列表与详情要跟着
    /// 说新板（否则就是第二个真相）。
    board: Mutex<Option<String>>,
    /// 板从哪来（同样可写：PUT 之后来源变成 `request`）。
    board_source: Mutex<BoardSource>,
    /// 创建时刻。
    pub created: Instant,
    tx: Sender<Job>,
    ctrl: Arc<AtomicU8>,
    busy: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    last_active: Arc<Mutex<Instant>>,
    /// 异步作业表（`run?async`）。
    jobs: Arc<Mutex<JobTable>>,
    /// 线程把手。**不为 join 服务**（见 [`SessionHandle::join_within`]）：它的
    /// 唯一作用是让「线程还在跑」这件事有个句柄拴着，进程退出前不被回收。
    #[allow(dead_code)]
    join: JoinHandle<()>,
}

impl SessionHandle {
    /// 会话是否还活着（线程未退出、未 panic）。
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// 是否正在执行作业。
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// 距上次活动多久。
    pub fn idle_for(&self) -> Duration {
        self.last_active
            .lock()
            .map(|instant| instant.elapsed())
            .unwrap_or_default()
    }

    /// 置暂停位（控制面，**不进队列**，立即生效于下一个窗口边界）。
    pub fn request_pause(&self) {
        self.ctrl.fetch_or(CTRL_PAUSE, Ordering::AcqRel);
    }

    /// 置取消位并关闭命令面 ⇒ ⇒ 线程的 `recv` 返回 `Err` 后自行退出。
    fn request_cancel(&self) {
        self.ctrl.fetch_or(CTRL_CANCEL, Ordering::AcqRel);
    }

    /// 一秒钟都等不到就不等了：**删除是控制面动作**，不该被正在跑的作业卡住。
    ///
    /// ⚠️ **只等标志、不 `join`**：句柄现在住在 `Arc` 里（可能被别的请求借
    /// 着）⇒ ⇒ `JoinHandle::join` 要按值消耗，拿不到。线程自己在 `recv`
    /// 失败后退出，句柄随 `Arc` 落地时自动 detach ⇒ ⇒ 不会留僵尸进程，
    /// 只是回收晚一步。
    fn join_within(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while self.is_alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn info(&self) -> SessionInfo {
        SessionInfo {
            id: self.id.clone(),
            board: self.board.lock().map(|b| b.clone()).unwrap_or(None),
            board_source: self
                .board_source
                .lock()
                .map(|s| *s)
                .unwrap_or(BoardSource::None),
            schema_version: SCHEMA_VERSION,
            age_ms: self.created.elapsed().as_millis() as u64,
            idle_ms: self.idle_for().as_millis() as u64,
            busy: self.is_busy(),
        }
    }

    /// 投递一个作业并等回执。忙 ⇒ `409 + E_SESS-02`；线程没了 ⇒
    /// `500 + E_SESS-03`。
    pub fn ping(&self) -> Result<(u32, u64), ApiError> {
        if !self.is_alive() {
            return Err(ApiError::session_poisoned(&self.id));
        }
        // ⚠️ 「忙」有一瞬的**假阳性**：会话线程先发回执、再清 `busy` 位
        //    ⇒ ⇒ 调用方拿到回执**立刻**发下一条（`POST /api/boot` 就是这种
        //    节奏）时，可能正好撞进这个缝隙。这里最多等 [`BUSY_GRACE`]；
        //    真在跑的命令（毫秒级以上）照样回 409 —— 判据没被放宽。
        if self.busy.load(Ordering::Acquire) {
            let mut waited = Duration::ZERO;
            while self.busy.load(Ordering::Acquire) && waited < BUSY_GRACE {
                thread::sleep(BUSY_GRACE_STEP);
                waited += BUSY_GRACE_STEP;
            }
            if self.busy.load(Ordering::Acquire) {
                return Err(ApiError::session_busy(&self.id));
            }
        }
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Job::Ping { reply: tx })
            .map_err(|_| ApiError::session_poisoned(&self.id))?;
        match rx.recv_timeout(Duration::from_millis(2_000)) {
            Ok(reply) => Ok((reply.pc, reply.executed)),
            Err(_) => Err(ApiError::session_poisoned(&self.id)),
        }
    }

    /// 投递一条作业并等回执（忙 ⇒ 409；线程没了 ⇒ 500；超时 ⇒ 504）。
    ///
    /// `map` 是**发生点**给的「内核码 + 原话 ⇒ `ApiError`」换算器 ——
    /// 状态码取决于谁在调用（同一个 `E_PLT-22` 在配板是 404、在装载是 409）。
    fn submit(
        &self,
        make: impl FnOnce(Sender<JobReply>) -> Job,
        map: fn(&str, &str) -> ApiError,
    ) -> Result<serde_json::Value, ApiError> {
        if !self.is_alive() {
            return Err(ApiError::session_poisoned(&self.id));
        }
        // ⚠️ 「忙」有一瞬的**假阳性**：会话线程先发回执、再清 `busy` 位
        //    ⇒ ⇒ 调用方拿到回执**立刻**发下一条（`POST /api/boot` 就是这种
        //    节奏）时，可能正好撞进这个缝隙。这里最多等 [`BUSY_GRACE`]；
        //    真在跑的命令（毫秒级以上）照样回 409 —— 判据没被放宽。
        if self.busy.load(Ordering::Acquire) {
            let mut waited = Duration::ZERO;
            while self.busy.load(Ordering::Acquire) && waited < BUSY_GRACE {
                thread::sleep(BUSY_GRACE_STEP);
                waited += BUSY_GRACE_STEP;
            }
            if self.busy.load(Ordering::Acquire) {
                return Err(ApiError::session_busy(&self.id));
            }
        }
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(make(tx))
            .map_err(|_| ApiError::session_poisoned(&self.id))?;
        match rx.recv_timeout(JOB_TIMEOUT) {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(failure)) => Err(map(&failure.code, &failure.message)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ApiError::session_timeout(&self.id)),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ApiError::session_poisoned(&self.id)),
        }
    }

    /// 记下「这个会话现在用的是哪块板」。**只在配板成功后调** ⇒ ⇒ 失败的
    /// `PUT` 不改口径（用户的板没被接受，会话还是原来那块）。
    fn remember_board(&self, board_id: String) {
        if let Ok(mut board) = self.board.lock() {
            *board = Some(board_id);
        }
        if let Ok(mut source) = self.board_source.lock() {
            *source = BoardSource::Request;
        }
    }

    /// 配板（R-03）：把板描述交给内核的 `SetMemoryMap`，回执是**当前生效
    /// 的图**（不是请求里那份 —— 内核可能因为「已装载镜像」让它待生效）。
    pub fn set_board(
        &self,
        description: PlatformDescription,
        expected_revision: Option<u64>,
    ) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::SetBoard {
                description: Box::new(description),
                expected_revision,
                reply,
            },
            crate::error::board_failure,
        )
    }

    /// 跑到停点（R-05）。停因由内核的 `run_until` 给，这里只搬运。
    pub fn run(&self, spec: RunSpec) -> Result<serde_json::Value, ApiError> {
        self.submit(|reply| Job::Run { spec, reply }, crate::error::load_failure)
    }

    /// 复位（R-14）：重新装载当前工程，回到装载起点。
    pub fn reset(&self) -> Result<serde_json::Value, ApiError> {
        self.submit(|reply| Job::Reset { reply }, crate::error::reset_failure)
    }

    /// 列出观察点（R-08）。
    pub fn watchpoints(&self) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Watchpoints { reply },
            crate::error::board_failure,
        )
    }

    /// 设置观察点（R-08）。
    pub fn set_watchpoint(&self, wp: Watchpoint) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::SetWatchpoint {
                watchpoint: wp,
                reply,
            },
            crate::error::board_failure,
        )
    }

    /// 清除观察点（R-08）。
    pub fn clear_watchpoint(&self, id: u32) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::ClearWatchpoint { id, reply },
            crate::error::board_failure,
        )
    }

    /// 读 console（R-06）。
    pub fn console(&self, offset: u64, limit: usize) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Console {
                offset,
                limit,
                reply,
            },
            crate::error::console_failure,
        )
    }

    /// trace 开关（P1）：`SetTraceEnabled`。
    pub fn set_trace(&self, enabled: bool) -> Result<serde_json::Value, ApiError> {
        self.debug(CommandPayload::SetTraceEnabled { enabled })
    }

    /// trace 导出（P1）：`ExportTrace`。
    pub fn export_trace(&self, command: CommandPayload) -> Result<serde_json::Value, ApiError> {
        self.debug(command)
    }

    /// 采集会话快照（P1）。
    pub fn snapshot(&self, meta: CheckpointMetaDto) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Snapshot { meta, reply },
            crate::error::snapshot_failure,
        )
    }

    /// 还原会话快照（P1）。
    pub fn restore(&self, payload: Box<SessionSnapshotDto>) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Restore { payload, reply },
            crate::error::snapshot_failure,
        )
    }

    /// 提交一条异步 `run`（P1）：立刻回 `job_id`，结果写进槽后由
    /// [`Self::job`] 读。
    pub fn run_async(&self, spec: RunSpec) -> Result<serde_json::Value, ApiError> {
        if !self.is_alive() {
            return Err(ApiError::session_poisoned(&self.id));
        }
        if self.busy.load(Ordering::Acquire) {
            return Err(ApiError::session_busy(&self.id));
        }
        let slot = Arc::new(AsyncSlot::new("run"));
        let job_id = {
            let mut table = self
                .jobs
                .lock()
                .map_err(|_| ApiError::session_poisoned(&self.id))?;
            let id = format!("job-{:04}", table.next);
            table.next += 1;
            table.map.insert(id.clone(), Arc::clone(&slot));
            table.order.push(id.clone());
            // 环满：丢**最旧的已结束**作业；全是 running 就压最旧那条
            // （它一定已经结束——单线程会话一次只跑一件）。
            while table.order.len() > ASYNC_JOB_CAP {
                let oldest = table.order.remove(0);
                table.map.remove(&oldest);
            }
            id
        };
        self.tx
            .send(Job::RunAsync { spec, slot })
            .map_err(|_| ApiError::session_poisoned(&self.id))?;
        Ok(serde_json::json!({
            "job_id": job_id,
            "kind": "run",
            "state": "running",
            "poll": format!("/api/sessions/{}/jobs/{}", self.id, job_id),
        }))
    }

    /// 读一条异步作业（P1）。
    pub fn job(&self, job_id: &str) -> Result<serde_json::Value, ApiError> {
        let slot = {
            let table = self
                .jobs
                .lock()
                .map_err(|_| ApiError::session_poisoned(&self.id))?;
            table.map.get(job_id).cloned()
        };
        match slot {
            Some(slot) => Ok(slot.read(job_id)),
            None => Err(ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "E_SESS-01",
                format!(
                    "job {job_id} does not exist in session {} (jobs are capped at {ASYNC_JOB_CAP})",
                    self.id
                ),
            )),
        }
    }

    /// 读事件流（P1）。
    pub fn events(&self, since: u64, limit: usize) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Events {
                since,
                limit,
                reply,
            },
            crate::error::debug_failure,
        )
    }

    /// 读状态（R-07）。
    pub fn state(&self) -> Result<serde_json::Value, ApiError> {
        self.submit(|reply| Job::State { reply }, crate::error::debug_failure)
    }

    /// 列符号（R-07）。
    pub fn symbols(&self, limit: usize) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Symbols { limit, reply },
            crate::error::debug_failure,
        )
    }

    /// 调试命令透传（R-07）。
    pub fn debug(&self, command: CommandPayload) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Debug { command, reply },
            crate::error::debug_failure,
        )
    }

    /// 契约命令（v1.11）：整条契约 `Message` 进会话线程，回体是这条命令产生
    /// 的全部事件（M-07 帧，首帧带 `req_id`）。
    ///
    /// ⚠️ 失败映射用 `debug_failure`：这条通道**只**在「线程没了 / 忙 / 超时」
    ///    这类传输级故障上返回 HTTP 错误；内核的业务判据（`E_PLT-22` /
    ///    `E_DBG-02` …）仍然以 `Error` 事件进回体 ⇒ ⇒ 一个判据只有一个落点。
    pub fn contract(
        &self,
        message: Message,
        req_id: Option<u64>,
    ) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Contract {
                message: Box::new(message),
                req_id,
                reply,
            },
            crate::error::debug_failure,
        )
    }

    /// 装载镜像（R-04）：把**已经拿到手的字节**包成一个单镜像工程交给内核，
    /// 回执是那一条 `ImageLoaded` + 落点 pc。
    ///
    /// ⚠️ **不在这里**判「有没有板」：内核 v0.76 起对无板装载直接回
    /// `E_PLT-22`（带中文引导）⇒ ⇒ 服务端**不造第二条判据**。
    pub fn load_image(
        &self,
        name: String,
        kind: ImageKind,
        bytes: Vec<u8>,
    ) -> Result<serde_json::Value, ApiError> {
        self.submit(
            |reply| Job::Load {
                name,
                kind,
                bytes,
                reply,
            },
            crate::error::load_failure,
        )
    }
}

/// 会话表。**唯一**的可变状态是表本身；会话状态全在各自线程里。
pub struct SessionRegistry {
    inner: Arc<Mutex<Inner>>,
    max_sessions: usize,
    /// 单账号并发上限（M-25）。默认 `usize::MAX` ⇒ 单测与兼容模式不受影响。
    max_per_account: usize,
    ttl: Duration,
    /// 每会话 console 环上限（`--max-console`）。
    max_console: usize,
}

struct Inner {
    next: u64,
    /// 句柄住在 `Arc` 里 ⇒ ⇒ 「借一条会话」可以先放锁再干活（不让一次装载
    /// 把整张表冻住）。
    sessions: HashMap<String, Arc<SessionHandle>>,
}

impl SessionRegistry {
    /// 建表。`max_sessions` 是并发上限（配额），`ttl` 是空闲回收窗口。
    pub fn new(max_sessions: usize, ttl: Duration, max_console: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next: 1,
                sessions: HashMap::new(),
            })),
            max_sessions,
            max_per_account: usize::MAX,
            ttl,
            max_console,
        }
    }

    /// 当前会话数。
    pub fn used(&self) -> usize {
        self.inner
            .lock()
            .map(|inner| inner.sessions.len())
            .unwrap_or_default()
    }

    /// 并发上限。
    pub fn quota(&self) -> usize {
        self.max_sessions
    }

    /// 建会话：配额检查 → 起线程 → 登记。
    ///
    /// ⚠️ 配额检查与登记在**同一把锁**里 ⇒ ⇒ 两个并发 `POST` 不会同时看到
    /// 「还差一个名额」然后各建一条。
    /// 设置单账号并发上限（M-25）。0 不合法 ⇒ 由 `Config::parse` 拒掉。
    pub fn set_max_per_account(&mut self, max: usize) {
        self.max_per_account = max.max(1);
    }

    /// 建会话并按**账号**记账。配额两条：全局 `max_sessions` 与
    /// `max_per_account`，**同一把锁里一起判** ⇒ ⇒ 一个账号打满自己的份额
    /// 不会把别人的名额吃掉，两个并发请求也不会同时看到「还差一个」。
    pub fn create_for(
        &self,
        board: Option<String>,
        board_source: BoardSource,
        owner: &str,
    ) -> Result<SessionInfo, ApiError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| ApiError::session_poisoned("registry"))?;
        // 顺手把已死的会话剔掉，别让「死了但还没删」占着额度。
        inner.sessions.retain(|_, handle| handle.is_alive());
        if inner.sessions.len() >= self.max_sessions {
            return Err(ApiError::session_quota(self.max_sessions));
        }
        let mine = inner
            .sessions
            .values()
            .filter(|handle| handle.owner == owner)
            .count();
        if mine >= self.max_per_account {
            return Err(ApiError::account_quota(self.max_per_account));
        }

        // ⚠️ **建会话就把板配进内核**，不是只记个名字：P0-3 的冒烟里
        //   `POST /api/sessions {"board":"default"}` 之后装载仍回
        //   `E_PLT-22`（会话写着 board=default，内核那边却没生效板）——
        //   「状态里有、真相里没有」正是这条要堵的洞。
        let description = match board.as_deref() {
            Some(template) => Some(
                riscv_sim_kernel::kernel_host::platform::builtin_description(template)
                    .map_err(|_| ApiError::template_not_found(template))?,
            ),
            None => None,
        };

        let seq = inner.next;
        inner.next += 1;
        let id = format!("sess-{seq:06x}");

        let (tx, rx) = mpsc::channel();
        let ctrl = Arc::new(AtomicU8::new(0));
        let busy = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let last_active = Arc::new(Mutex::new(Instant::now()));

        let handle = Arc::new(SessionHandle {
            id: id.clone(),
            owner: owner.to_string(),
            board: Mutex::new(board.clone()),
            board_source: Mutex::new(board_source),
            created: Instant::now(),
            tx,
            ctrl: Arc::clone(&ctrl),
            busy: Arc::clone(&busy),
            alive: Arc::clone(&alive),
            last_active: Arc::clone(&last_active),
            jobs: Arc::new(Mutex::new(JobTable::default())),
            join: spawn_worker(
                id.clone(),
                rx,
                ctrl,
                busy,
                alive,
                last_active,
                self.max_console,
                description,
            ),
        });
        let info = handle.info();
        inner.sessions.insert(id, handle);
        Ok(info)
    }

    /// 列表：**只列这个账号自己的**会话（M-25）。配额报的是「这个账号自己的
    /// 用量 / 自己那条上限」—— 别人的会话既看不到，也不该算进我的账。
    pub fn list_for(&self, owner: &str) -> (Vec<SessionInfo>, usize, usize) {
        let inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return (Vec::new(), self.max_per_account, 0),
        };
        let mut infos: Vec<SessionInfo> = inner
            .sessions
            .values()
            .filter(|handle| handle.is_alive() && handle.owner == owner)
            .map(|handle| handle.info())
            .collect();
        infos.sort_by(|a, b| a.id.cmp(&b.id));
        let used = infos.len();
        (infos, self.max_per_account, used)
    }

    /// 这条会话属于哪个账号。**不存在也走这一条** ⇒ 404。
    ///
    /// ⚠️ 判据只有一处（中间件），不是每个处理器自己再查一遍：多写一遍就是
    /// 多一个会漏的地方。
    pub fn owner_of(&self, id: &str) -> Result<String, ApiError> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| ApiError::session_poisoned("registry"))?;
        inner
            .sessions
            .get(id)
            .filter(|handle| handle.is_alive())
            .map(|handle| handle.owner.clone())
            .ok_or_else(|| ApiError::session_not_found(id))
    }

    /// 取一条会话的句柄（**放锁**后调用方才用它干活）。不存在 ⇒ 404。
    fn handle_for(&self, id: &str) -> Result<Arc<SessionHandle>, ApiError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::session_poisoned("registry"))?;
        Ok(Arc::clone(
            guard
                .sessions
                .get(id)
                .ok_or_else(|| ApiError::session_not_found(id))?,
        ))
    }

    /// 取一条会话（探活一次，证明不是「表里有、线程已死」）。
    pub fn probe(&self, id: &str) -> Result<(SessionInfo, u32, u64), ApiError> {
        let handle = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| ApiError::session_poisoned("registry"))?;
            Arc::clone(
                guard
                    .sessions
                    .get(id)
                    .ok_or_else(|| ApiError::session_not_found(id))?,
            )
        };
        // ⚠️ 锁已经放了：探活要等回执（最长 2 s），握着表等会冻住所有人。
        let (pc, executed) = handle.ping()?;
        Ok((handle.info(), pc, executed))
    }

    /// 置暂停位（控制面动作，**不排队**）。会话已死 ⇒ 500 + `E_SESS-03`。
    ///
    /// ⚠️ 这里**不**等「真的停下来」：P0-4 的推进者会在下一个窗口边界响应，
    /// 等它停下是 `run` 的回执该回答的事，不是 `pause` 该等的（否则
    /// `pause` 会被一次长 `run` 卡住，正是控制器面的死角）。
    pub fn pause(&self, id: &str) -> Result<SessionInfo, ApiError> {
        let handle = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| ApiError::session_poisoned("registry"))?;
            Arc::clone(
                guard
                    .sessions
                    .get(id)
                    .ok_or_else(|| ApiError::session_not_found(id))?,
            )
        };
        if !handle.is_alive() {
            return Err(ApiError::session_poisoned(id));
        }
        handle.request_pause();
        Ok(handle.info())
    }

    /// 配板（R-03）。**借**一条会话（`Arc` + 放锁）⇒ ⇒ 一次装载/配板不会
    /// 把整张表冻住。
    pub fn set_board(
        &self,
        id: &str,
        description: PlatformDescription,
        expected_revision: Option<u64>,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| ApiError::session_poisoned("registry"))?;
            Arc::clone(
                guard
                    .sessions
                    .get(id)
                    .ok_or_else(|| ApiError::session_not_found(id))?,
            )
        };
        let reply = handle.set_board(description, expected_revision)?;
        if let Some(board_id) = reply
            .get("board_id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        {
            handle.remember_board(board_id);
        }
        Ok(reply)
    }

    /// 跑到停点（R-05）。**借**一条会话，同 [`Self::set_board`]。
    pub fn run(&self, id: &str, spec: RunSpec) -> Result<serde_json::Value, ApiError> {
        let handle = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| ApiError::session_poisoned("registry"))?;
            Arc::clone(
                guard
                    .sessions
                    .get(id)
                    .ok_or_else(|| ApiError::session_not_found(id))?,
            )
        };
        handle.run(spec)
    }

    /// 复位（R-14）。
    pub fn reset(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.reset()
    }

    /// 列出观察点（R-08）。
    pub fn watchpoints(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.watchpoints()
    }

    /// 设置观察点（R-08）。
    pub fn set_watchpoint(&self, id: &str, wp: Watchpoint) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.set_watchpoint(wp)
    }

    /// 清除观察点（R-08）。
    pub fn clear_watchpoint(&self, id: &str, wp: u32) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.clear_watchpoint(wp)
    }

    /// 读 console（R-06）。**借**一条会话，同 [`Self::set_board`]。
    pub fn console(
        &self,
        id: &str,
        offset: u64,
        limit: usize,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| ApiError::session_poisoned("registry"))?;
            Arc::clone(
                guard
                    .sessions
                    .get(id)
                    .ok_or_else(|| ApiError::session_not_found(id))?,
            )
        };
        handle.console(offset, limit)
    }

    /// trace 开关（P1）。
    pub fn set_trace(&self, id: &str, enabled: bool) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.set_trace(enabled)
    }

    /// trace 导出（P1）。
    pub fn export_trace(
        &self,
        id: &str,
        command: CommandPayload,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.export_trace(command)
    }

    /// 提交异步 `run`（P1）。
    pub fn run_async(&self, id: &str, spec: RunSpec) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.run_async(spec)
    }

    /// 读一条异步作业（P1）。
    pub fn job(&self, id: &str, job_id: &str) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.job(job_id)
    }

    /// 采集会话快照（P1）。
    pub fn snapshot(
        &self,
        id: &str,
        meta: CheckpointMetaDto,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.snapshot(meta)
    }

    /// 还原会话快照（P1）。
    pub fn restore(
        &self,
        id: &str,
        payload: Box<SessionSnapshotDto>,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.restore(payload)
    }

    /// 读事件流（P1）。
    pub fn events(
        &self,
        id: &str,
        since: u64,
        limit: usize,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.events(since, limit)
    }

    /// 读状态（R-07）。**借**一条会话，同 [`Self::set_board`]。
    pub fn state(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.state()
    }

    /// 列符号（R-07）。
    pub fn symbols(&self, id: &str, limit: usize) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.symbols(limit)
    }

    /// 调试命令（R-07）。
    pub fn debug(&self, id: &str, command: CommandPayload) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.debug(command)
    }

    /// 契约命令（v1.11）：见 [`SessionHandle::contract`]。
    pub fn contract(
        &self,
        id: &str,
        message: Message,
        req_id: Option<u64>,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = self.handle_for(id)?;
        handle.contract(message, req_id)
    }

    /// 装载镜像（R-04）。**借**一条会话，同 [`Self::set_board`]。
    pub fn load_image(
        &self,
        id: &str,
        name: String,
        kind: ImageKind,
        bytes: Vec<u8>,
    ) -> Result<serde_json::Value, ApiError> {
        let handle = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| ApiError::session_poisoned("registry"))?;
            Arc::clone(
                guard
                    .sessions
                    .get(id)
                    .ok_or_else(|| ApiError::session_not_found(id))?,
            )
        };
        handle.load_image(name, kind, bytes)
    }

    /// 删一条会话：置取消位 → 摘表 → 线程自行退出。
    ///
    /// `join` 放到**锁外**：join 最长等 1 s，握着锁等会把整张表冻住。
    pub fn delete(&self, id: &str) -> Result<SessionInfo, ApiError> {
        let handle = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| ApiError::session_poisoned("registry"))?;
            inner
                .sessions
                .remove(id)
                .ok_or_else(|| ApiError::session_not_found(id))?
        };
        let info = handle.info();
        handle.request_cancel();
        handle.join_within(Duration::from_secs(1));
        Ok(info)
    }

    /// 回收空闲超过 TTL 的会话，返回回收条数。
    pub fn sweep(&self) -> usize {
        let expired: Vec<Arc<SessionHandle>> = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return 0,
            };
            let mut expired: Vec<Arc<SessionHandle>> = Vec::new();
            let mut reap: Vec<String> = Vec::new();
            for (id, handle) in inner.sessions.iter() {
                if !handle.is_alive() || handle.idle_for() >= self.ttl {
                    reap.push(id.clone());
                }
            }
            for id in reap {
                if let Some(handle) = inner.sessions.remove(&id) {
                    expired.push(handle);
                }
            }
            expired
        };
        let count = expired.len();
        for handle in expired {
            handle.request_cancel();
            handle.join_within(Duration::from_millis(200));
        }
        count
    }

    /// 进程退出时把全部会话叫停（不留孤儿线程，R-11）。
    pub fn shutdown(&self) {
        let all: Vec<Arc<SessionHandle>> = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            inner.sessions.drain().map(|(_, handle)| handle).collect()
        };
        for handle in all {
            handle.request_cancel();
            handle.join_within(Duration::from_millis(500));
        }
    }
}

/// 前缀模式的**单窗口预算**（条）。
///
/// 为什么不是一次跑到 `steps`：`prefix` 的判据在 console 字节里，内核对
/// console 内容一无所知 ⇒ ⇒ 只能「小步跑 + 每步看一眼」，窗口越小停得越准。
/// 1 000 条的量级是「几十微秒」⇒ ⇒ `executed` 相对真实停点最多**多算一个
/// 窗口**（文档里把这一点写成明口径，不假装精确）。
const RUN_PREFIX_CHUNK: u32 = 1_000;

/// 前缀模式**没给 `steps`** 时的默认预算（条）。
///
/// ⚠️ 前缀可能永远不出现（镜像根本没打印那一行）⇒ ⇒ 必须有个上界，
/// 否则请求会一直跑到墙钟熔断为止。取与 `POST /api/boot` 同一量级。
const RUN_PREFIX_DEFAULT_BUDGET: u32 = 1_000_000;

/// **一次 `run` 的完整执行体**（同步与异步共用，R-05）。
///
/// 停因判定只有一处（内核 `run_until`）；这里额外做三件**服务端**的事：
///
/// 1. **前缀观察**：有小窗口推进时，每个窗口扫一遍新增的 console 字节；
///    命中 ⇒ `stop_reason = "prefix"`（与 `POST /api/boot` 同一套做法）。
/// 2. **断点回执补全**：停因是 `breakpoint` 时，用 `ListBreakpoints` 的
///    权威表按 `pc` 找回**是哪一条**（内核的 `Halted` 只带 pc）。
/// 3. **统一回体**：`stop_reason / executed / pc / t_ps / total_executed /
///    console{base,bytes,total}`。
fn execute_run(
    host: &mut KernelHost,
    console: &mut ConsoleRing,
    events_ring: &mut EventRing,
    spec: &RunSpec,
    pause_requested: &dyn Fn() -> bool,
) -> serde_json::Value {
    let before = host.total_executed();
    let started = Instant::now();
    let deadline = spec
        .timeout_ms
        .map(|ms| started + Duration::from_millis(ms));
    let prefix = spec.prefix.as_deref();
    // 前缀模式 ⇒ 预算缺省 1e6（`steps` 给了就听它的）。
    let mut remaining: Option<u32> = match prefix {
        Some(_) => Some(spec.steps.unwrap_or(RUN_PREFIX_DEFAULT_BUDGET)),
        None => spec.steps,
    };
    let run_base = console.total;
    // 前缀可能**跨窗口**：保留上一窗口末尾 `len-1` 字节。
    let mut tail: Vec<u8> = Vec::new();
    // `until=breakpoint` 连续「不算数」的停点次数（兜底，见下）。
    let mut skips: u32 = 0;
    // ---- 平台侧错误（E_ENG-03 无进展 / E_ENG-04 异常连环 / E_BUS-01 …）----
    //
    // ⚠️ 内核把这几种「主动中止这一批」的信号放在**事件流**里（`Error`），
    //   回体却只说 `halt`/`budget` ⇒ ⇒ 客户端会把「平台中止」读成「guest
    //   停下」。这里把本窗口新增事件里的第一个错误码提出来，直接收工并
    //   写进回体（`stop_reason = "error"`），不要再继续循环（守卫到阈值
    //   之后每一步都会再命中，继续跑只会再撞同一个错误）。
    let mut error_code: Option<String> = None;

    // ⚠️ 停因**只**由 `break <label>` 交付（带值 break）⇒ ⇒ 不存在「先给个
    //    初值再覆盖」那种永远不会被读到的赋值；也省掉末尾 `unwrap_or` 兜底
    //    —— 兜底值一旦写错，只有这里一处会错。
    let stop: &'static str = loop {
        let chunk = remaining.map(|n| n.min(RUN_PREFIX_CHUNK));
        // 前缀模式：每个窗口都重新算一次剩余墙钟（内核只认绝对上限）。
        let left_ms = deadline
            .map(|until| until.saturating_duration_since(Instant::now()).as_millis() as u64);
        if left_ms == Some(0) {
            break "timeout";
        }
        let timeout_arg = if prefix.is_some() {
            left_ms
        } else {
            spec.timeout_ms
        };
        let report = host.run_until(chunk, timeout_arg, pause_requested);
        // ⚠️ 每个窗口**立刻**把字节搬进会话环：内核捕获环有界，攒着会丢。
        let pending = host.take_pending_events();
        if error_code.is_none() {
            error_code = pending.iter().find_map(|event| match event {
                EventPayload::Error { code, .. } => Some(code.clone()),
                _ => None,
            });
        }
        events_ring.push(&pending);
        let drained = host.drain_console();
        console.push(&drained);
        // 平台错误 ⇒ 这一批已经中止，立刻收工（**不**回滚、不假装是停点）。
        if error_code.is_some() {
            break "error";
        }

        // ---- 前缀观察（只在服务端发生，停因仍由内核给）----
        let mut hit_prefix = false;
        if let Some(needle) = prefix {
            let mut window = tail.clone();
            window.extend_from_slice(&drained);
            if contains_bytes(&window, needle.as_bytes()) {
                hit_prefix = true;
            } else {
                let keep = needle.len().saturating_sub(1);
                tail = window[window.len().saturating_sub(keep)..].to_vec();
            }
        }

        // ---- 内核的停因（优先级最高：它说停了就是停了）----
        let halt = match report.stop {
            RunStop::Halted(_) | RunStop::Paused => Some(stop_label(report.stop)),
            RunStop::Timeout => Some("timeout"),
            RunStop::Steps | RunStop::Budget => None,
        };
        if let Some(label) = halt {
            // `until=breakpoint` ⇒ 观察点 / 单步这类停点**不算收工**，继续跑；
            // 其余（断点 / trap / 结束 / 暂停 / 超时）照常收。
            let accepted = match spec.until {
                Some(until) => until.accepts(label),
                None => true,
            };
            if accepted {
                break label;
            }
            // 兜底：连续跳过 64 次仍没等到目标停点 ⇒ 收工（避免死循环）。
            skips += 1;
            if skips >= 64 {
                break label;
            }
            continue;
        }
        if hit_prefix {
            break "prefix";
        }
        // ---- 预算 ----
        match remaining {
            Some(rem) => {
                let spent = chunk.unwrap_or(rem).min(rem);
                let left = rem - spent;
                remaining = Some(left);
                if left == 0 {
                    // 显式 `steps` 跑满 ⇒ `steps`（步进语义）；前缀模式的
                    // 缺省预算跑满 ⇒ `budget`（「没等到那一行」）。
                    break if spec.steps.is_some() {
                        "steps"
                    } else {
                        "budget"
                    };
                }
            }
            None => {
                // 没有预算 ⇒ 只可能是单次 `until` 调用已经回来（继续不下去）。
                break "budget";
            }
        }
    };

    let executed = host.total_executed().saturating_sub(before);
    let mut body = serde_json::json!({
        "stop_reason": stop,
        "executed": executed,
        "pc": host.pc(),
        "t_ps": host.now_ps(),
        "total_executed": host.total_executed(),
        "console": {
            "base": run_base,
            "bytes": console.total.saturating_sub(run_base),
            "total": console.total,
        },
        // ---- D-1：预算用尽要**明说** ----
        //
        // ⚠️ 前缀模式的缺省预算（1e6 条）跑满时，PC 常常停在 idle 的
        //   `wif/jal` 循环里 ⇒ ⇒ 客户端只看到 `stop_reason = "budget"` 会
        //   把它读成「卡死」。这里把「预算用尽」写成**独立字段**，并回显
        //   上限，让调用方知道该给 `steps` 而不是继续等。
        "budget_exhausted": stop == "budget",
        "budget_steps": RUN_PREFIX_DEFAULT_BUDGET,
    });
    if let Some(code) = &error_code {
        body["error_code"] = serde_json::json!(code);
    }
    // 断点命中：按 pc 去权威表里找（`Halted` 只带 pc，不带 id）。
    if body.get("stop_reason").and_then(|v| v.as_str()) == Some("breakpoint") {
        let pc = host.pc();
        let events = host.handle_message(&Message::command(1, CommandPayload::ListBreakpoints));
        let hit = events.iter().find_map(|event| match event {
            EventPayload::Breakpoints { breakpoints } => breakpoints
                .iter()
                .find(|info| info.breakpoint.enabled && info.breakpoint.addr == pc)
                .map(|info| {
                    serde_json::json!({
                        "id": info.breakpoint.id,
                        "addr": info.breakpoint.addr,
                        "kind": info.breakpoint.kind,
                    })
                }),
            _ => None,
        });
        if let Some(hit) = hit {
            body["breakpoint"] = hit;
        }
    }
    body
}

/// 字节子串判定（不引依赖的朴素实现；console 窗口长度有限，够用）。
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// 停因的**线上拼写**（HTTP body 的 `stop_reason`）。
///
/// ⚠️ 这是**服务端**的词表，不是契约 `HaltReason` 的照抄：`steps` / `budget` /
/// `timeout` / `pause` 里有三个是宿主的决定（guest 没停）⇒ ⇒ 它们不进契约。
fn stop_label(stop: RunStop) -> &'static str {
    match stop {
        RunStop::Halted(HaltReason::Breakpoint) => "breakpoint",
        RunStop::Halted(HaltReason::StepOver) => "step-over",
        RunStop::Halted(HaltReason::Watchpoint) => "watchpoint",
        RunStop::Halted(HaltReason::EndOfExecution) => "end",
        // v21：本窗口预算用尽，机器还在跑（`run` 走显式 steps 分块，正常不会
        // 出现；留着是为了「枚举加一个变体就有一处翻译」这条不漏）。
        RunStop::Halted(HaltReason::BudgetExhausted) => "budget",
        RunStop::Halted(HaltReason::Trap) => "trap",
        RunStop::Halted(HaltReason::UserPause) => "pause",
        RunStop::Halted(HaltReason::Watchdog) => "watchdog",
        RunStop::Steps => "steps",
        RunStop::Budget => "budget",
        RunStop::Timeout => "timeout",
        RunStop::Paused => "pause",
    }
}

/// 从事件流里取出内核报的第一条错误。
fn kernel_failure(events: &[EventPayload]) -> Option<KernelFailure> {
    events.iter().find_map(|event| match event {
        EventPayload::Error { code, message } => Some(KernelFailure {
            code: code.clone(),
            message: message.clone(),
        }),
        _ => None,
    })
}

/// 配板回执：`{revision, board_id, pending, loaded, regions}`。
fn board_reply(host: &KernelHost, events: &[EventPayload]) -> JobReply {
    if let Some(failure) = kernel_failure(events) {
        return Err(failure);
    }
    let (pending, loaded, revision) = events
        .iter()
        .find_map(|event| match event {
            EventPayload::MemoryMap {
                pending,
                loaded,
                revision,
                ..
            } => Some((*pending, *loaded, *revision)),
            _ => None,
        })
        .ok_or_else(|| KernelFailure {
            code: "E_PROTO-08".to_string(),
            message: "内核没有回 memory.map 事件（配板回执缺失）".to_string(),
        })?;
    let regions = host.memory_map().regions;
    Ok(serde_json::json!({
        "revision": revision,
        "board_id": host.platform_id(),
        "pending": pending,
        "loaded": loaded,
        "regions": regions,
    }))
}

/// 装载回执：那一条 `ImageLoaded` + 装载后的 pc。
///
/// ⚠️ **没有回执就是异常**：内核 v19 起装载成功必发一条 ⇒ ⇒ 拿不到说明
/// 「内核说成功、但没给证据」⇒ ⇒ 明确报出，不猜一个 deadline。
fn load_reply(host: &KernelHost, events: &[EventPayload]) -> JobReply {
    if let Some(failure) = kernel_failure(events) {
        return Err(failure);
    }
    let receipt = events.iter().find_map(|event| match event {
        EventPayload::ImageLoaded {
            name,
            kind,
            entry,
            segments,
            elf_bits,
            has_rvc,
            platform_revision,
        } => Some(serde_json::json!({
            "name": name,
            "kind": kind,
            "entry": entry,
            "segments": segments,
            "isa": { "bits": elf_bits, "has_rvc": has_rvc },
            "platform_revision": platform_revision,
            "pc": host.pc(),
            "executed": host.total_executed(),
        })),
        _ => None,
    });
    receipt.ok_or_else(|| KernelFailure {
        code: "E_PROTO-08".to_string(),
        message: "内核报告装载成功，却没有发 image.loaded 回执".to_string(),
    })
}

/// 这条调试命令会不会动断点表（⇒ ⇒ 回执要另问一次 `ListBreakpoints`）。
fn breakpoint_family(command: &CommandPayload) -> bool {
    matches!(
        command,
        CommandPayload::SetBreakpoint(_)
            | CommandPayload::ClearBreakpoint { .. }
            | CommandPayload::SetBreakpointEnabled { .. }
            | CommandPayload::ListBreakpoints
    )
}

/// 把调试命令的回执翻成**扁平** JSON。
///
/// ⚠️ **不**直接 `serde_json::to_value(EventPayload)`：契约事件的线上形状是
/// 外部标签（`{"MemoryDump": {...}}`）⇒ ⇒ 那样前端要多剥一层，而 HTTP 的
/// 契约就是「一条请求 = 一个对象」⇒ ⇒ 这里按命令逐条翻。
fn debug_reply(
    command: &CommandPayload,
    host: &KernelHost,
    events: &[EventPayload],
    table_events: Option<&[EventPayload]>,
) -> JobReply {
    if let Some(failure) = kernel_failure(events) {
        return Err(failure);
    }
    match command {
        CommandPayload::ResolveSymbol { name } => {
            let resolved = events.iter().rev().find_map(|event| match event {
                EventPayload::SymbolResolved {
                    symbol,
                    candidates,
                    total,
                    ..
                } => Some((symbol.clone(), candidates.clone(), *total)),
                _ => None,
            });
            match resolved {
                Some((symbol, candidates, total)) => Ok(serde_json::json!({
                    "name": name,
                    "symbol": symbol,
                    "candidates": candidates,
                    "candidates_total": total,
                })),
                None => Err(KernelFailure {
                    code: "E_PROTO-08".to_string(),
                    message: "内核回 ResolveSymbol 却没有 SymbolResolved 事件".to_string(),
                }),
            }
        }
        CommandPayload::DumpMemory { addr, .. } => {
            let bytes = events.iter().rev().find_map(|event| match event {
                EventPayload::MemoryDump { bytes, .. } => Some(bytes.clone()),
                _ => None,
            });
            match bytes {
                Some(bytes) => Ok(serde_json::json!({
                    "addr": addr,
                    "len": bytes.len(),
                    "bytes": bytes,
                })),
                None => Err(KernelFailure {
                    code: "E_PROTO-08".to_string(),
                    message: "内核回 DumpMemory 却没有 MemoryDump 事件".to_string(),
                }),
            }
        }
        CommandPayload::Disasm { addr, count } => {
            let entries = events.iter().rev().find_map(|event| match event {
                EventPayload::Disasm { entries } => Some(entries.clone()),
                _ => None,
            });
            match entries {
                Some(entries) => Ok(serde_json::json!({
                    "addr": addr,
                    "requested": count,
                    "returned": entries.len(),
                    // 可能**短于** count：地址未映射就截断（不是错误）。
                    "entries": entries,
                })),
                None => Err(KernelFailure {
                    code: "E_PROTO-08".to_string(),
                    message: "内核回 Disasm 却没有 Disasm 事件".to_string(),
                }),
            }
        }
        CommandPayload::ExportTrace { offset, limit, .. } => {
            let chunk = events.iter().rev().find_map(|event| match event {
                EventPayload::TraceExport {
                    source,
                    format,
                    offset,
                    next_offset,
                    total,
                    bytes,
                    chunk,
                    overflow_marked,
                } => Some(serde_json::json!({
                    "source": source,
                    "format": format,
                    "offset": offset,
                    "next_offset": next_offset,
                    "total": total,
                    "bytes": bytes,
                    "chunk": chunk,
                    "overflow_marked": overflow_marked,
                })),
                _ => None,
            });
            match chunk {
                Some(value) => Ok(value),
                None => Err(KernelFailure {
                    code: "E_PROTO-08".to_string(),
                    message: format!(
                        "内核回 ExportTrace 却没有 TraceExport 事件（offset={offset}, limit={limit}）"
                    ),
                }),
            }
        }
        CommandPayload::SetTraceEnabled { enabled } => {
            let state = events.iter().rev().find_map(|event| match event {
                EventPayload::TraceState {
                    enabled,
                    len,
                    dropped,
                } => Some(serde_json::json!({
                    "enabled": enabled,
                    "len": len,
                    "dropped": dropped,
                })),
                _ => None,
            });
            match state {
                Some(value) => Ok(value),
                None => Err(KernelFailure {
                    code: "E_PROTO-08".to_string(),
                    message: format!(
                        "内核回 SetTraceEnabled 却没有 TraceState 事件（enabled={enabled}）"
                    ),
                }),
            }
        }
        _ => {
            // 断点族（Set / Clear / Enable / List）的回执都是**整张表**。
            // `Set` / `Clear` 的即时事件里没有表 ⇒ ⇒ 用先问出来的那次。
            let source = table_events.unwrap_or(events);
            let table = source.iter().rev().find_map(|event| match event {
                EventPayload::Breakpoints { breakpoints } => Some(breakpoints.clone()),
                _ => None,
            });
            match table {
                Some(breakpoints) => Ok(serde_json::json!({
                    "breakpoints": breakpoints,
                    "pc": host.pc(),
                })),
                None => Err(KernelFailure {
                    code: "E_PROTO-08".to_string(),
                    message: "断点命令没有 Breakpoints 回执".to_string(),
                }),
            }
        }
    }
}

/// 起会话线程：宿主从生到死只属于这里。
///
/// ⚠️ 参数多是**如实**的：七个句柄各自是一个共享点（控制位 / 忙位 / 活位 /
///    心跳 / 配额…），塞进一个元组只是把「七处」变成「七处 + 一层壳」。
#[allow(clippy::too_many_arguments)]
fn spawn_worker(
    id: String,
    rx: Receiver<Job>,
    ctrl: Arc<AtomicU8>,
    busy: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    last_active: Arc<Mutex<Instant>>,
    max_console: usize,
    board: Option<PlatformDescription>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(format!("sess-{id}"))
        .spawn(move || {
            // `KernelHost` 是 `!Send` ⇒ ⇒ 只能在这里 `new()`，不能从外面搬进来。
            let mut host = KernelHost::new();
            let mut console = ConsoleRing::new(max_console);
            let mut events_ring = EventRing::new(EVENT_RING_CAP, &id);
            // 本会话**最后一次成功装载**的镜像（复位 R-14 的原料）。
            let mut last_image: Option<(String, ImageKind, Vec<u8>)> = None;
            // 事件流的基础：**订阅 `sim.state.delta`** ⇒ ⇒ 此后每个运行窗口都会
            // 产出一条增量（不订阅的话内核只发 `Tick`，事件流等于空的）。
            let baseline = host.handle_message(&Message::command(
                0,
                CommandPayload::Subscribe {
                    topic: TOPIC_STATE_DELTA.to_string(),
                },
            ));
            events_ring.push(&baseline);
            // 启动默认 / 请求指定的板，在建会话的这一刻就配进内核 ⇒ ⇒
            // 会话一出生就能直接 `…/image` 装载（R-03 的「配板先于装载」）。
            if let Some(description) = board {
                let events = host.handle_message(&Message::command(
                    1,
                    CommandPayload::SetMemoryMap {
                        description,
                        expected_revision: None,
                    },
                ));
                // 内置模板不可能配不上：配不上说明模板表坏了 ⇒ ⇒ 会话直接中毒，
                // 不要带着「有板」的假象去跑。
                assert!(
                    !events
                        .iter()
                        .any(|event| matches!(event, EventPayload::Error { .. })),
                    "内置板模板必须能配上：{events:?}"
                );
            }
            // panic 也要把 `alive` 落下去 ⇒ ⇒ 外面才会说「中毒」而不是「忙」。
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                while let Ok(job) = rx.recv() {
                    if ctrl.load(Ordering::Acquire) & CTRL_CANCEL != 0 {
                        break;
                    }
                    busy.store(true, Ordering::Release);
                    match job {
                        Job::Ping { reply } => {
                            let _ = reply.send(PingReply {
                                pc: host.pc(),
                                executed: host.total_executed(),
                            });
                        }
                        Job::Run { spec, reply } => {
                            // ⚠️ 暂停位在**窗口边界**被读（≤1 个预算窗口的延迟）：
                            //   `run_until` 收一个闭包 ⇒ ⇒ 内核不知道「谁在要求暂停」，
                            //   只知道「要不要停」。
                            let pause_flag = Arc::clone(&ctrl);
                            let body = execute_run(
                                &mut host,
                                &mut console,
                                &mut events_ring,
                                &spec,
                                &|| pause_flag.load(Ordering::Acquire) & CTRL_PAUSE != 0,
                            );
                            let _ = reply.send(Ok(body));
                        }
                        Job::Console {
                            offset,
                            limit,
                            reply,
                        } => {
                            let _ = reply.send(match console.read(offset, limit) {
                                Ok(bytes) => Ok(serde_json::json!({
                                    "offset": offset,
                                    "base": console.base,
                                    "total": console.total,
                                    "bytes": bytes,
                                })),
                                Err(base) => Err(KernelFailure {
                                    code: "E_SESS-04".to_string(),
                                    message: format!(
                                        "console window lost: the ring starts at offset {base}; \
                                         re-read from there (requested offset {offset})"
                                    ),
                                }),
                            });
                        }
                        Job::RunAsync { spec, slot } => {
                            let started = Instant::now();
                            let pause_flag = Arc::clone(&ctrl);
                            let body = execute_run(
                                &mut host,
                                &mut console,
                                &mut events_ring,
                                &spec,
                                &|| pause_flag.load(Ordering::Acquire) & CTRL_PAUSE != 0,
                            );
                            let elapsed_ms = started.elapsed().as_millis() as u64;
                            slot.finish(Ok(body), elapsed_ms);
                        }
                        Job::Snapshot { meta, reply } => {
                            let payload = host.capture_session(meta);
                            let bytes = serde_json::to_vec(&payload)
                                .map(|raw| raw.len())
                                .unwrap_or(0);
                            let _ = reply.send(Ok(serde_json::json!({
                                "snapshot": serde_json::to_value(&payload)
                                    .unwrap_or(serde_json::Value::Null),
                                "bytes": bytes,
                            })));
                        }
                        Job::Restore { payload, reply } => {
                            // ⚠️ `restore_session` **校验先于应用**：畸形载荷一个字节都不
                            //    会改（第 0 步 validate 失败即返回）⇒ ⇒ 失败时如实报码。
                            let _ = reply.send(match host.restore_session(&payload) {
                                Ok(()) => Ok(serde_json::json!({
                                    "restored": true,
                                    "pc": host.pc(),
                                    "executed": host.total_executed(),
                                })),
                                Err(err) => Err(KernelFailure {
                                    code: format!("{}-{:02}", err.family.as_str(), err.scenario),
                                    message: err.message,
                                }),
                            });
                        }
                        Job::Events {
                            since,
                            limit,
                            reply,
                        } => {
                            let (frames, next, lost) = events_ring.since(since, limit);
                            let _ = reply.send(Ok(serde_json::json!({
                                "frames": frames,
                                "next": next,
                                "lost": lost,
                            })));
                        }
                        Job::State { reply } => {
                            let _ = reply.send(Ok(serde_json::json!({
                                "pc": host.pc(),
                                "registers": host.registers(),
                                "executed": host.total_executed(),
                                "t_ps": host.now_ps(),
                            })));
                        }
                        Job::Symbols { limit, reply } => {
                            let symbols = host.list_symbols(limit);
                            let _ = reply.send(Ok(serde_json::json!({
                                "symbols": symbols,
                                "returned": symbols.len(),
                                "limit": limit,
                            })));
                        }
                        Job::Debug { command, reply } => {
                            let events = host.handle_message(&Message::command(1, command.clone()));
                            // ⚠️ 断点族里 `SetBreakpoint` / `ClearBreakpoint` 的即时
                            //   回执只有 `Tick`（内核 v13 起表只由 `ListBreakpoints`
                            //   给）⇒ ⇒ **另问一次**表，回执才是「现在真实有什么」。
                            let table = breakpoint_family(&command).then(|| {
                                host.handle_message(&Message::command(
                                    1,
                                    CommandPayload::ListBreakpoints,
                                ))
                            });
                            events_ring.push(&events);
                            if let Some(table) = table.as_deref() {
                                events_ring.push(table);
                            }
                            let _ =
                                reply.send(debug_reply(&command, &host, &events, table.as_deref()));
                        }
                        Job::Contract {
                            message,
                            req_id,
                            reply,
                        } => {
                            // ⚠️ 整条消息原样进内核：`handle_message` 自己判
                            //   `schema_version` / 命令变体 ⇒ ⇒ 服务端不造
                            //   第二条判据（错也是内核的一条 `Error` 事件）。
                            let events = host.handle_message(&message);
                            let frames = events_ring.push_answer(&events, req_id);
                            let _ = reply.send(Ok(serde_json::json!({ "frames": frames })));
                        }
                        Job::SetBoard {
                            description,
                            expected_revision,
                            reply,
                        } => {
                            let events = host.handle_message(&Message::command(
                                1,
                                CommandPayload::SetMemoryMap {
                                    description: *description,
                                    expected_revision,
                                },
                            ));
                            events_ring.push(&events);
                            let _ = reply.send(board_reply(&host, &events));
                        }
                        Job::Load {
                            name,
                            kind,
                            bytes,
                            reply,
                        } => {
                            // ⚠️ 一次请求 = 一个单镜像工程。工程 id 用固定前缀
                            //   ⇒ ⇒ 内核里「最后一个工程」只在会话线程里，不
                            //   会跨会话串味。
                            let project_name = name.clone();
                            let project = Project {
                                id: "m24-load".to_string(),
                                name: name.clone(),
                                tree: Default::default(),
                                images: vec![ImageRef {
                                    name,
                                    kind,
                                    source: ImageSource::Bytes(bytes.clone()),
                                }],
                                version: ProjectVersion::default(),
                            };
                            let events = host.handle_message(&Message::command(
                                1,
                                CommandPayload::LoadProject(project),
                            ));
                            events_ring.push(&events);
                            let result = load_reply(&host, &events);
                            // ⚠️ **`LoadProject` 会清空订阅表**（内核语义：新工程 ⇒
                            //    旧订阅全部失效，客户端必须重订）。所以装载成功之后
                            //    必须**立刻重订** `sim.state.delta`，否则此后每个运行
                            //    窗口又只剩 `Tick` —— 事件流静默变空。
                            if result.is_ok() {
                                let restored = host.handle_message(&Message::command(
                                    2,
                                    CommandPayload::Subscribe {
                                        topic: TOPIC_STATE_DELTA.to_string(),
                                    },
                                ));
                                events_ring.push(&restored);
                                // ---- D-3：换固件 ⇒ console 清内容、偏移不归零 ----
                                //
                                // ⚠️ 不清的话上一份固件的尾巴会留在 `[0, total)` 里
                                //   ⇒ ⇒ 客户端从头读会读成「这一份的输出」，误判成
                                //   「新固件没打印」。偏移继续记账 ⇒ ⇒ 旧 offset 回
                                //   `E_SESS-04` + base，客户端据此重新对齐。
                                console.clear();
                                // 记住这份工程：复位（R-14）要原样重装一遍。
                                last_image = Some((project_name, kind, bytes));
                            }
                            let _ = reply.send(result);
                        }
                        Job::Reset { reply } => {
                            // ⚠️ 没有记住的镜像 ⇒ ⇒ 如实拒绝（不是 200 空跑）。
                            match last_image.clone() {
                                None => {
                                    let _ = reply.send(Err(KernelFailure {
                                        code: "E_SESS-02".to_string(),
                                        message: "会话还没有装载过镜像，没有可复位的工程"
                                            .to_string(),
                                    }));
                                }
                                Some((name, kind, bytes)) => {
                                    let project = Project {
                                        id: "m24-load".to_string(),
                                        name: name.clone(),
                                        tree: Default::default(),
                                        images: vec![ImageRef {
                                            name,
                                            kind,
                                            source: ImageSource::Bytes(bytes),
                                        }],
                                        version: ProjectVersion::default(),
                                    };
                                    let events = host.handle_message(&Message::command(
                                        1,
                                        CommandPayload::LoadProject(project),
                                    ));
                                    events_ring.push(&events);
                                    let result = load_reply(&host, &events);
                                    if result.is_ok() {
                                        let restored = host.handle_message(&Message::command(
                                            2,
                                            CommandPayload::Subscribe {
                                                topic: TOPIC_STATE_DELTA.to_string(),
                                            },
                                        ));
                                        events_ring.push(&restored);
                                    }
                                    // ⚠️ console 环**不裁**（R-06：只追加不改写）⇒ ⇒ 复位
                                    //    之后新一段从 `total` 起；回执把 `base/total` 一起
                                    //    给出去，客户端据此对齐，不必猜。
                                    let _ = reply.send(result.map(|mut loaded| {
                                        loaded["restored"] = serde_json::json!(true);
                                        loaded["console"] = serde_json::json!({
                                            "base": console.base,
                                            "total": console.total,
                                        });
                                        loaded
                                    }));
                                }
                            }
                        }
                        Job::Watchpoints { reply } => {
                            let watchpoints = host.list_watchpoints();
                            let _ = reply.send(Ok(serde_json::json!({
                                "watchpoints": watchpoints,
                                "returned": watchpoints.len(),
                            })));
                        }
                        Job::SetWatchpoint { watchpoint, reply } => {
                            let events = host.handle_message(&Message::command(
                                1,
                                CommandPayload::SetWatchpoint(watchpoint),
                            ));
                            events_ring.push(&events);
                            let _ = reply.send(match kernel_failure(&events) {
                                Some(failure) => Err(failure),
                                None => {
                                    let watchpoints = host.list_watchpoints();
                                    Ok(serde_json::json!({
                                        "watchpoints": watchpoints,
                                        "returned": watchpoints.len(),
                                    }))
                                }
                            });
                        }
                        Job::ClearWatchpoint { id, reply } => {
                            let events = host.handle_message(&Message::command(
                                1,
                                CommandPayload::ClearWatchpoint { id },
                            ));
                            events_ring.push(&events);
                            let _ = reply.send(match kernel_failure(&events) {
                                Some(failure) => Err(failure),
                                None => {
                                    let watchpoints = host.list_watchpoints();
                                    Ok(serde_json::json!({
                                        "watchpoints": watchpoints,
                                        "returned": watchpoints.len(),
                                    }))
                                }
                            });
                        }
                    }
                    busy.store(false, Ordering::Release);
                    if let Ok(mut instant) = last_active.lock() {
                        *instant = Instant::now();
                    }
                }
            }));
            alive.store(false, Ordering::Release);
            if outcome.is_err() {
                // 线程名字里带 id ⇒ ⇒ 这条日志能对上是哪条会话。
                eprintln!("riscv_sim_server: session worker {id} panicked; marker set to poisoned");
            }
        })
        .expect("spawning a session thread must not fail on a healthy process")
}

#[cfg(test)]
mod tests {
    use super::*;
    // ⚠️ 与 `RunUntil::Breakpoint` 同名但不同物：这里要的是契约的断点结构。
    use riscv_sim_contract::model::Breakpoint as ContractBreakpoint;

    fn registry() -> SessionRegistry {
        SessionRegistry::new(2, Duration::from_millis(50), 1024)
    }

    fn delta() -> EventPayload {
        EventPayload::SimStateDelta {
            registers: Vec::new(),
            pc: Some(0),
            t_ps: 0,
        }
    }

    #[test]
    fn async_run_reports_through_a_job_slot() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        // ⚠️ 必须先装一个**真的会跑**的镜像：空工程跑 1 条就 `end-of-execution`
        //    ⇒ ⇒ 「恰好 n 条」这条断言会假红。这里用 4 字节自跳
        //    （`jal x0, 0` = `6f 00 00 00`）。
        registry
            .load_image(
                &id,
                "spin.bin".to_string(),
                ImageKind::RawHex,
                vec![0x6f, 0, 0, 0],
            )
            .expect("装载自跳镜像");
        let submitted = registry
            .run_async(
                &id,
                RunSpec {
                    steps: Some(2),
                    ..RunSpec::default()
                },
            )
            .expect("空闲会话必须能提交异步 run");
        let job_id = submitted
            .get("job_id")
            .and_then(|value| value.as_str())
            .expect("回执必须带 job_id")
            .to_string();

        // 轮询到结束（上限给 2 s，正常是毫秒级）。
        let mut state = String::new();
        for _ in 0..100 {
            let body = registry.job(&id, &job_id).expect("刚提交的作业必须查得到");
            state = body
                .get("state")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            if state != "running" {
                assert_eq!(state, "done", "作业只能 running → done/failed");
                assert_eq!(
                    body.get("report")
                        .and_then(|report| report.get("executed"))
                        .and_then(|value| value.as_u64()),
                    Some(2),
                    "StepN 语义是「恰好 n 条」"
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(state, "done", "异步作业 2 s 内必须跑完");

        // 不存在的作业 ⇒ 404（`E_SESS-01`），不是空 200。
        assert!(registry.job(&id, "job-9999").is_err());
    }

    #[test]
    fn snapshot_then_restore_round_trips_a_session() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        // 采集：六段齐全，宿主代填 `t_virt`。
        let taken = registry
            .snapshot(&id, CheckpointMetaDto::default())
            .expect("fresh session 必须能采集快照");
        let payload = taken.get("snapshot").expect("回执必须带 snapshot").clone();
        assert!(payload.get("m12_engine").is_some());
        assert!(payload.get("m13_state").is_some());

        // 还原：同一份载荷必须能被吃回去（校验通过 ⇒ 应用）。
        let restored: SessionSnapshotDto =
            serde_json::from_value(payload).expect("回执必须是合法的 SessionSnapshotDto");
        let reply = registry
            .restore(&id, Box::new(restored))
            .expect("合法快照必须能还原");
        assert_eq!(reply.get("restored"), Some(&serde_json::Value::Bool(true)));

        // 畸形载荷 ⇒ **失败**（而不是静默接受）。
        let broken = SessionSnapshotDto {
            schema_version: 999,
            ..SessionSnapshotDto::default()
        };
        assert!(registry.restore(&id, Box::new(broken)).is_err());
    }

    #[test]
    fn event_ring_skips_ticks_and_caps_itself() {
        let mut ring = EventRing::new(2, "s1");
        // `Tick` 是心跳 ⇒ **不进流**（不进环、也不占 seq）。
        ring.push(&[EventPayload::Tick {
            elapsed_ps: 1,
            pc: 0,
        }]);
        assert_eq!(ring.since(0, 8).0.len(), 0);
        assert_eq!(ring.since(0, 8).1, 0);

        ring.push(&[delta(), delta(), delta()]);
        // 环满从旧的一端栽 ⇒ ⇒ 只留最新两条。
        let (frames, next, lost) = ring.since(0, 8);
        assert_eq!(frames.len(), 2);
        assert_eq!(next, 3);
        // 要过被挤掉的那一条 ⇒ ⇒ `lost` 必须立起来（不能静默跳号）。
        assert!(lost);
        // 从环里最旧的那条起读 ⇒ ⇒ 没丢。
        assert!(!ring.since(1, 8).2);
    }

    #[test]
    fn create_lists_and_probes_a_session() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("minimal".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        assert_eq!(info.board.as_deref(), Some("minimal"));
        assert_eq!(info.board_source, BoardSource::Request);
        assert_eq!(info.schema_version, SCHEMA_VERSION);

        // `list_for` 报的是**这个账号**的上限（这里没设 ⇒ 无上限），不是全局 2。
        let (infos, _max, used) = registry.list_for(crate::account::LEGACY_ACCOUNT);
        assert_eq!(used, 1);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, info.id);

        let (probed, pc, executed) = registry.probe(&info.id).unwrap();
        assert_eq!(probed.id, info.id);
        assert_eq!(pc, 0, "P0-2 还没有镜像 ⇒ pc 为 0");
        assert_eq!(executed, 0);
    }

    #[test]
    fn unknown_session_is_not_found_not_busy() {
        let registry = registry();
        let err = registry.probe("sess-deadbeef").unwrap_err();
        assert_eq!(err.code(), "E_SESS-01");
        assert_eq!(err.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn quota_rejects_the_third_session() {
        let registry = registry();
        registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap();
        registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap();
        let err = registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap_err();
        assert_eq!(err.code(), "E_SESS-05");
        assert_eq!(err.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(registry.used(), 2);
    }

    #[test]
    fn delete_removes_the_session_and_a_second_delete_is_not_found() {
        let registry = registry();
        let info = registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap();
        registry.delete(&info.id).unwrap();
        assert_eq!(registry.used(), 0);
        let err = registry.delete(&info.id).unwrap_err();
        assert_eq!(err.code(), "E_SESS-01");
    }

    #[test]
    fn ttl_reaps_idle_sessions() {
        let registry = SessionRegistry::new(4, Duration::from_millis(20), 1024);
        let info = registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(registry.sweep(), 1);
        assert_eq!(registry.used(), 0);
        assert_eq!(registry.probe(&info.id).unwrap_err().code(), "E_SESS-01");
    }

    #[test]
    fn shutdown_reaps_everything() {
        let registry = registry();
        registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap();
        registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap();
        registry.shutdown();
        assert_eq!(registry.used(), 0);
    }

    /// **环的上限只裁旧字节**：新字节必须留住，`base` 随裁掉的量推进。
    #[test]
    fn console_ring_keeps_the_newest_bytes_and_advances_the_base() {
        let mut ring = ConsoleRing::new(4);
        ring.push(b"abcdef");
        assert_eq!(ring.total, 6, "total 数的是**写过**的全部，不是现在还在的");
        assert_eq!(ring.base, 2, "裁掉 2 个 ⇒ base 推到 2");
        let bytes = ring.read(2, 8).expect("窗口内必须能读");
        assert_eq!(bytes, b"cdef");
        // 早于 base 的偏移 ⇒ 报 base（客户端据此重新对齐）。
        let lost = ring.read(0, 8).expect_err("丢失的窗口必须报错");
        assert_eq!(lost, 2);
        // 晚于 total 的偏移 ⇒ 空，不是错（只是还没产生）。
        assert_eq!(ring.read(6, 8).expect("未产生不算丢"), b"");
    }

    /// D-3：`clear` 清内容但**不**归零偏移 ⇒ ⇒ 旧 offset 被如实拒绝。
    #[test]
    fn console_ring_clear_keeps_the_offset_accounting() {
        let mut ring = ConsoleRing::new(64);
        ring.push(b"old");
        ring.clear();
        assert_eq!(ring.total, 3, "total 是**写过**的记账，不因清空而回退");
        assert_eq!(ring.base, 3, "base 推到 total ⇒ 旧窗口全部作废");
        // 旧 offset ⇒ 报 base（客户端重新对齐），不是「变空了」。
        assert_eq!(ring.read(0, 8).expect_err("旧窗口必须报错"), 3);
        // 从新 base 起读 ⇒ 只有新字节。
        ring.push(b"new");
        assert_eq!(ring.read(3, 8).expect("新窗口"), b"new");
        assert_eq!(ring.read(6, 8).expect("未产生不算丢"), b"");
    }

    /// `limit` 小于剩余量时**只给 limit**，且不越界。
    #[test]
    fn console_ring_read_respects_the_limit() {
        let mut ring = ConsoleRing::new(64);
        ring.push(b"0123456789");
        assert_eq!(ring.read(3, 2).expect("窗口内"), b"34");
        assert_eq!(ring.read(9, 99).expect("窗口内"), b"9");
    }

    /// 停因词表：**宿主侧四种**停因必须与契约 `HaltReason` 分开拼写。
    #[test]
    fn stop_label_separates_host_stops_from_guest_halts() {
        assert_eq!(
            stop_label(RunStop::Halted(HaltReason::Breakpoint)),
            "breakpoint"
        );
        assert_eq!(stop_label(RunStop::Halted(HaltReason::Trap)), "trap");
        assert_eq!(stop_label(RunStop::Steps), "steps");
        assert_eq!(stop_label(RunStop::Budget), "budget");
        assert_eq!(stop_label(RunStop::Timeout), "timeout");
        // `UserPause` 与宿主 `Paused` **同一个词**：调用方只关心「停了」。
        assert_eq!(stop_label(RunStop::Halted(HaltReason::UserPause)), "pause");
        assert_eq!(stop_label(RunStop::Paused), "pause");
    }

    /// 前缀停因（R-05）：console 里出现 `prefix` ⇒ `stop_reason = "prefix"`。
    ///
    /// 程序（4 条，RawHex）：`lui t0,0x10000` → `addi t1,x0,65` →
    /// `sb t1,0(t0)`（写 UART THR）→ `jal x0,0`（自跳）。板用 `qemu-virt`
    /// （UART0 @ 0x1000_0000）。
    #[test]
    fn run_stops_on_the_console_prefix() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("qemu-virt".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        let program = vec![
            0xb7, 0x02, 0x00, 0x10, // lui t0, 0x10000
            0x13, 0x03, 0x10, 0x04, // addi t1, x0, 65 ('A')
            0x23, 0x80, 0x62, 0x00, // sb t1, 0(t0)
            0x6f, 0x00, 0x00, 0x00, // jal x0, 0
        ];
        registry
            .load_image(&id, "puts.bin".to_string(), ImageKind::RawHex, program)
            .expect("装载打印镜像");
        let report = registry
            .run(
                &id,
                RunSpec {
                    prefix: Some("A".to_string()),
                    timeout_ms: Some(1_000),
                    ..RunSpec::default()
                },
            )
            .expect("前缀 run 必须成功");
        assert_eq!(
            report.get("stop_reason").and_then(|v| v.as_str()),
            Some("prefix"),
            "console 出现 `A` 就该停在前缀：{report}"
        );
        assert_eq!(
            report
                .get("console")
                .and_then(|c| c.get("bytes"))
                .and_then(|b| b.as_u64()),
            Some(1),
            "回体必须报出本次新增的 1 个字节：{report}"
        );
        // R-05：回体带虚拟时间。
        assert!(report.get("t_ps").and_then(|v| v.as_u64()).is_some());
    }

    /// B-2 异常连环守卫：内核**主动中止**这一批时，回体必须说 `error` + 错误码，
    /// 不许只说 `halt`。
    ///
    /// ⚠️ 背景：`RunStop` 里没有 `Error` 变体（错误只走事件流的 `Error`），
    /// 窗口循环随后补一个 `Halted(EndOfExecution)` ⇒ ⇒ 不翻译的话客户端会把
    /// 「平台中止（E_ENG-04）」读成「guest 停下来了」。
    #[test]
    fn run_reports_a_platform_error_abort_instead_of_a_halt() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("qemu-virt".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        // 非法指令 @0x0c ⇒ handler 在 0x10 也非法 ⇒ 同一 (cause, mepc) 连环。
        let program = vec![
            0x97, 0x02, 0x00, 0x00, // 0x00  auipc x5, 0
            0x93, 0x82, 0x02, 0x01, // 0x04  addi  x5, x5, 16
            0x73, 0x90, 0x52, 0x30, // 0x08  csrrw x0, mtvec, x5
            0x00, 0x00, 0x00, 0x00, // 0x0c  非法指令
            0x00, 0x00, 0x00, 0x00, // 0x10  非法指令（handler）
        ];
        registry
            .load_image(&id, "storm.bin".to_string(), ImageKind::RawHex, program)
            .expect("装载陷阱连环镜像");
        registry
            .contract(
                &id,
                Message::command(
                    0,
                    CommandPayload::SetStopPolicy {
                        stop_on_trap: false,
                        max_trap_events: Some(1_000),
                        stall_guard: Some(3),
                    },
                ),
                Some(1),
            )
            .expect("设策略必须成功");

        let report = registry
            .run(
                &id,
                RunSpec {
                    steps: Some(1_000),
                    ..RunSpec::default()
                },
            )
            .expect("run 必须返回（中止是回体信息，不是 HTTP 错误）");
        assert_eq!(
            report.get("stop_reason").and_then(|v| v.as_str()),
            Some("error"),
            "平台中止必须报 error，不许伪装成 halt：{report}"
        );
        assert_eq!(
            report.get("error_code").and_then(|v| v.as_str()),
            Some("E_ENG-04"),
            "回体要带内核的错误码：{report}"
        );
        assert!(
            report
                .get("executed")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX)
                < 10,
            "连环守卫必须远早于配额（1000）就停：{report}"
        );
    }

    /// 复位（R-14）：回到装载起点 —— pc 回 entry、已执行条数归零。
    #[test]
    fn reset_returns_to_the_loading_start() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        registry
            .load_image(
                &id,
                "spin.bin".to_string(),
                ImageKind::RawHex,
                vec![0x6f, 0, 0, 0],
            )
            .expect("装载自跳镜像");
        registry
            .run(
                &id,
                RunSpec {
                    steps: Some(10),
                    ..RunSpec::default()
                },
            )
            .expect("先跑 10 条");
        let state_before = registry.state(&id).expect("读状态");
        assert_eq!(
            state_before.get("executed").and_then(|v| v.as_u64()),
            Some(10)
        );
        let reset = registry.reset(&id).expect("复位必须成功");
        assert_eq!(reset.get("restored").and_then(|v| v.as_bool()), Some(true));
        let state_after = registry.state(&id).expect("复位后读状态");
        assert_eq!(
            state_after.get("pc").and_then(|v| v.as_u64()),
            Some(0),
            "复位后 pc 回到 entry"
        );
        assert_eq!(
            state_after.get("executed").and_then(|v| v.as_u64()),
            Some(0),
            "复位后计数归零"
        );
    }

    /// 没装过镜像就没有「当前工程」可复位 ⇒ 如实拒绝（不是 200 空跑）。
    #[test]
    fn reset_without_an_image_is_rejected() {
        let registry = registry();
        let info = registry
            .create_for(None, BoardSource::None, crate::account::LEGACY_ACCOUNT)
            .unwrap();
        let err = registry.reset(&info.id).expect_err("空会话不能复位");
        assert_eq!(err.status(), axum::http::StatusCode::CONFLICT);
    }

    /// 观察点（R-08）：设置 / 列出 / 清除，全部走内核的表。
    #[test]
    fn watchpoints_go_through_the_kernel_table() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        let wp = Watchpoint {
            id: 7,
            addr: 0x8000_0000,
            size: riscv_sim_contract::model::WriteSize::Word,
            access: riscv_sim_contract::model::WatchAccess::Write,
        };
        let table = registry.set_watchpoint(&id, wp).expect("设置观察点");
        let listed = table.get("watchpoints").and_then(|v| v.as_array()).cloned();
        assert_eq!(listed.map(|list| list.len()), Some(1), "{table}");
        assert_eq!(
            table
                .get("watchpoints")
                .and_then(|v| v.as_array())
                .and_then(|list| list.first())
                .and_then(|wp| wp.get("id"))
                .and_then(|v| v.as_u64()),
            Some(7)
        );
        let cleared = registry.clear_watchpoint(&id, 7).expect("清除观察点");
        assert_eq!(
            cleared
                .get("watchpoints")
                .and_then(|v| v.as_array())
                .map(|list| list.len()),
            Some(0)
        );
    }

    /// 事件帧就是 M-07 帧（R-13）：`{type,seq,sessionId,role,reqId,payload}`，
    /// 且 `payload` 是**完整的**契约 `Message`（带 `schema_version` /
    /// `direction` / `seq`），**不是**只取里面那一层 `{"Event":…}`。
    ///
    /// ⚠️ 这条断言是**按前端解码器**写的：`front/lib/message.js` 的
    /// `decodeMessage` 第一件事就是读 `schema_version` ⇒ ⇒ 少了信封就是整帧
    /// `E_PROTO-08`。曾经这里只断言「`Event` 键在」，于是服务端发裸内层、
    /// 前端一帧也解不开，冒烟**照样绿**。
    #[test]
    fn event_frames_use_the_m07_envelope() {
        let mut ring = EventRing::new(4, "sess-x");
        ring.push(&[delta()]);
        let (frames, next, _) = ring.since(0, 8);
        assert_eq!(next, 1);
        let frame = frames.first().expect("必须有帧");
        assert_eq!(frame.get("type").and_then(|v| v.as_str()), Some("event"));
        assert_eq!(frame.get("seq").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(
            frame.get("sessionId").and_then(|v| v.as_str()),
            Some("sess-x")
        );
        assert!(frame.get("role").map(|v| v.is_null()).unwrap_or(false));
        assert!(frame.get("reqId").map(|v| v.is_null()).unwrap_or(false));
        let payload = frame.get("payload").expect("payload 必须在");
        assert_eq!(
            payload.get("schema_version").and_then(|v| v.as_u64()),
            Some(riscv_sim_contract::SCHEMA_VERSION as u64),
            "payload 必须是完整信封：前端 decodeMessage 先读 schema_version"
        );
        assert_eq!(
            payload.get("direction").and_then(|v| v.as_str()),
            Some("Event")
        );
        assert_eq!(payload.get("seq").and_then(|v| v.as_u64()), Some(0));
        assert!(
            payload
                .get("payload")
                .and_then(|inner| inner.get("Event"))
                .is_some(),
            "信封里面才是 `{{\"Event\":…}}`：{payload}"
        );
    }

    /// 契约命令（v1.11）：回体必须是前端 `handleFrame` 能直接解的那种帧。
    ///
    /// 判据按前端写：`decodeFrame` 要 `type` / `seq` / `payload`，`decodeMessage`
    /// 要 `schema_version`，M-05 认**第一条**帧的 `reqId`。
    #[test]
    fn contract_command_answers_with_frames_the_front_end_can_decode() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        registry
            .load_image(
                &id,
                "spin.bin".to_string(),
                ImageKind::RawHex,
                vec![0x6f, 0, 0, 0],
            )
            .expect("装载自跳镜像");

        let body = registry
            .contract(
                &id,
                Message::command(0, CommandPayload::StepN { n: 1 }),
                Some(7),
            )
            .expect("契约命令必须成功");
        let frames = body
            .get("frames")
            .and_then(|value| value.as_array())
            .expect("回体必须是 frames 数组");
        assert!(!frames.is_empty(), "一条命令至少要有一条事件");

        let first = &frames[0];
        assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("event"));
        assert_eq!(
            first.get("reqId").and_then(|v| v.as_u64()),
            Some(7),
            "首帧必须回贴配对号（M-05 按它配对）"
        );
        assert_eq!(
            first.get("sessionId").and_then(|v| v.as_str()),
            Some(id.as_str())
        );
        assert!(first.get("seq").and_then(|v| v.as_u64()).is_some());
        let payload = first.get("payload").expect("payload 必须在");
        assert_eq!(
            payload.get("schema_version").and_then(|v| v.as_u64()),
            Some(riscv_sim_contract::SCHEMA_VERSION as u64),
            "完整信封：前端 decodeMessage 先读 schema_version"
        );
        assert_eq!(
            payload.get("direction").and_then(|v| v.as_str()),
            Some("Event")
        );
        assert!(
            payload
                .get("payload")
                .and_then(|inner| inner.get("Event"))
                .is_some(),
            "信封里面才是 `{{\"Event\":…}}`：{payload}"
        );
        assert_eq!(
            frames.len(),
            1,
            "回体只有**应答**那一帧；其余事件走事件流（不重复投递）"
        );
    }

    /// 契约命令的心跳应答（`Tick`）**不进环**，但必须回得去：它的帧用保留
    /// 号段（[`UNRINGED_SEQ_BASE`] 起）⇒ ⇒ 与环里的 seq 不撞号，前端的
    /// 「同会话同一 seq 只应用一次」不会误删真事件。
    #[test]
    fn contract_tick_answer_uses_the_unringed_seq_band() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();

        let body = registry
            .contract(
                &id,
                Message::command(
                    0,
                    CommandPayload::SetBreakpoint(ContractBreakpoint {
                        id: 0x4,
                        addr: 0x4,
                        ..ContractBreakpoint::default()
                    }),
                ),
                Some(9),
            )
            .expect("下断点必须成功");
        let frames = body
            .get("frames")
            .and_then(|value| value.as_array())
            .expect("回体必须是 frames 数组");
        assert_eq!(frames.len(), 1, "SetBreakpoint 的即时应答只有 Tick");
        assert_eq!(frames[0].get("reqId").and_then(|v| v.as_u64()), Some(9));
        let seq = frames[0]
            .get("seq")
            .and_then(|v| v.as_u64())
            .expect("帧 seq 必须在");
        assert!(
            seq >= UNRINGED_SEQ_BASE,
            "Tick 应答必须走保留号段（{seq} < {UNRINGED_SEQ_BASE}）"
        );
    }

    /// 应答帧**不进**事件流：一条命令产生的事件只有一条路到页面 ⇒ ⇒
    /// 前端不需要去重，「同一条状态增量算两遍」不可能发生；同时其余事件
    /// （状态增量 / console / 波形）照旧进环 ⇒ ⇒ 观察端不会瞎。
    #[test]
    fn contract_answer_is_not_duplicated_in_the_event_stream() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        registry
            .load_image(
                &id,
                "addi.bin".to_string(),
                ImageKind::RawHex,
                // `addi x5, x0, 1`（pc 会从 0 走到 4 ⇒ 一定产生状态增量）
                vec![0x93, 0x02, 0x10, 0x00],
            )
            .expect("装载一条 addi");

        let before_next = registry.events(&id, 0, 64).unwrap()["next"]
            .as_u64()
            .expect("next");
        let body = registry
            .contract(
                &id,
                Message::command(0, CommandPayload::StepN { n: 1 }),
                Some(11),
            )
            .expect("契约命令必须成功");
        let frames = body["frames"].as_array().expect("frames 数组").clone();
        assert_eq!(frames.len(), 1);
        let answer_seq = frames[0]["seq"].as_u64().expect("应答帧 seq");

        let tail = registry.events(&id, before_next, 64).expect("读事件流");
        let stream = tail["frames"].as_array().expect("事件流的 frames");
        assert!(
            !stream
                .iter()
                .any(|frame| frame["seq"].as_u64() == Some(answer_seq)),
            "应答帧不许在事件流里出现第二份：{tail}"
        );
        assert!(
            stream
                .iter()
                .any(|frame| frame["payload"]["payload"]["Event"]
                    .get("SimStateDelta")
                    .is_some()),
            "真正对观察者有意义的状态增量必须留在事件流里：{tail}"
        );
        for frame in stream {
            assert!(
                frame.get("reqId").map(|v| v.is_null()).unwrap_or(false),
                "事件流里的帧不带任何人的配对号：{frame}"
            );
        }
    }

    /// 契约路径的 `LoadProject` **不代订** topic：内核装载会清空订阅，重订是
    /// 客户端的事（与本地 Worker 模式同一条口径）。REST 装载路径的
    /// 「代订 `sim.state.delta`」只服务不认识契约的客户端。
    #[test]
    fn contract_load_project_leaves_resubscription_to_the_client() {
        let registry = registry();
        let info = registry
            .create_for(
                Some("default".to_string()),
                BoardSource::Request,
                crate::account::LEGACY_ACCOUNT,
            )
            .unwrap();
        let id = info.id.clone();
        let before_next = registry.events(&id, 0, 64).unwrap()["next"]
            .as_u64()
            .expect("next");

        let project = Project {
            id: "m24-contract".to_string(),
            name: "spin.bin".to_string(),
            tree: Default::default(),
            images: vec![ImageRef {
                name: "spin.bin".to_string(),
                kind: ImageKind::RawHex,
                source: ImageSource::Bytes(vec![0x6f, 0, 0, 0]),
            }],
            version: ProjectVersion::default(),
        };
        registry
            .contract(
                &id,
                Message::command(0, CommandPayload::LoadProject(project)),
                Some(3),
            )
            .expect("契约装载必须成功");

        let tail = registry.events(&id, before_next, 64).expect("读事件流");
        for frame in tail["frames"].as_array().unwrap() {
            let event = &frame["payload"]["payload"]["Event"];
            assert!(
                event.get("Subscribed").is_none(),
                "契约路径不许代订任何 topic：{frame}"
            );
        }

        // 内核的订阅已被 `LoadProject` 清空 ⇒ ⇒ 再走一步**没有**状态增量
        // （事件流里只剩心跳应答之外的东西）。这一条就是「页面必须自己重订」
        // 的证据。
        let step_next = registry.events(&id, 0, 64).unwrap()["next"]
            .as_u64()
            .expect("next");
        registry
            .contract(
                &id,
                Message::command(1, CommandPayload::StepN { n: 1 }),
                Some(4),
            )
            .expect("契约步进必须成功");
        let after_step = registry.events(&id, step_next, 64).expect("读事件流");
        let stream = after_step["frames"].as_array().unwrap();
        assert!(
            stream.is_empty(),
            "没订阅 ⇒ 这一步只会有心跳应答（不进环）：{after_step}"
        );
        for frame in stream {
            assert!(
                frame["payload"]["payload"]["Event"]
                    .get("SimStateDelta")
                    .is_none(),
                "没订阅就不该有增量：{frame}"
            );
        }
    }
}
