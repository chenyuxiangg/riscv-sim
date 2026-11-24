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

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use riscv_sim_contract::SCHEMA_VERSION;
use riscv_sim_kernel::kernel_host::KernelHost;

use crate::error::ApiError;

/// 控制位：暂停。推进者在窗口边界读到它就停，不回滚已完成的窗口。
pub const CTRL_PAUSE: u8 = 1 << 0;
/// 控制位：取消（删除 / TTL 回收 / 关进程）。置上后线程不再接新活。
pub const CTRL_CANCEL: u8 = 1 << 1;

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
enum Job {
    /// 探活：证明「线程还在、状态还能取」。
    Ping {
        /// 回执通道。线程侧只发一次。
        reply: Sender<PingReply>,
    },
}

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
    /// 生效的板模板 id。
    pub board: Option<String>,
    /// 板从哪来。
    pub board_source: BoardSource,
    /// 创建时刻。
    pub created: Instant,
    tx: Sender<Job>,
    ctrl: Arc<AtomicU8>,
    busy: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    last_active: Arc<Mutex<Instant>>,
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
    fn join_within(self, timeout: Duration) {
        // `JoinHandle` 没有带超时的 join ⇒ ⇒ 用「等标志 + 短睡」实现，
        // 超时就放弃 join 并让线程继续自生自灭（它已经看不到命令面了）。
        let deadline = Instant::now() + timeout;
        while self.is_alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if !self.is_alive() {
            let _ = self.join.join();
        }
    }

    fn info(&self) -> SessionInfo {
        SessionInfo {
            id: self.id.clone(),
            board: self.board.clone(),
            board_source: self.board_source,
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
        if self.busy.load(Ordering::Acquire) {
            return Err(ApiError::session_busy(&self.id));
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
}

/// 会话表。**唯一**的可变状态是表本身；会话状态全在各自线程里。
pub struct SessionRegistry {
    inner: Arc<Mutex<Inner>>,
    max_sessions: usize,
    ttl: Duration,
}

struct Inner {
    next: u64,
    sessions: HashMap<String, SessionHandle>,
}

impl SessionRegistry {
    /// 建表。`max_sessions` 是并发上限（配额），`ttl` 是空闲回收窗口。
    pub fn new(max_sessions: usize, ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next: 1,
                sessions: HashMap::new(),
            })),
            max_sessions,
            ttl,
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
    pub fn create(
        &self,
        board: Option<String>,
        board_source: BoardSource,
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

        let seq = inner.next;
        inner.next += 1;
        let id = format!("sess-{seq:06x}");

        let (tx, rx) = mpsc::channel();
        let ctrl = Arc::new(AtomicU8::new(0));
        let busy = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let last_active = Arc::new(Mutex::new(Instant::now()));

        let handle = SessionHandle {
            id: id.clone(),
            board: board.clone(),
            board_source,
            created: Instant::now(),
            tx,
            ctrl: Arc::clone(&ctrl),
            busy: Arc::clone(&busy),
            alive: Arc::clone(&alive),
            last_active: Arc::clone(&last_active),
            join: spawn_worker(id.clone(), rx, ctrl, busy, alive, last_active),
        };
        let info = handle.info();
        inner.sessions.insert(id, handle);
        Ok(info)
    }

    /// 列表：还活着的会话 + 配额用量。
    pub fn list(&self) -> (Vec<SessionInfo>, usize, usize) {
        let inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return (Vec::new(), 0, self.max_sessions),
        };
        let mut infos: Vec<SessionInfo> = inner
            .sessions
            .values()
            .filter(|handle| handle.is_alive())
            .map(SessionHandle::info)
            .collect();
        infos.sort_by(|a, b| a.id.cmp(&b.id));
        (infos, self.max_sessions, inner.sessions.len())
    }

    /// 取一条会话（探活一次，证明不是「表里有、线程已死」）。
    pub fn probe(&self, id: &str) -> Result<(SessionInfo, u32, u64), ApiError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::session_poisoned("registry"))?;
        let handle = guard
            .sessions
            .get(id)
            .ok_or_else(|| ApiError::session_not_found(id))?;
        let (pc, executed) = handle.ping()?;
        Ok((handle.info(), pc, executed))
    }

    /// 置暂停位（控制面动作，**不排队**）。会话已死 ⇒ 500 + `E_SESS-03`。
    ///
    /// ⚠️ 这里**不**等「真的停下来」：P0-4 的推进者会在下一个窗口边界响应，
    /// 等它停下是 `run` 的回执该回答的事，不是 `pause` 该等的（否则
    /// `pause` 会被一次长 `run` 卡住，正是控制器面的死角）。
    pub fn pause(&self, id: &str) -> Result<SessionInfo, ApiError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::session_poisoned("registry"))?;
        let handle = guard
            .sessions
            .get(id)
            .ok_or_else(|| ApiError::session_not_found(id))?;
        if !handle.is_alive() {
            return Err(ApiError::session_poisoned(id));
        }
        handle.request_pause();
        Ok(handle.info())
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
            let handle = inner
                .sessions
                .remove(id)
                .ok_or_else(|| ApiError::session_not_found(id))?;
            handle
        };
        let info = handle.info();
        handle.request_cancel();
        handle.join_within(Duration::from_secs(1));
        Ok(info)
    }

    /// 回收空闲超过 TTL 的会话，返回回收条数。
    pub fn sweep(&self) -> usize {
        let expired: Vec<SessionHandle> = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return 0,
            };
            let mut expired: Vec<SessionHandle> = Vec::new();
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
        let all: Vec<SessionHandle> = {
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

/// 起会话线程：宿主从生到死只属于这里。
fn spawn_worker(
    id: String,
    rx: Receiver<Job>,
    ctrl: Arc<AtomicU8>,
    busy: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    last_active: Arc<Mutex<Instant>>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(format!("sess-{id}"))
        .spawn(move || {
            // `KernelHost` 是 `!Send` ⇒ ⇒ 只能在这里 `new()`，不能从外面搬进来。
            let host = KernelHost::new();
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

    fn registry() -> SessionRegistry {
        SessionRegistry::new(2, Duration::from_millis(50))
    }

    #[test]
    fn create_lists_and_probes_a_session() {
        let registry = registry();
        let info = registry
            .create(Some("minimal".to_string()), BoardSource::Request)
            .unwrap();
        assert_eq!(info.board.as_deref(), Some("minimal"));
        assert_eq!(info.board_source, BoardSource::Request);
        assert_eq!(info.schema_version, SCHEMA_VERSION);

        let (infos, max, used) = registry.list();
        assert_eq!(max, 2);
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
            .create(None, BoardSource::None)
            .unwrap();
        registry
            .create(None, BoardSource::None)
            .unwrap();
        let err = registry.create(None, BoardSource::None).unwrap_err();
        assert_eq!(err.code(), "E_SESS-05");
        assert_eq!(err.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(registry.used(), 2);
    }

    #[test]
    fn delete_removes_the_session_and_a_second_delete_is_not_found() {
        let registry = registry();
        let info = registry.create(None, BoardSource::None).unwrap();
        registry.delete(&info.id).unwrap();
        assert_eq!(registry.used(), 0);
        let err = registry.delete(&info.id).unwrap_err();
        assert_eq!(err.code(), "E_SESS-01");
    }

    #[test]
    fn ttl_reaps_idle_sessions() {
        let registry = SessionRegistry::new(4, Duration::from_millis(20));
        let info = registry.create(None, BoardSource::None).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(registry.sweep(), 1);
        assert_eq!(registry.used(), 0);
        assert_eq!(registry.probe(&info.id).unwrap_err().code(), "E_SESS-01");
    }

    #[test]
    fn shutdown_reaps_everything() {
        let registry = registry();
        registry.create(None, BoardSource::None).unwrap();
        registry.create(None, BoardSource::None).unwrap();
        registry.shutdown();
        assert_eq!(registry.used(), 0);
    }
}
