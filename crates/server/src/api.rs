//! HTTP 面（M-24 · §3.1）。P0-1 有 `GET /api/version` 与 `GET /healthz`；
//! P0-2 加上会话四条（建 / 列 / 查 / 删）。
//!
//! ⚠️ 兜底**不用** axum 的默认 404（那是空 body 的纯文本）—— R-09 要的是
//! `{code, detail, status}`；同理，「会话不存在」是 `E_SESS-01`，「路径不存在」
//! 是 `E_PROTO-08` ⇒ ⇒ 两种 404 必须能被客户端区分开。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use riscv_sim_contract::model::{
    Breakpoint, BreakpointKind, TraceFormat, TraceSource, WatchAccess, Watchpoint, WriteSize,
};
use riscv_sim_contract::platform::PlatformDescription;
// ⚠️ axum 的 WS 类型也叫 `Message` ⇒ ⇒ 契约的这条用别名进这一层，不做
//    `use ... as _` 的隐式覆盖（两个 `Message` 混用一次就是一条难查的错）。
use riscv_sim_contract::protocol::{CommandPayload, Direction, Message as ContractMessage};
use riscv_sim_contract::session::{CheckpointMetaDto, SessionSnapshotDto};
use riscv_sim_kernel::kernel_host::platform;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{Config, DEFAULT_MAX_CONSOLE};
use crate::error::ApiError;
use crate::session::{BoardSource, RunSpec, RunUntil, SessionInfo, SessionRegistry};
use crate::upload;

/// 远程模式的令牌头名（R-15）。
const TOKEN_HEADER: &str = "x-auth-token";

/// CORS 的固定三段：允许的请求头 / 方法 / 客户端可读的响应头。
///
/// ⚠️ **可读响应头必须列全**：`x-boot-*` / `x-console-*` 是对拍与分页的判据
/// ⇒ ⇒ 不 expose 的话浏览器 `fetch` 拿不到它们（跨域响应头**默认不可读**），
/// 客户端会以为「服务端没给」。
const CORS_ALLOW_HEADERS: &str = "content-type,x-auth-token";
const CORS_ALLOW_METHODS: &str = "GET,POST,PUT,DELETE,OPTIONS";
const CORS_EXPOSE_HEADERS: &str = "x-boot-stop,x-boot-executed,x-boot-console-total,x-boot-console-lost,x-boot-board-revision,x-boot-ram-window,x-console-offset,x-console-base,x-console-total,x-trace-next,x-trace-lost";
const CORS_MAX_AGE_SECS: &str = "600";

/// `run` 的 `prefix` 长度上限（字节）。
///
/// 前缀是「console 里出现这一行」的判据 ⇒ ⇒ 真实前缀是几十字节；
/// 给到 1 KiB 已经远超需要，再长只会让每个窗口做一次无意义的长匹配。
const MAX_PREFIX_BYTES: usize = 1024;

/// 一次 console 读的默认上限（64 KiB）。真正的上界是 `--max-console`。
const DEFAULT_CONSOLE_READ: usize = 64 * 1024;

/// 一次内存转储的默认字节数（上界由契约 `DUMP_MAX_BYTES` 管）。
const DEFAULT_MEMORY_LEN: u32 = 64;

/// 一次反汇编的默认条数。
const DEFAULT_DISASM_COUNT: u32 = 16;

/// `/api/boot` 的默认指令预算 / 硬上限，以及每个窗口的条数。
const BOOT_DEFAULT_STEPS: u32 = 1_000_000;
const BOOT_MAX_STEPS: u32 = 100_000_000;
const BOOT_CHUNK: u32 = 10_000;

/// WS 事件流的**轮询节拍**与每次最多取多少帧。
///
/// ⚠️ 为什么要轮询：事件环住在会话线程里（`!Send` 的宿主只能在那儿读），
/// 所以 WS 这一侧只能**按拍查询**，不是内核主动推。
const WS_POLL: Duration = Duration::from_millis(50);
const WS_CHUNK: usize = 256;
/// WS 每拍最多推多少 console 字节（R-06 的推送通道）。
const WS_CONSOLE_CHUNK: usize = 4096;

/// `GET .../events`（REST 版）一次最多取多少帧。
const DEFAULT_EVENT_LIMIT: usize = 256;
const MAX_EVENT_LIMIT: usize = 4096;

/// 一次 trace 导出的默认条数。
const DEFAULT_TRACE_LIMIT: u32 = 1024;

/// 一次符号列表的默认条数 / 硬上限（**服务端**的界，不是内核判据）。
const DEFAULT_SYMBOL_LIMIT: usize = 512;
const MAX_SYMBOL_LIMIT: usize = 4096;

/// 全局状态 + 会话表把手。
#[derive(Clone)]
pub struct AppState {
    /// 进程启动时刻（算 uptime 用）。
    pub started: Instant,
    /// **实际**绑定的地址。
    pub bind: SocketAddr,
    /// 启动默认板（可能为 `None`）。
    pub board: Option<String>,
    /// 会话空闲回收时长。
    pub session_ttl: Duration,
    /// 每会话 console 缓冲上限。
    pub max_console: usize,
    /// 镜像路径白名单根（`--allow-image-root`；`None` ⇒ 只收上传字节）。
    pub allow_image_root: Option<PathBuf>,
    /// 单次上传的镜像字节上限（`--max-image-bytes`）。
    pub max_image_bytes: usize,
    /// 一次 `restore` 的载荷上限。
    pub max_snapshot_bytes: usize,
    /// `run` 一次请求的墙钟上限（`--max-run-ms`）。
    pub max_run_ms: u64,
    /// 远程模式令牌；`None` ⇒ 不校验。
    pub token: Option<String>,
    /// 显式放行的浏览器 origin（`--cors-origins`）。
    ///
    /// ⚠️ 回环 origin **不靠这张表**：那是[`origin_is_loopback`]的默认判据。
    pub cors_origins: Vec<String>,
    /// 会话表。
    pub registry: Arc<SessionRegistry>,
}

impl AppState {
    /// 由配置与实际绑定地址构造（会话表在这里一起建起来）。
    pub fn new(cfg: &Config, bind: SocketAddr) -> Self {
        Self {
            started: Instant::now(),
            bind,
            board: cfg.board.clone(),
            session_ttl: cfg.session_ttl,
            max_console: cfg.max_console,
            allow_image_root: cfg.allow_image_root.clone(),
            max_image_bytes: cfg.max_image_bytes,
            max_snapshot_bytes: cfg.max_snapshot_bytes,
            max_run_ms: cfg.max_run_ms,
            token: cfg.token.clone(),
            cors_origins: cfg.cors_origins.clone(),
            registry: Arc::new(SessionRegistry::new(
                cfg.max_sessions,
                cfg.session_ttl,
                cfg.max_console,
            )),
        }
    }
}

/// `GET /api/version` 的回体（R-01 的三个字段是**必答项**）。
#[derive(Debug, Serialize)]
pub struct VersionPayload {
    /// 内核版本（`riscv_sim_kernel::VERSION`）。
    pub kernel: &'static str,
    /// 消息形状版本（协商判据：不符 ⇒ `E_PROTO-01`）。
    pub schema_version: u32,
    /// 契约 crate 的发布版本。
    pub contract_version: &'static str,
    /// 宿主进程自己的版本。
    pub server: &'static str,
    /// 实际监听地址。
    pub bind: String,
}

/// 组装 `/api/version` 的回体。三个版本字段各取自己的真相源。
pub fn version_payload(state: &AppState) -> VersionPayload {
    VersionPayload {
        kernel: riscv_sim_kernel::VERSION,
        schema_version: riscv_sim_contract::SCHEMA_VERSION,
        contract_version: riscv_sim_contract::CONTRACT_VERSION,
        server: env!("CARGO_PKG_VERSION"),
        bind: state.bind.to_string(),
    }
}

/// `POST /api/sessions` 的请求体。**整份可选**：不带 body 等于「用启动默认板」。
#[derive(Debug, Default, Deserialize)]
pub struct CreateSessionRequest {
    /// 想要的板模板 id；缺省 ⇒ 用 `--board` 的启动默认值。
    #[serde(default)]
    pub board: Option<String>,
}

/// 会话描述（列表与详情共用形状）。
#[derive(Debug, Serialize)]
pub struct SessionPayload {
    /// 会话 id。
    pub session_id: String,
    /// 生效的板模板 id。
    pub board: Option<String>,
    /// 板从哪来：`request` / `startup-default` / `none`。
    pub board_source: &'static str,
    /// 会话内核宿主理解的 schema 版本。
    pub schema_version: u32,
    /// 已存活毫秒。
    pub age_ms: u64,
    /// 距上次活动毫秒。
    pub idle_ms: u64,
    /// 是否正在执行一条命令。
    pub busy: bool,
}

impl SessionPayload {
    fn from_info(info: &SessionInfo) -> Self {
        Self {
            session_id: info.id.clone(),
            board: info.board.clone(),
            board_source: info.board_source.as_str(),
            schema_version: info.schema_version,
            age_ms: info.age_ms,
            idle_ms: info.idle_ms,
            busy: info.busy,
        }
    }
}

/// 装配路由。P0-3 的板与装载、P0-4 的 run 都挂在这张表上，**不动**已有路径。
pub fn router(state: AppState) -> Router {
    let guarded = Router::new()
        .route("/api/sessions", post(create_session).get(list_sessions))
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/pause", post(pause_session))
        .route("/api/sessions/{id}/reset", post(reset_session))
        .route("/api/sessions/{id}/board", put(set_board))
        .route("/api/sessions/{id}/image", post(load_image))
        .route("/api/sessions/{id}/run", post(run_session))
        .route("/api/sessions/{id}/step", post(step_session))
        .route("/api/sessions/{id}/command", post(contract_command))
        .route("/api/sessions/{id}/console", get(read_console))
        .route("/api/sessions/{id}/events", get(read_events))
        .route("/api/sessions/{id}/events/ws", get(events_ws))
        .route(
            "/api/sessions/{id}/trace",
            get(read_trace).post(set_trace_enabled),
        )
        .route("/api/sessions/{id}/trace/enabled", put(set_trace_enabled))
        .route("/api/sessions/{id}/jobs/{job}", get(get_job))
        .route("/api/sessions/{id}/snapshot", post(take_snapshot))
        .route("/api/sessions/{id}/restore", post(restore_snapshot))
        .route("/api/sessions/{id}/state", get(session_state))
        .route("/api/sessions/{id}/memory", get(read_memory_range))
        .route("/api/sessions/{id}/symbols", get(list_symbols))
        .route("/api/sessions/{id}/disasm", get(disassemble))
        .route(
            "/api/sessions/{id}/breakpoints",
            get(list_breakpoints).post(set_breakpoint),
        )
        .route(
            "/api/sessions/{id}/watchpoints",
            get(list_watchpoints).post(set_watchpoint),
        )
        .route(
            "/api/sessions/{id}/watchpoints/{wp}",
            delete(clear_watchpoint),
        )
        .route(
            "/api/sessions/{id}/breakpoints/{bp}",
            delete(clear_breakpoint),
        )
        .route(
            "/api/sessions/{id}/breakpoints/{bp}/enabled",
            put(enable_breakpoint),
        )
        .route("/api/boot", post(boot))
        .route("/api/board/templates", get(list_templates))
        .route("/api/board/templates/{id}", get(get_template))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new()
        .route("/api/version", get(version))
        .route("/healthz", get(healthz))
        .merge(guarded)
        .fallback(unknown_route)
        // ⚠️ CORS 必须在**最外层**：预检（`OPTIONS`）既没有令牌、也不会被路由
        // 认出来 ⇒ ⇒ 放在 `require_token` 里面那一层会被 401/405 拒掉 ⇒ ⇒
        // 浏览器根本读不到「这个 origin 允不允许」。
        .layer(middleware::from_fn_with_state(state.clone(), cors_headers))
        .with_state(state)
}

async fn version(State(state): State<AppState>) -> Json<VersionPayload> {
    Json(version_payload(&state))
}

async fn healthz(State(state): State<AppState>) -> Json<serde_json::Value> {
    let uptime_ms = state.started.elapsed().as_millis() as u64;
    Json(json!({
        "status": "ok",
        "uptime_ms": uptime_ms,
        "sessions": state.registry.used(),
        "session_quota": state.registry.quota(),
        "board": state.board,
        "session_ttl_s": state.session_ttl.as_secs(),
        "max_console": if state.max_console == 0 { DEFAULT_MAX_CONSOLE } else { state.max_console },
        // ⚠️ **`/api/boot` 的墙钟硬上限**也报出来（R-12）：对拍脚本要在**发请求
        //   之前**就知道「我这条 `timeout_ms` 会不会被 400 拒」，而不是撞一次
        //   才算得出来。
        "max_run_ms": state.max_run_ms,
        "bind": state.bind.to_string(),
    }))
}

/// 板的解析规则（**纯函数** ⇒ ⇒ 三种来源可单测，不必起进程）。
///
/// 优先级：**请求体 > `--board` 启动默认 > 无板**。
pub fn resolve_board(
    requested: Option<String>,
    startup: Option<String>,
) -> (Option<String>, BoardSource) {
    match (requested, startup) {
        (Some(board), _) => (Some(board), BoardSource::Request),
        (None, Some(board)) => (Some(board), BoardSource::StartupDefault),
        (None, None) => (None, BoardSource::None),
    }
}

/// 建会话。`board` 的优先级：**请求体 > `--board` 启动默认 > 无板**。
///
/// ⚠️ 三种来源都要记进 `board_source` ⇒ ⇒ 事后追「这块板从哪来」不靠猜；
/// 「无板」也是**一种合法状态**（装载时才回 `E_PLT-22`），不是建会话失败。
async fn create_session(
    State(state): State<AppState>,
    body: Option<Json<CreateSessionRequest>>,
) -> Result<(StatusCode, Json<SessionPayload>), ApiError> {
    let requested = body.and_then(|Json(req)| req.board);
    let (board, source) = resolve_board(requested, state.board.clone());
    let info = state.registry.create(board, source)?;
    Ok((StatusCode::CREATED, Json(SessionPayload::from_info(&info))))
}

async fn list_sessions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (infos, max, used) = state.registry.list();
    let sessions: Vec<SessionPayload> = infos.iter().map(SessionPayload::from_info).collect();
    Json(json!({
        "sessions": sessions,
        "quota": { "max": max, "used": used },
    }))
}

/// 取一条会话。**顺带探活**：表里有、线程已死 ⇒ 500 + `E_SESS-03`
/// （不是假装还在，然后让下一个请求撞墙）。
async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (info, pc, executed) = state.registry.probe(&id)?;
    let payload = SessionPayload::from_info(&info);
    Ok(Json(json!({
        "session": payload,
        "pc": pc,
        "executed": executed,
    })))
}

/// `PUT …/board` 的请求体：`template` 与 `description` **二选一**。
#[derive(Debug, Deserialize)]
pub struct SetBoardRequest {
    /// 内置模板 id（`default` / `qemu-virt` / `minimal`）。
    #[serde(default)]
    pub template: Option<String>,
    /// 现成的一份板描述（客户端自己攒的板）。
    #[serde(default)]
    pub description: Option<PlatformDescription>,
    /// 乐观并发的期望修订号；给了且不符 ⇒ `409 + E_PLT-13`（内核判）。
    #[serde(default)]
    pub expected_revision: Option<u64>,
}

/// 配板（R-03）：**一会话一板**，问题在服务端翻成「哪份描述」之后就交给内核。
///
/// ⚠️ **模板查不到是 404**（服务端自己判，内核根本不会被问到）⇒ ⇒ 与
/// 「无板装载 409」同码不同状态 —— 状态由发生点决定（表 9）。
async fn set_board(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SetBoardRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let description = match (req.template, req.description) {
        (Some(template), None) => platform::builtin_description(&template)
            .map_err(|_| ApiError::template_not_found(&template))?,
        (None, Some(description)) => description,
        (Some(_), Some(_)) => {
            return Err(ApiError::bad_request(
                "give either `template` or `description`, not both",
            ));
        }
        (None, None) => {
            return Err(ApiError::bad_request(
                "one of `template` or `description` is required",
            ));
        }
    };
    let body = state
        .registry
        .set_board(&id, description, req.expected_revision)?;
    Ok(Json(body))
}

/// `POST …/image` 的 JSON 形态：从白名单路径读。
#[derive(Debug, Deserialize)]
struct ImagePathRequest {
    /// 镜像文件路径（必须在 `--allow-image-root` 下）。
    path: String,
    /// 覆盖回显名（缺省取文件名）。
    #[serde(default)]
    name: Option<String>,
}

/// 装载镜像（R-04）：`octet-stream` 直传字节，或 JSON `{path}` 从白名单读。
///
/// ⚠️ **不判「有没有板」**：那是内核的事（无板 ⇒ `409 + E_PLT-22` + 中文
/// 引导）⇒ ⇒ 服务端**不造第二条判据**。
async fn load_image(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let content_type = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (name, bytes) = if content_type.starts_with("application/json") {
        let request: ImagePathRequest = serde_json::from_slice(&body)
            .map_err(|err| ApiError::bad_request(format!("bad JSON body: {err}")))?;
        let resolved = upload::whitelist_path(state.allow_image_root.as_deref(), &request.path)?;
        // ⚠️ 读盘放 blocking 池：16 MiB 的同步读会把 async 工作线程钉住。
        let read_from = resolved.clone();
        let bytes = tokio::task::spawn_blocking(move || std::fs::read(&read_from))
            .await
            .map_err(|err| ApiError::bad_request(format!("image read task failed: {err}")))?
            .map_err(|err| {
                ApiError::bad_request(format!(
                    "image path {:?} cannot be read: {err}",
                    request.path
                ))
            })?;
        let name = request
            .name
            .unwrap_or_else(|| upload::name_from_path(&resolved));
        (name, bytes)
    } else {
        let name = headers
            .get("x-image-name")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("image.bin")
            .to_string();
        (name, body.to_vec())
    };

    if bytes.is_empty() {
        return Err(ApiError::bad_request(format!(
            "image {name:?} is empty; there is nothing to load"
        )));
    }
    if bytes.len() > state.max_image_bytes {
        return Err(ApiError::image_too_large(state.max_image_bytes));
    }
    let kind = upload::kind_for(&name, &bytes)?;
    let reply = state.registry.load_image(&id, name, kind, bytes)?;
    Ok(Json(reply))
}

/// 板模板清单（只读元数据）。
///
/// ⚠️ 元数据**不在这里数**：`platform::template_infos()` 是内核与 M-24
/// **共用的**唯一定义点（`device_count` 的口径变一次两边一起变）。
async fn list_templates() -> Json<serde_json::Value> {
    Json(json!({ "templates": platform::template_infos() }))
}

/// 板模板详情（完整 `PlatformDescription`）。
async fn get_template(Path(id): Path<String>) -> Result<Json<serde_json::Value>, ApiError> {
    let description =
        platform::builtin_description(&id).map_err(|_| ApiError::template_not_found(&id))?;
    Ok(Json(json!({ "id": id, "description": description })))
}

/// `POST …/run` 的请求体：**整份可选**（R-05）。
#[derive(Debug, Default, Deserialize)]
pub struct RunRequest {
    /// 指令上限（不给 ⇒ 只看墙钟）。
    #[serde(default)]
    pub steps: Option<u32>,
    /// 墙钟上限（毫秒）；不给 ⇒ 用 `--max-run-ms`。
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// console 里出现这个字节串就停（R-05 的 `prefix`）。
    #[serde(default)]
    pub prefix: Option<String>,
    /// 继续到下一个停点：`breakpoint` / `halt`（R-05）。
    #[serde(default)]
    pub until: Option<String>,
    /// `true` ⇒ **异步**：立刻回 `202 {job_id, poll}`，结果由
    /// `GET …/jobs/{job_id}` 轮询（P1）。
    #[serde(default, rename = "async")]
    pub run_async: bool,
}

/// 把请求折成一份 [`RunSpec`]（**纯函数** ⇒ ⇒ 校验可单测）。
///
/// 规则：
/// - `steps = 0` ⇒ 400（「跑 0 条」没有意义，且会让调用方以为跑过）；
/// - **`steps` 与 `timeout_ms` 同时给 ⇒ 400**：`StepN` 是「恰好 n 条」的
///   调试器语义，套墙钟会让它变成「跑了多少算多少」⇒ ⇒ 两件事不能混；
/// - `until` 只认 `breakpoint` / `halt`，其余 ⇒ 400；`until` 与 `steps`
///   同时给 ⇒ 400（「跑满 n 条」与「跑到停点」是两句话）；
/// - `prefix` 空串 ⇒ 400（空串会立刻命中，等于没给）；超过
///   [`MAX_PREFIX_BYTES`] ⇒ 400；
/// - `timeout_ms = 0` ⇒ 400；`> --max-run-ms` ⇒ 400（**不静默夹到上限**，
///   否则客户端以为它要的 30 s 生效了）。
pub fn plan_run(request: RunRequest, max_run_ms: u64) -> Result<RunSpec, ApiError> {
    if let (Some(_), Some(_)) = (request.steps, request.timeout_ms) {
        return Err(ApiError::bad_request(
            "give either `steps` or `timeout_ms`, not both (step semantics are exact-n)",
        ));
    }
    if let Some(steps) = request.steps {
        if steps == 0 {
            return Err(ApiError::bad_request("field `steps` must be >= 1"));
        }
    }
    let until = match request.until.as_deref() {
        None => None,
        Some(raw) => match RunUntil::parse(raw) {
            Some(until) => Some(until),
            None => {
                return Err(ApiError::bad_request(format!(
                    "field `until` must be \"breakpoint\" or \"halt\" (got {raw:?})"
                )))
            }
        },
    };
    if until.is_some() && request.steps.is_some() {
        return Err(ApiError::bad_request(
            "give either `steps` or `until`, not both (exact-n vs run-to-the-next-stop)",
        ));
    }
    if let Some(prefix) = request.prefix.as_deref() {
        if prefix.is_empty() {
            return Err(ApiError::bad_request(
                "field `prefix` must not be empty (an empty needle matches at once)",
            ));
        }
        if prefix.len() > MAX_PREFIX_BYTES {
            return Err(ApiError::bad_request(format!(
                "field `prefix` is {} bytes; the limit is {MAX_PREFIX_BYTES}",
                prefix.len()
            )));
        }
    }
    let timeout_ms = if request.steps.is_some() {
        None
    } else if until.is_some() || request.prefix.is_some() {
        // 前缀 / until 需要一个熔断：缺省沿用服务端上限，0 与超限都拒。
        match request.timeout_ms {
            None => Some(max_run_ms),
            Some(0) => return Err(ApiError::bad_request("field `timeout_ms` must be >= 1")),
            Some(timeout_ms) if timeout_ms > max_run_ms => {
                return Err(ApiError::bad_request(format!(
                    "timeout_ms {timeout_ms} exceeds --max-run-ms ({max_run_ms}); raise the limit or lower the request"
                )))
            }
            Some(timeout_ms) => Some(timeout_ms),
        }
    } else {
        let timeout_ms = request.timeout_ms.unwrap_or(max_run_ms);
        if timeout_ms == 0 {
            return Err(ApiError::bad_request("field `timeout_ms` must be >= 1"));
        }
        if timeout_ms > max_run_ms {
            return Err(ApiError::bad_request(format!(
                "timeout_ms {timeout_ms} exceeds --max-run-ms ({max_run_ms}); raise the limit or lower the request"
            )));
        }
        Some(timeout_ms)
    };
    Ok(RunSpec {
        steps: request.steps,
        timeout_ms,
        prefix: request.prefix,
        until,
    })
}

/// 跑到停点（R-05）。
///
/// ⚠️ **`timeout` 回 504，但 body 仍带停点信息**：只回一个错误体的话，
/// 客户端拿不到「跑到哪儿了」—— 而熔断最常见的用法正是「跑了 5 秒，停在
/// pc=X，已执行 N 条」。
async fn run_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<RunRequest>>,
) -> Result<Response, ApiError> {
    let request = body.map(|Json(request)| request).unwrap_or_default();
    // ⚠️ 先记下 `async`：`plan_run` 吃的是**同一份**折算规则（steps 与
    //    timeout_ms 互斥、0 与超上限都拒），异步与同步不能各判一套。
    let run_async = request.run_async;
    let max_run_ms = state.max_run_ms;
    let spec = plan_run(request, max_run_ms)?;
    if run_async {
        let submitted = state.registry.run_async(&id, spec)?;
        return Ok((StatusCode::ACCEPTED, Json(submitted)).into_response());
    }
    let report = state.registry.run(&id, spec)?;
    if report.get("stop_reason").and_then(|value| value.as_str()) == Some("timeout") {
        let mut timeout_body = report;
        timeout_body["code"] = json!(format!(
            "E_{}-06",
            riscv_sim_contract::error::ErrorFamily::Session.as_str()
        ));
        timeout_body["detail"] = json!(format!(
            "run hit the wall-clock limit ({max_run_ms} ms); the guest is stopped, see `executed` / `pc`"
        ));
        timeout_body["status"] = json!(504);
        return Ok((StatusCode::GATEWAY_TIMEOUT, Json(timeout_body)).into_response());
    }
    Ok(Json(report).into_response())
}

/// `GET …/jobs/{job}` —— 读一条异步作业（P1）。
async fn get_job(
    State(state): State<AppState>,
    Path((id, job)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let body = state.registry.job(&id, &job)?;
    Ok(Json(body))
}

/// `POST …/step` 的请求体。
#[derive(Debug, Deserialize)]
pub struct StepRequest {
    /// **恰好**跑多少条（`n >= 1`）。
    pub n: u32,
}

/// 精确 n 条（R-05）。**没有墙钟**：`StepN` 语义就是「恰好 n 条」。
async fn step_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<StepRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if request.n == 0 {
        return Err(ApiError::bad_request("field `n` must be >= 1"));
    }
    let report = state.registry.run(
        &id,
        RunSpec {
            steps: Some(request.n),
            ..RunSpec::default()
        },
    )?;
    Ok(Json(report))
}

/// `POST …/command` 的请求体（v1.11）：整条契约消息 + M-07 帧的配对号。
#[derive(Debug, Deserialize)]
pub struct ContractCommandRequest {
    /// 冻结的契约 `Message`（`direction` 必须是 `Command`）。
    pub message: ContractMessage,
    /// M-07 帧的 `reqId`：服务端回贴到**第一条**应答帧上。
    #[serde(default, rename = "reqId")]
    pub req_id: Option<u64>,
}

/// 命令面只接受命令：方向不是 `Command` 就 400。
fn ensure_command_direction(message: &ContractMessage) -> Result<(), ApiError> {
    if message.direction != Direction::Command {
        return Err(ApiError::bad_request(
            "field `message.direction` must be `Command`（这条通道是命令面）",
        ));
    }
    Ok(())
}

/// 契约命令（v1.11）：整条消息进会话线程，回体是这条命令的**应答帧**。
///
/// 回体 = `{"frames": [<首条应答帧，带 reqId>]}`：正是前端 M-05 要的那一颗
/// 「配对用的响应」；这条命令引发的其余事件（状态增量 / console / 波形）
/// 照旧进事件流，谁在看都能看见，但**不会**在回体里再给一份。
///
/// ⚠️ 这条通道**不翻译**命令：前端 M-05 发什么，内核 `handle_message` 就收
///    什么 ⇒ ⇒ 不存在「契约命令 → REST 端点」的第二张映射表。
/// ⚠️ 内核的业务错误（`E_PLT-22` / `E_DBG-02` …）**不**转 HTTP 错误：它们
///    以 `Error` 事件当应答进回体（带 `reqId`），由前端 M-05 拒掉对应命令
///    ⇒ ⇒ 同一个判据只有一个落点。
async fn contract_command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ContractCommandRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_command_direction(&request.message)?;
    let body = state
        .registry
        .contract(&id, request.message, request.req_id)?;
    Ok(Json(body))
}

/// `GET …/console` 的查询串。
#[derive(Debug, Default, Deserialize)]
pub struct ConsoleQuery {
    /// 起始绝对偏移（缺省 0）。
    #[serde(default)]
    pub offset: Option<u64>,
    /// 最多多少字节（缺省 64 KiB，且不超过 `--max-console`）。
    #[serde(default)]
    pub limit: Option<usize>,
}

/// 读 console 字节（R-06）。
///
/// ⚠️ **回原始字节**（`application/octet-stream`），不套 JSON：console 是
/// 文本流，把它塞进 JSON 数组会让 `curl | xxd` 这类用法变得没法用。
/// 三对方位信息走 header：`X-Console-Offset` / `X-Console-Base` /
/// `X-Console-Total`。
/// `GET /api/sessions/{id}/events?since=&limit=`
#[derive(Debug, Deserialize)]
struct EventsQuery {
    /// 起始序号（缺省 = 0，即「从头」）。
    since: Option<u64>,
    /// 最多多少帧（默认 256，硬上限 4096）。
    limit: Option<usize>,
}

async fn read_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let since = query.since.unwrap_or(0);
    let limit = plan_event_limit(query.limit);
    let body = state.registry.events(&id, since, limit)?;
    Ok(Json(body))
}

/// 事件条数：**缺省 256**，超上限**夹到上限**（读是幂等的，夹不改变语义）。
fn plan_event_limit(requested: Option<usize>) -> usize {
    requested
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .clamp(1, MAX_EVENT_LIMIT)
}

/// `GET /api/sessions/{id}/events/ws` —— WS 事件流（P1 的主通道）。
///
/// 协议（R-13）：服务端**只往外推**，一帧一条 JSON —— 帧形就是 M-07 的
/// `{type, seq, sessionId, role, reqId, payload}`（`payload` 直接是契约
/// `Message` 的 `payload`，内核事件不做二次包装）⇒ ⇒ 前端
/// `front/services/m07_transport.js` 的 server-mode 分支用它自己的
/// `decodeFrame` 就能解。背压丢帧时**另发一条** `type:"resume"` 的帧，
/// `payload` 里写明丢了多少、从哪个 seq 续 ⇒ ⇒ 不静默断流。
async fn events_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    ws.on_upgrade(move |socket| stream_events(socket, state, id))
}

async fn stream_events(mut socket: WebSocket, state: AppState, id: String) {
    let mut next: u64 = 0;
    // console 推送的游标（R-06 的 P1：与拉取**共用同一份 offset**）。
    let mut console_next: u64 = 0;
    let mut console_lost = false;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(WS_POLL) => {
                let since = next;
                let body = match state.registry.events(&id, since, WS_CHUNK) {
                    Ok(body) => body,
                    Err(err) => {
                        // 会话没了（删除 / TTL）⇒ ⇒ 推最后一帧说明原因再关。
                        //
                        // ⚠️ **不**把 `ApiError` 的场景码塞进帧里：`ApiError` 的
                        //    getter 是 `cfg(test)` 的（生产路径只写不读，避免同一
                        //    判据有第二个消费方）⇒ ⇒ 这里只报「关了」+ 原因类。
                        //    客户端要精确码就用 `GET .../events` 的 HTTP 错误体。
                        let reason = if state.registry.probe(&id).is_ok() {
                            "session busy or shutting down"
                        } else {
                            "session gone"
                        };
                        // 关闭帧沿用 M-07 的 `resume` 形状（`payload` 里说明原因），
                        // 客户端照帧解析即可，不必另认一种封套。
                        let frame = serde_json::json!({
                            "type": "resume",
                            "seq": since,
                            "sessionId": id,
                            "role": serde_json::Value::Null,
                            "reqId": serde_json::Value::Null,
                            "payload": {
                                "closed": true,
                                "reason": reason,
                                "status": err.into_response().status().as_u16(),
                            },
                        });
                        let _ = socket.send(Message::Text(frame.to_string().into())).await;
                        let _ = socket.send(Message::Close(None)).await;
                        return;
                    }
                };
                let got_next = body.get("next").and_then(|value| value.as_u64()).unwrap_or(since);
                let lost = body.get("lost").and_then(|value| value.as_bool()).unwrap_or(false);
                next = got_next;
                // 背压：丢了就**显式**报（R-13：允许丢帧，但不许静默断流）。
                if lost {
                    let frame = serde_json::json!({
                        "type": "resume",
                        "seq": got_next,
                        "sessionId": id,
                        "role": serde_json::Value::Null,
                        "reqId": serde_json::Value::Null,
                        "payload": { "lost": true, "next": got_next },
                    });
                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                        return;
                    }
                }
                if let Some(frames) = body.get("frames").and_then(|value| value.as_array()) {
                    for frame in frames {
                        if socket
                            .send(Message::Text(frame.to_string().into()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                // ---- console 推送（R-06 P1）----
                if !console_lost {
                    match state.registry.console(&id, console_next, WS_CONSOLE_CHUNK) {
                        Ok(body) => {
                            let base = body.get("base").and_then(|v| v.as_u64()).unwrap_or(0);
                            let total = body.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                            if base > console_next {
                                // 窗口丢了：显式说「从哪重新对齐」，不静默少字节。
                                console_lost = true;
                                let frame = serde_json::json!({
                                    "type": "resume",
                                    "seq": base,
                                    "sessionId": id,
                                    "role": serde_json::Value::Null,
                                    "reqId": serde_json::Value::Null,
                                    "payload": {
                                        "topic": "sim.console",
                                        "lost": true,
                                        "from": console_next,
                                        "base": base,
                                    },
                                });
                                if socket
                                    .send(Message::Text(frame.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            let bytes: Vec<u8> = body
                                .get("bytes")
                                .and_then(|value| value.as_array())
                                .map(|list| {
                                    list.iter()
                                        .filter_map(|byte| byte.as_u64().map(|v| v as u8))
                                        .collect()
                                })
                                .unwrap_or_default();
                            if !bytes.is_empty() {
                                let len = bytes.len() as u64;
                                let frame = console_frame(&id, console_next, &bytes);
                                console_next += len;
                                if socket
                                    .send(Message::Text(frame.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            } else if total == console_next {
                                // 没有新字节，游标不动（保持与拉取同一份 offset）。
                            }
                        }
                        Err(_) => console_lost = true,
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    // 本端**只往外推**：入向只看「关没关」。
                    None | Some(Ok(Message::Close(_))) | Some(Err(_)) => return,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

/// console 的 M-07 帧：`payload` 是**完整的**契约 `Message`
/// （`{schema_version, direction, seq, payload:{"Event":{"Console":{...}}}}`），
/// `seq` 用 **console 绝对偏移** ⇒ ⇒ 与 `GET …/console?offset=` 同一份账
/// （R-06：不能两个真相）。
///
/// ⚠️ 信封**不能省**：前端 `decodeMessage` 第一步就要 `schema_version`，
/// 只给内层 `{"Event":…}` 会被整帧判成 `E_PROTO-08` ⇒ ⇒ 终端的字节流看起来
/// 「一个都没有」。与 Worker 通道同形（`payload: eventMessage(...)`）。
fn console_frame(session_id: &str, offset: u64, bytes: &[u8]) -> serde_json::Value {
    let message = riscv_sim_contract::protocol::Message::event(
        offset,
        riscv_sim_contract::protocol::EventPayload::Console {
            bytes: bytes.to_vec(),
        },
    );
    let payload = serde_json::to_value(&message).unwrap_or(serde_json::Value::Null);
    json!({
        "type": "event",
        "seq": offset,
        "sessionId": session_id,
        "role": serde_json::Value::Null,
        "reqId": serde_json::Value::Null,
        "payload": payload,
    })
}

/// `GET /api/sessions/{id}/trace?source=&format=&offset=&limit=`
#[derive(Debug, Deserialize)]
struct TraceQuery {
    /// `execution`（缺省）或 `waveform`。
    source: Option<String>,
    /// `csv`（缺省）/ `json` / `vcd`（只 warranted waveform）。
    format: Option<String>,
    /// 续读起点（条目数）。
    offset: Option<u64>,
    /// 本次最多多少条。
    limit: Option<u32>,
}

async fn read_trace(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TraceQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let source = parse_trace_source(query.source.as_deref())?;
    let format = parse_trace_format(query.format.as_deref(), source)?;
    let command = CommandPayload::ExportTrace {
        source,
        format,
        offset: query.offset.unwrap_or(0),
        limit: plan_trace_limit(query.limit),
        filter: Default::default(),
    };
    let body = state.registry.export_trace(&id, command)?;
    Ok(Json(body))
}

fn parse_trace_source(raw: Option<&str>) -> Result<TraceSource, ApiError> {
    match raw.unwrap_or("execution").to_ascii_lowercase().as_str() {
        "execution" | "exec" => Ok(TraceSource::Execution),
        "waveform" | "wave" => Ok(TraceSource::Waveform),
        other => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_PROTO-01",
            format!("unknown trace source {other:?}; expected execution|waveform"),
        )),
    }
}

fn parse_trace_format(raw: Option<&str>, source: TraceSource) -> Result<TraceFormat, ApiError> {
    let default = match source {
        TraceSource::Execution => "csv",
        TraceSource::Waveform => "vcd",
    };
    let name = raw.unwrap_or(default).to_ascii_lowercase();
    match (name.as_str(), source) {
        ("csv", _) => Ok(TraceFormat::Csv),
        ("json", TraceSource::Execution) => Ok(TraceFormat::Json),
        ("vcd", TraceSource::Waveform) => Ok(TraceFormat::Vcd),
        _ => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_PROTO-01",
            format!("format {name:?} is not valid for source {source:?}; see contract TraceFormat"),
        )),
    }
}

fn plan_trace_limit(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_TRACE_LIMIT)
        .min(DEFAULT_TRACE_LIMIT)
}

/// `PUT /api/sessions/{id}/trace/enabled`
#[derive(Debug, Deserialize)]
struct TraceEnabledBody {
    enabled: bool,
}

async fn set_trace_enabled(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TraceEnabledBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let reply = state.registry.set_trace(&id, body.enabled)?;
    Ok(Json(reply))
}

/// `POST /api/sessions/{id}/snapshot` —— 采集会话快照（六段齐全）。
///
/// 请求体可省：`{id?, base_seq?}` 只影响检查点元数据；**`t_virt` 由宿主
/// 按引擎虚拟时间代填**（同一个量不能有两个真相）。
async fn take_snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<SnapshotRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let request = body.map(|Json(request)| request).unwrap_or_default();
    let meta = CheckpointMetaDto {
        id: request.id,
        // 宿主会把它覆盖成引擎的虚拟时间；这里给 0 只是占位。
        t_virt: 0,
        t_host: state.started.elapsed().as_millis() as u64,
        base_seq: request.base_seq,
        version: riscv_sim_contract::session::SESSION_SNAPSHOT_SCHEMA_VERSION,
    };
    let reply = state.registry.snapshot(&id, meta)?;
    Ok(Json(reply))
}

/// 一次快照采集的请求体（全可选）。
#[derive(Debug, Default, Deserialize)]
struct SnapshotRequest {
    /// 检查点 id（缺省 0）。
    #[serde(default)]
    id: u32,
    /// 稀疏输入流的基线序号（缺省 0）。
    #[serde(default)]
    base_seq: u32,
}

/// `POST /api/sessions/{id}/restore` —— 还原会话快照。
///
/// ⚠️ 体是 `SessionSnapshotDto` 的 JSON。**上限**由 `--max-snapshot-bytes`
/// 管（默认 32 MiB）：快照含整页 RAM ⇒ 比镜像大得多，不能沿用
/// `--max-image-bytes`。
async fn restore_snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.len() > state.max_snapshot_bytes {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "E_SNAP-04",
            format!(
                "snapshot body is {} bytes, over the {} byte limit (--max-snapshot-bytes)",
                body.len(),
                state.max_snapshot_bytes
            ),
        ));
    }
    let payload: Box<SessionSnapshotDto> = serde_json::from_slice(&body).map_err(|err| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_SNAP-03",
            format!("snapshot body is not a valid SessionSnapshotDto: {err}"),
        )
    })?;
    let reply = state.registry.restore(&id, payload)?;
    Ok(Json(reply))
}

async fn read_console(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ConsoleQuery>,
) -> Result<Response, ApiError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_CONSOLE_READ)
        .min(state.max_console.max(1));
    let body = state.registry.console(&id, offset, limit)?;
    let bytes: Vec<u8> = body
        .get("bytes")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|byte| byte.as_u64().map(|value| value as u8))
                .collect()
        })
        .unwrap_or_default();
    let base = body
        .get("base")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let total = body
        .get("total")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("x-console-offset", offset.to_string())
        .header("x-console-base", base.to_string())
        .header("x-console-total", total.to_string())
        .body(Body::from(bytes))
        .map_err(|err| ApiError::bad_request(format!("cannot build console response: {err}")))?;
    Ok(response)
}

/// 读状态（R-07）：`{pc, registers[32], executed}`。
async fn session_state(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(state.registry.state(&id)?))
}

/// `GET …/memory` 的查询串。
#[derive(Debug, Default, Deserialize)]
pub struct MemoryQuery {
    /// 起始地址（**必填**；缺了是 HTTP 层的参数不完整，内核无法替它判）。
    #[serde(default)]
    pub addr: Option<u32>,
    /// 字节数（缺省 64；`0` 与 `> DUMP_MAX_BYTES` 由内核回 `E_DBG-03`）。
    #[serde(default)]
    pub len: Option<u32>,
    /// 附加视图：`u32`（小端 32 位字）或 `asc`（可打印字符）。
    #[serde(default)]
    pub format: Option<String>,
}

/// 读一段内存（R-07）：走契约 `DumpMemory` ⇒ 一次最多 `DUMP_MAX_BYTES`。
///
/// ⚠️ **长度合法性不在这里夹**：`len == 0` / 超限是内核 `E_DBG-03` 的判据
/// ⇒ ⇒ 服务端再判一次就是第二个真相（也省得两边口径漂）。
async fn read_memory_range(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<MemoryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let addr = query
        .addr
        .ok_or_else(|| ApiError::bad_request("query `addr` is required"))?;
    let len = query.len.unwrap_or(DEFAULT_MEMORY_LEN);
    let mut body = state
        .registry
        .debug(&id, CommandPayload::DumpMemory { addr, len })?;
    // R-07 的「可选视图」：`format=u32|asc` 只**追加**视图，原始字节照旧
    // 在 `bytes` 里 ⇒ ⇒ 两个消费者（要判位的 / 要看的）不必二选一。
    if let Some(format) = query.format.as_deref() {
        let bytes: Vec<u8> = body
            .get("bytes")
            .and_then(|value| value.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|byte| byte.as_u64().map(|value| value as u8))
                    .collect()
            })
            .unwrap_or_default();
        let (words, ascii) = memory_view(&bytes, format)?;
        if let Some(words) = words {
            body["u32"] = json!(words);
        }
        if let Some(ascii) = ascii {
            body["asc"] = json!(ascii);
        }
        body["format"] = json!(format);
    }
    Ok(Json(body))
}

/// 内存视图（R-07）：`u32` ⇒ 小端 32 位字（不足 4 字节的尾巴丢掉）；
/// `asc` ⇒ 可打印字符视图（不可打印按 `.`，与 `xxd -c` 的习惯一致）。
fn memory_view(bytes: &[u8], format: &str) -> Result<(Option<Vec<u32>>, Option<String>), ApiError> {
    match format.to_ascii_lowercase().as_str() {
        "u32" | "word" => {
            let words: Vec<u32> = bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            Ok((Some(words), None))
        }
        "asc" | "ascii" => {
            let text: String = bytes
                .iter()
                .map(|byte| {
                    if (0x20..0x7f).contains(byte) {
                        char::from(*byte)
                    } else {
                        '.'
                    }
                })
                .collect();
            Ok((None, Some(text)))
        }
        other => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_PROTO-01",
            format!("unknown memory format {other:?}; expected u32|asc"),
        )),
    }
}

/// `GET …/symbols` 的查询串。
#[derive(Debug, Default, Deserialize)]
pub struct SymbolsQuery {
    /// 最多多少条（缺省 512，硬上限 4096）。
    #[serde(default)]
    pub limit: Option<usize>,
}

/// 列符号（R-07）。**只读宿主索引**，不重解析镜像。
async fn list_symbols(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<SymbolsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = plan_symbol_limit(query.limit);
    Ok(Json(state.registry.symbols(&id, limit)?))
}

/// 符号条数上限的折算（**纯函数**，可单测）。
///
/// 缺省 512；`0` ⇒ **空表**（「要 0 条」这个请求是明确的，不替它改）；
/// 超 4096 ⇒ 夹到 4096（这是**服务端**自己的界，不是内核判据）。
pub fn plan_symbol_limit(limit: Option<usize>) -> usize {
    match limit {
        Some(0) => 0,
        Some(limit) => limit.min(MAX_SYMBOL_LIMIT),
        None => DEFAULT_SYMBOL_LIMIT,
    }
}

/// `GET …/disasm` 的查询串。
#[derive(Debug, Default, Deserialize)]
pub struct DisasmQuery {
    /// 起始地址（必填）。
    #[serde(default)]
    pub addr: Option<u32>,
    /// 最多多少条（缺省 16；可能短于它 —— 未映射地址就截断，不是错误）。
    #[serde(default)]
    pub count: Option<u32>,
}

/// 反汇编（R-07）。
async fn disassemble(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DisasmQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let addr = query
        .addr
        .ok_or_else(|| ApiError::bad_request("query `addr` is required"))?;
    let count = query.count.unwrap_or(DEFAULT_DISASM_COUNT);
    Ok(Json(
        state
            .registry
            .debug(&id, CommandPayload::Disasm { addr, count })?,
    ))
}

/// 断点表的「新条目要什么」（**HTTP 面**的三选一）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointPlan {
    /// 直接给地址。
    Address(u32),
    /// 给符号名 ⇒ 先 `ResolveSymbol` 拿到地址，再按 `kind = Symbol` 注册。
    Symbol(String),
    /// 只给条件表达式 ⇒ 观察式条目，`id = 0` 由内核分配。
    Condition(String),
}

/// `POST …/breakpoints` 的请求体。
#[derive(Debug, Default, Deserialize)]
pub struct BreakpointRequest {
    /// 地址（与 `symbol`、`condition` 三选一）。
    #[serde(default)]
    pub addr: Option<u32>,
    /// 符号名（与 `addr`、`condition` 三选一）。
    #[serde(default)]
    pub symbol: Option<String>,
    /// 条件表达式（与 `addr`、`symbol` 三选一）。
    #[serde(default)]
    pub condition: Option<String>,
}

/// 三选一的校验（**纯函数**，可单测）。
///
/// 给两个 / 给零个都拒：这条请求的三种形态在语义上**互斥**（地址是给定
/// 事实、符号要解析、条件不绑地址）⇒ 允许两个同时出现就是允许自相矛盾。
pub fn plan_breakpoint(request: BreakpointRequest) -> Result<BreakpointPlan, ApiError> {
    let given = [
        request.addr.is_some(),
        request.symbol.is_some(),
        request.condition.is_some(),
    ]
    .iter()
    .filter(|given| **given)
    .count();
    if given != 1 {
        return Err(ApiError::bad_request(
            "give exactly one of `addr`, `symbol`, `condition`",
        ));
    }
    if let Some(addr) = request.addr {
        return Ok(BreakpointPlan::Address(addr));
    }
    if let Some(symbol) = request.symbol {
        let symbol = symbol.trim().to_string();
        if symbol.is_empty() {
            return Err(ApiError::bad_request("field `symbol` must not be blank"));
        }
        return Ok(BreakpointPlan::Symbol(symbol));
    }
    let condition = request.condition.unwrap_or_default();
    if condition.trim().is_empty() {
        return Err(ApiError::bad_request(
            "field `condition` must not be blank (use `addr` for an unconditional breakpoint)",
        ));
    }
    Ok(BreakpointPlan::Condition(condition))
}

/// 读断点表（R-07）。
async fn list_breakpoints(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(
        state.registry.debug(&id, CommandPayload::ListBreakpoints)?,
    ))
}

/// 下断点（R-07）：地址 / 符号 / 条件三选一，回执是**当前真实表**。
///
/// ⚠️ **符号类必须先解地址**：内核的 `SetBreakpoint` 只认 `addr`（`kind`
/// 只是来源标记，不负责解析）⇒ ⇒ 服务端先发 `ResolveSymbol`，解不出来就
/// 如实拒，**不编**一个地址。
async fn set_breakpoint(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<BreakpointRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let breakpoint = match plan_breakpoint(request)? {
        BreakpointPlan::Address(addr) => Breakpoint {
            id: addr,
            addr,
            source_hint: None,
            enabled: true,
            kind: BreakpointKind::Address,
            condition: None,
        },
        BreakpointPlan::Symbol(name) => {
            let resolved = state
                .registry
                .debug(&id, CommandPayload::ResolveSymbol { name: name.clone() })?;
            let addr = resolved
                .get("symbol")
                .and_then(|symbol| symbol.get("addr"))
                .and_then(|addr| addr.as_u64())
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "E_DBG-02",
                        format!("no such symbol: {name}"),
                    )
                })? as u32;
            Breakpoint {
                id: addr,
                addr,
                source_hint: Some(name),
                enabled: true,
                kind: BreakpointKind::Symbol,
                condition: None,
            }
        }
        BreakpointPlan::Condition(condition) => Breakpoint {
            // 观察式条目没有地址 ⇒ `id = 0` 是「请内核分配」（与前端同口径）。
            id: 0,
            addr: 0,
            source_hint: None,
            enabled: true,
            kind: BreakpointKind::Condition,
            condition: Some(condition),
        },
    };
    let table = state
        .registry
        .debug(&id, CommandPayload::SetBreakpoint(breakpoint))?;
    Ok((StatusCode::CREATED, Json(table)))
}

/// 删断点（R-07）。幂等：id 不存在 ⇒ 表不变、仍 200。
async fn clear_breakpoint(
    State(state): State<AppState>,
    Path((id, bp)): Path<(String, u32)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(
        state
            .registry
            .debug(&id, CommandPayload::ClearBreakpoint { id: bp })?,
    ))
}

/// `PUT …/breakpoints/{id}/enabled` 的请求体。
#[derive(Debug, Deserialize)]
pub struct EnableRequest {
    /// 开 / 关。
    pub enabled: bool,
}

/// 断点启停（R-07）。幂等：id 不存在 ⇒ 表不变、仍 200。
async fn enable_breakpoint(
    State(state): State<AppState>,
    Path((id, bp)): Path<(String, u32)>,
    Json(request): Json<EnableRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(state.registry.debug(
        &id,
        CommandPayload::SetBreakpointEnabled {
            id: bp,
            enabled: request.enabled,
        },
    )?))
}

/// `POST …/reset` —— 复位（R-14）：回到**当前工程的装载起点**。
///
/// 口径与前端「复位 = 重新装载当前工程」一致：用的是这条会话**最后一次
/// 成功装载**的那份镜像（服务端记着），板与订阅原样重建。
async fn reset_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(state.registry.reset(&id)?))
}

/// `GET …/watchpoints` —— 观察点表（R-08）。
async fn list_watchpoints(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(state.registry.watchpoints(&id)?))
}

/// `POST …/watchpoints` 的请求体（R-08）。
///
/// ⚠️ **id 可省**：省了就由服务端按「当前最大 id + 1」分配（与内核的
///   `set_watchpoint_with_id` 显式 id 语义对齐）；显式给了就按给的来
///   （同一 id 重复注册 ⇒ 覆盖，内核语义）。
#[derive(Debug, Deserialize)]
pub struct WatchpointRequest {
    /// 被观察的地址（必填）。
    pub addr: u32,
    /// 观察宽度：`byte` / `half` / `word`（缺省 `word`）。
    #[serde(default)]
    pub size: Option<String>,
    /// 访问方向：`read` / `write` / `either`（缺省 `write`）。
    #[serde(default)]
    pub access: Option<String>,
    /// 显式 id（缺省 ⇒ 服务端分配）。
    #[serde(default)]
    pub id: Option<u32>,
}

async fn set_watchpoint(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<WatchpointRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let size = parse_write_size(request.size.as_deref())?;
    let access = parse_watch_access(request.access.as_deref())?;
    let existing = state.registry.watchpoints(&id)?;
    let watchpoint_id = match request.id {
        Some(explicit) => explicit,
        None => {
            let max = existing
                .get("watchpoints")
                .and_then(|value| value.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|wp| wp.get("id").and_then(|value| value.as_u64()))
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            (max + 1) as u32
        }
    };
    let wp = Watchpoint {
        id: watchpoint_id,
        addr: request.addr,
        size,
        access,
    };
    let table = state.registry.set_watchpoint(&id, wp)?;
    Ok((StatusCode::CREATED, Json(table)))
}

/// 删观察点（R-08）。幂等：id 不存在 ⇒ 表不变、仍 200（内核就是幂等）。
async fn clear_watchpoint(
    State(state): State<AppState>,
    Path((id, wp)): Path<(String, u32)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(state.registry.clear_watchpoint(&id, wp)?))
}

fn parse_write_size(raw: Option<&str>) -> Result<WriteSize, ApiError> {
    match raw.map(|value| value.to_ascii_lowercase()).as_deref() {
        None | Some("word") | Some("4") => Ok(WriteSize::Word),
        Some("half") | Some("2") => Ok(WriteSize::Half),
        Some("byte") | Some("1") => Ok(WriteSize::Byte),
        Some(other) => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_PROTO-01",
            format!("unknown watchpoint size {other:?}; expected byte|half|word"),
        )),
    }
}

fn parse_watch_access(raw: Option<&str>) -> Result<WatchAccess, ApiError> {
    match raw.map(|value| value.to_ascii_lowercase()).as_deref() {
        None | Some("write") => Ok(WatchAccess::Write),
        Some("read") => Ok(WatchAccess::Read),
        Some("either") | Some("read-write") => Ok(WatchAccess::Either),
        Some(other) => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_PROTO-01",
            format!("unknown watchpoint access {other:?}; expected read|write|either"),
        )),
    }
}

/// `POST /api/boot` 的请求体（**一把梭** · R-08）。
#[derive(Debug, Default, Deserialize)]
pub struct BootRequest {
    /// 镜像路径（白名单内；必填）。
    pub path: String,
    /// 板模板 id；缺省 ⇒ `default`（一把梭不该逼客户端先配板）。
    #[serde(default)]
    pub board: Option<String>,
    /// console 里出现这个前缀就停（字节级子串）。
    #[serde(default)]
    pub prefix: Option<String>,
    /// 指令预算（缺省 1 000 000，上限 100 000 000）。
    #[serde(default)]
    pub steps: Option<u32>,
    /// 墙钟上限（毫秒，缺省 `--max-run-ms`）。
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// 把抓到的 console 字节写到这里（必须在 `--allow-image-root` 内）。
    #[serde(default)]
    pub write_to: Option<String>,
}

/// 一条 curl 跑完「配板 → 装载 → 跑到停点 → 取 console」（R-08）。
///
/// ⚠️ **临时会话**：为一请求建一条、跑完就删 ⇒ ⇒ 一把梭不占会话配额（也
/// 不给调用方留下要清理的东西）。失败路径也走 `delete`（best effort）。
///
/// ⚠️ **不是第二套停因**：每次窗口都是 `registry.run(steps=chunk)`，停因
/// 仍由内核 `run_until` 给；这里只做「攒 console / 判前缀 / 判预算」。
async fn boot(
    State(state): State<AppState>,
    Json(request): Json<BootRequest>,
) -> Result<Response, ApiError> {
    let path = request.path.trim().to_string();
    if path.is_empty() {
        return Err(ApiError::bad_request("field `path` is required"));
    }
    let steps = match request.steps {
        None => BOOT_DEFAULT_STEPS,
        Some(0) => return Err(ApiError::bad_request("field `steps` must be >= 1")),
        Some(steps) if steps > BOOT_MAX_STEPS => {
            return Err(ApiError::bad_request(format!(
                "steps {steps} exceeds the boot cap ({BOOT_MAX_STEPS})"
            )))
        }
        Some(steps) => steps,
    };
    let timeout_ms = match request.timeout_ms {
        None => state.max_run_ms,
        Some(0) => return Err(ApiError::bad_request("field `timeout_ms` must be >= 1")),
        Some(timeout_ms) if timeout_ms > state.max_run_ms => {
            return Err(ApiError::bad_request(format!(
                "timeout_ms {timeout_ms} exceeds --max-run-ms ({})",
                state.max_run_ms
            )))
        }
        Some(timeout_ms) => timeout_ms,
    };

    // 读镜像（与 `POST …/image` 的路径通道同一口径）。
    let resolved = upload::whitelist_path(state.allow_image_root.as_deref(), &path)?;
    let read_from = resolved.clone();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&read_from))
        .await
        .map_err(|err| ApiError::bad_request(format!("image read task failed: {err}")))?
        .map_err(|err| {
            ApiError::bad_request(format!("image path {path:?} cannot be read: {err}"))
        })?;
    if bytes.is_empty() {
        return Err(ApiError::bad_request(format!("image {path:?} is empty")));
    }
    let name = upload::name_from_path(&resolved);
    let kind = upload::kind_for(&name, &bytes)?;

    // 建临时会话（缺省板 = default）。
    let board = match request.board {
        Some(board) => Some(board),
        None => Some("default".to_string()),
    };
    let info = state
        .registry
        .create(board.clone(), crate::session::BoardSource::Request)?;
    let id = info.id.clone();

    // 板子的 **RAM 窗口**（`base:size`，十进制）⇒ ⇒ 随回执头一起给出（R-12：
    // 对拍脚本要拿它去配参考侧的内存图）。取值走契约层的
    // `ram_window()`（「取哪段 RAM」的唯一落点）⇒ ⇒ 这里不自己挑段，也不
    // 自己算 `base + size`。
    let ram_window = board.as_deref().and_then(boot_ram_window);

    let outcome = boot_once(
        &state,
        &id,
        name,
        kind,
        bytes,
        request.prefix,
        steps,
        timeout_ms,
        request.write_to,
        ram_window,
    )
    .await;
    // ⚠️ 无论如何都删：一把梭是**一次性**资源。
    let _ = state.registry.delete(&id);
    outcome
}

/// `boot` 的正文：装载 → 分窗跑到停点 / 前缀 / 预算 → 收 console。
#[allow(clippy::too_many_arguments)]
async fn boot_once(
    state: &AppState,
    id: &str,
    name: String,
    kind: riscv_sim_contract::model::ImageKind,
    bytes: Vec<u8>,
    prefix: Option<String>,
    steps: u32,
    timeout_ms: u64,
    write_to: Option<String>,
    ram_window: Option<(u32, u64)>,
) -> Result<Response, ApiError> {
    let loaded = state.registry.load_image(id, name, kind, bytes)?;
    let started = Instant::now();
    let deadline = Duration::from_millis(timeout_ms);
    let mut remaining = steps;
    let mut executed: u64 = 0;
    let mut collected: Vec<u8> = Vec::new();
    let mut console_offset: u64 = 0;
    let mut console_total: u64 = 0;
    let mut window_lost = false;
    // ⚠️ 不给初值：循环的**每一个** `break` 之前都先写它 ⇒ ⇒ 初值只会是
    //    「永远不会被读到的那一个」（clippy 也按这个口径判）。
    let stop: String;

    loop {
        if remaining == 0 {
            stop = String::from("budget");
            break;
        }
        if started.elapsed() >= deadline {
            stop = String::from("timeout");
            break;
        }
        let chunk = remaining.min(BOOT_CHUNK);
        // ⚠️ 与 `run` **同一份**停点描述：`boot` 自己不再造停因，只是把
        //   「跑一个窗口」交给会话线程，前缀/预算由它自己判（R-12）。
        let report = state.registry.run(
            id,
            RunSpec {
                steps: Some(chunk),
                ..RunSpec::default()
            },
        )?;
        let ran = report
            .get("executed")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        executed += ran;
        remaining = remaining.saturating_sub(u32::try_from(ran).unwrap_or(chunk));

        // 取这一窗口新产生的 console 字节。
        let console = state
            .registry
            .console(id, console_offset, BOOT_CHUNK as usize)?;
        let base = console
            .get("base")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        console_total = console
            .get("total")
            .and_then(|value| value.as_u64())
            .unwrap_or(console_total);
        if base > console_offset {
            // 环把窗口裁掉了 ⇒ ⇒ 只能从新的 base 接着读（不假装没丢）。
            window_lost = true;
            console_offset = base;
        }
        if let Some(chunk) = console.get("bytes").and_then(|value| value.as_array()) {
            let chunk: Vec<u8> = chunk
                .iter()
                .filter_map(|byte| byte.as_u64().map(|value| value as u8))
                .collect();
            console_offset += chunk.len() as u64;
            collected.extend_from_slice(&chunk);
        }
        if let Some(prefix) = &prefix {
            if contains(&collected, prefix.as_bytes()) {
                stop = String::from("prefix");
                break;
            }
        }

        let stop_reason = report
            .get("stop_reason")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        match stop_reason.as_str() {
            "steps" => {}
            other => {
                stop = other.to_string();
                break;
            }
        }
    }

    if let Some(target) = write_to {
        write_console_bytes(state.allow_image_root.as_deref(), &target, &collected)?;
    }

    let board = loaded
        .get("platform_revision")
        .and_then(|value| value.as_u64())
        .map(|revision| revision.to_string())
        .unwrap_or_default();
    let status = if stop == "timeout" {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::OK
    };
    let response = Response::builder()
        .status(status)
        .header("content-type", "application/octet-stream")
        .header("x-boot-stop", stop.as_str())
        .header("x-boot-executed", executed.to_string())
        .header("x-boot-console-total", console_total.to_string())
        .header("x-boot-console-lost", if window_lost { "1" } else { "0" })
        .header("x-boot-board-revision", board)
        .header(
            "x-boot-ram-window",
            ram_window
                .map(|(base, size)| format!("{base}:{size}"))
                .unwrap_or_default(),
        )
        .body(Body::from(collected))
        .map_err(|err| ApiError::bad_request(format!("cannot build boot response: {err}")))?;
    Ok(response)
}

/// 板子的 **RAM 窗口**（`base:size` 的数值形态）—— `POST /api/boot` 的
/// `x-boot-ram-window` 头就是它。
///
/// ⚠️ **「取哪段 RAM」只有一个落点**：走契约层的 `PlatformDescription::ram_window()`
/// （STAGE-19 裁决 I）⇒ ⇒ 服务端不自己挑段、也不自己算 `base + size` ⇒ ⇒
/// 与内核实际映射的那一段**不可能**漂。
fn boot_ram_window(board: &str) -> Option<(u32, u64)> {
    platform::builtin_description(board)
        .ok()
        .and_then(|description| description.ram_window())
}

/// 把 console 字节写到 `write_to`（**必须**落在 `--allow-image-root` 内）。
///
/// ⚠️ 目标文件通常**还不存在** ⇒ ⇒ 不能直接 `canonicalize` 它；规范化
/// **父目录**，再把文件名接上 ⇒ ⇒ `..` 穿越照样被拒。
fn write_console_bytes(
    root: Option<&std::path::Path>,
    target: &str,
    bytes: &[u8],
) -> Result<(), ApiError> {
    let root = root.ok_or_else(|| {
        ApiError::bad_request("`write_to` needs --allow-image-root (there is no writable root)")
    })?;
    let canonical_root = std::fs::canonicalize(root).map_err(|err| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "E_STORE-07",
            format!("the allow-image-root {root:?} cannot be resolved: {err}"),
        )
    })?;
    let requested = std::path::Path::new(target);
    let parent = requested
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let canonical_parent = std::fs::canonicalize(parent).map_err(|err| {
        ApiError::bad_request(format!(
            "write_to parent {parent:?} cannot be resolved: {err}"
        ))
    })?;
    let file_name = requested
        .file_name()
        .ok_or_else(|| ApiError::bad_request("`write_to` must name a file"))?;
    let resolved = canonical_parent.join(file_name);
    if !resolved.starts_with(&canonical_root) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "E_STORE-07",
            format!("write_to {target:?} is outside the allow-image-root {canonical_root:?}"),
        ));
    }
    std::fs::write(&resolved, bytes).map_err(|err| {
        ApiError::bad_request(format!("write_to {target:?} cannot be written: {err}"))
    })
}

/// 子串查找（字节级）—— 与 `osz-boot` 的 `contains` 同一口径。
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// 暂停会话（控制面）：置位即返回，推进者在下一个窗口边界停。
async fn pause_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let info = state.registry.pause(&id)?;
    Ok(Json(json!({
        "session": SessionPayload::from_info(&info),
        "pause_requested": true,
    })))
}

/// 删会话。**两次删的口径是有意的**：第二次回 404 + `E_SESS-01`（R-02），
/// 而不是 204 —— 客户端据此知道「它已经不在了」。
async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let info = state.registry.delete(&id)?;
    Ok(Json(json!({
        "deleted": SessionPayload::from_info(&info),
    })))
}

/// 远程模式令牌闸门（R-15）。**只在给了令牌时校验**：回环开发不必带 header。
async fn require_token(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let Some(expected) = state.token.as_deref() else {
        return next.run(req).await;
    };
    let presented = req
        .headers()
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());
    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), expected.as_bytes()) => {
            next.run(req).await
        }
        _ => ApiError::unauthorized().into_response(),
    }
}

/// 浏览器跨域（CORS）：页面与服务端天然是**两个 origin**（`front/` 由
/// `tools/serve-front.py` 或任意静态服务器托管，服务端在 `:9876`）⇒ ⇒ 不做
/// 这一层，浏览器在跨域检查那一关就把 `fetch` 拒掉 —— 与「服务好不好」无关。
///
/// ⚠️ **不放 `*`**：放行集合 = 显式 `--cors-origins` ∪ 回环 origin
/// （`{http,https}://{127.0.0.1,localhost,[::1]}[:port]`）。默认只绑回环
/// ⇒ ⇒ 效果是「本机页面能读、外站读不到」，与 R-15 的方向一致；前端换了
/// 主机名/端口之外的位置，才需要往 `--cors-origins` 里加一条。
async fn cors_headers(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let origin = req
        .headers()
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let allowed = origin
        .as_deref()
        .map(|value| cors_origin_allowed(&state.cors_origins, value))
        .unwrap_or(false);

    // 预检：路由表里没有 `OPTIONS`（也不该为每个端点手写一条）⇒ ⇒ 在这里
    // 直接答完，**不进** `next`（否则会被 405 拒掉，浏览器照样读不到集合）。
    let preflight = req.method() == Method::OPTIONS
        && req.headers().contains_key("access-control-request-method");
    if preflight {
        let mut response = if allowed {
            StatusCode::NO_CONTENT.into_response()
        } else {
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "code": "E_TRX-03",
                    "detail": "跨域预检被拒：这个 origin 不在放行集合里（--cors-origins）",
                    "status": 403,
                })),
            )
                .into_response()
        };
        if allowed {
            apply_cors(&mut response, origin.as_deref().unwrap_or_default());
        }
        return response;
    }

    let mut response = next.run(req).await;
    if allowed {
        apply_cors(&mut response, origin.as_deref().unwrap_or_default());
    }
    response
}

/// 把 CORS 响应头写进一条响应（只对**已判定放行**的 origin 调用）。
fn apply_cors(response: &mut Response, origin: &str) {
    use axum::http::header::{HeaderValue, VARY};
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert("access-control-allow-origin", value);
        // ⚠️ **不**发 `access-control-allow-credentials`：令牌走请求头而不是
        // cookie ⇒ ⇒ 不需要凭据模式；发了只会把这个口子往「带 cookie」那边
        // 推一步。
        headers.insert(
            "access-control-allow-methods",
            HeaderValue::from_static(CORS_ALLOW_METHODS),
        );
        headers.insert(
            "access-control-allow-headers",
            HeaderValue::from_static(CORS_ALLOW_HEADERS),
        );
        headers.insert(
            "access-control-expose-headers",
            HeaderValue::from_static(CORS_EXPOSE_HEADERS),
        );
        headers.insert(
            "access-control-max-age",
            HeaderValue::from_static(CORS_MAX_AGE_SECS),
        );
        // 放行集合按 origin 变化 ⇒ ⇒ 中间缓存必须按 `Origin` 分桶。
        headers.insert(VARY, HeaderValue::from_static("origin"));
    }
}

/// 回环 origin 判据（纯函数 ⇒ ⇒ 不必起服务就能单测）。
///
/// 认三种主机名：`127.0.0.1` / `localhost` / `[::1]`，端口随便是谁 —— 本机
/// 任何静态服务器（`serve-front.py` 换过多少个口都行）都算「本机页面」。
fn origin_is_loopback(origin: &str) -> bool {
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return false;
    }
    let host = if let Some(tail) = rest.strip_prefix('[') {
        tail.split_once(']')
            .map(|(host, _)| host)
            .unwrap_or_default()
    } else if let Some((host, _port)) = rest.split_once(':') {
        host
    } else {
        rest
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// origin 放行判据：回环 origin ∪ 显式清单（大小写不敏感，末尾 `/` 无意义）。
fn cors_origin_allowed(allow_list: &[String], origin: &str) -> bool {
    let trimmed = origin.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return false;
    }
    if origin_is_loopback(trimmed) {
        return true;
    }
    allow_list
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(trimmed))
}

/// 定长比较：**不**在第一个不同字节就返回 —— 否则令牌的前缀/长度会从耗时
/// 上漏出去。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 兜底 404：保留方法与路径，便于客户端发现是「打错路径」而不是「会话没了」。
async fn unknown_route(method: Method, uri: axum::http::Uri) -> ApiError {
    ApiError::unknown_route(method.as_str(), uri.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let cfg = Config::default();
        AppState::new(&cfg, "127.0.0.1:9876".parse().unwrap())
    }

    #[test]
    fn version_reports_the_contract_schema_not_a_literal() {
        let payload = version_payload(&state());
        assert_eq!(payload.schema_version, riscv_sim_contract::SCHEMA_VERSION);
        assert_eq!(
            payload.contract_version,
            riscv_sim_contract::CONTRACT_VERSION
        );
        assert_eq!(payload.kernel, riscv_sim_kernel::VERSION);
        assert_eq!(payload.bind, "127.0.0.1:9876");
    }

    #[test]
    fn version_payload_is_stable_json_shape() {
        let payload = version_payload(&state());
        let json = serde_json::to_string(&payload).unwrap();
        for key in [
            "\"kernel\"",
            "\"schema_version\"",
            "\"contract_version\"",
            "\"server\"",
            "\"bind\"",
        ] {
            assert!(json.contains(key), "missing {key} in {json}");
        }
    }

    #[test]
    fn constant_time_eq_matches_only_identical_bytes() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    /// 回环 origin 认三种写法、任意端口；外站 origin 一律不认。
    #[test]
    fn loopback_origins_are_recognised_without_any_config() {
        for origin in [
            "http://127.0.0.1:8000",
            "http://localhost:8000",
            "http://localhost",
            "https://127.0.0.1:443",
            "http://[::1]:5173",
        ] {
            assert!(origin_is_loopback(origin), "{origin} 必须是回环");
        }
        for origin in [
            "http://127.0.0.2:8000",
            "http://10.0.0.5:8000",
            "https://example.com",
            "ftp://127.0.0.1:8000",
            "127.0.0.1:8000",
        ] {
            assert!(!origin_is_loopback(origin), "{origin} 不该被当回环");
        }
    }

    /// 放行集合 = 回环 ∪ 显式清单；大小写与末尾 `/` 不影响判定，外站照样拒。
    #[test]
    fn cors_allow_list_extends_the_loopback_default() {
        let list = vec!["https://sim.example.com".to_string()];
        assert!(cors_origin_allowed(&list, "http://127.0.0.1:8000"));
        assert!(cors_origin_allowed(&list, "https://sim.example.com"));
        assert!(cors_origin_allowed(&list, "https://SIM.example.com/"));
        assert!(
            !cors_origin_allowed(&list, "https://evil.example.com"),
            "不在清单里的外站 origin 必须拒 —— 通配等于把内核交给任何网页"
        );
        assert!(!cors_origin_allowed(&[], "https://evil.example.com"));
    }

    /// console 帧同样是**完整信封**：前端解码器先读 `schema_version`。
    #[test]
    fn console_frames_carry_the_full_contract_envelope() {
        let frame = console_frame("sess-x", 7, b"A");
        assert_eq!(frame.get("type").and_then(|v| v.as_str()), Some("event"));
        assert_eq!(frame.get("seq").and_then(|v| v.as_u64()), Some(7));
        let payload = frame.get("payload").expect("payload 必须在");
        assert_eq!(
            payload.get("schema_version").and_then(|v| v.as_u64()),
            Some(riscv_sim_contract::SCHEMA_VERSION as u64)
        );
        assert_eq!(
            payload.get("direction").and_then(|v| v.as_str()),
            Some("Event")
        );
        let bytes = payload
            .get("payload")
            .and_then(|inner| inner.get("Event"))
            .and_then(|event| event.get("Console"))
            .and_then(|console| console.get("bytes"))
            .and_then(|bytes| bytes.as_array())
            .expect("Event→Console→bytes");
        assert_eq!(
            bytes.iter().filter_map(|v| v.as_u64()).collect::<Vec<_>>(),
            vec![b'A' as u64]
        );
    }

    #[test]
    fn resolve_board_prefers_request_then_startup_default() {
        let (board, source) = resolve_board(Some("qemu-virt".into()), Some("minimal".into()));
        assert_eq!(board.as_deref(), Some("qemu-virt"));
        assert_eq!(source, BoardSource::Request);

        let (board, source) = resolve_board(None, Some("minimal".into()));
        assert_eq!(board.as_deref(), Some("minimal"));
        assert_eq!(source, BoardSource::StartupDefault);

        let (board, source) = resolve_board(None, None);
        assert!(board.is_none());
        assert_eq!(source, BoardSource::None, "无板是合法状态，不是失败");
    }

    #[test]
    fn startup_default_board_reaches_the_session() {
        let cfg = Config {
            board: Some("minimal".to_string()),
            ..Config::default()
        };
        let state = AppState::new(&cfg, "127.0.0.1:9876".parse().unwrap());
        assert_eq!(state.board.as_deref(), Some("minimal"));

        let (board, source) = resolve_board(None, state.board.clone());
        let info = state.registry.create(board, source).unwrap();
        assert_eq!(info.board.as_deref(), Some("minimal"));
        assert_eq!(info.board_source.as_str(), "startup-default");
    }

    /// `plan_run` 的三条拒绝：两个都给、`steps=0`、超上限。
    #[test]
    fn plan_run_rejects_conflicting_or_zero_bounds() {
        let both = plan_run(
            RunRequest {
                steps: Some(3),
                timeout_ms: Some(100),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect_err("steps 与 timeout 不能同时给");
        assert_eq!(both.status(), StatusCode::BAD_REQUEST);

        let zero = plan_run(
            RunRequest {
                steps: Some(0),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect_err("steps=0 没有意义");
        assert_eq!(zero.status(), StatusCode::BAD_REQUEST);

        let too_long = plan_run(
            RunRequest {
                timeout_ms: Some(30_000),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect_err("超上限必须拒，不能静默夹");
        assert_eq!(too_long.status(), StatusCode::BAD_REQUEST);
    }

    /// 缺省行为：不写 `timeout_ms` ⇒ 用 `--max-run-ms`；写 `steps` ⇒ 不带墙钟。
    #[test]
    fn plan_run_defaults_to_the_process_limit() {
        assert_eq!(
            plan_run(RunRequest::default(), 5_000).expect("缺省合法"),
            RunSpec {
                steps: None,
                timeout_ms: Some(5_000),
                prefix: None,
                until: None,
            }
        );
        assert_eq!(
            plan_run(
                RunRequest {
                    steps: Some(7),
                    ..RunRequest::default()
                },
                5_000
            )
            .expect("步进合法"),
            RunSpec {
                steps: Some(7),
                timeout_ms: None,
                prefix: None,
                until: None,
            }
        );
    }

    /// 断点三选一：只给一个才通过；给零个 / 两个都拒。
    #[test]
    fn plan_breakpoint_requires_exactly_one_selector() {
        assert_eq!(
            plan_breakpoint(BreakpointRequest {
                addr: Some(0x20),
                ..Default::default()
            })
            .expect("只给地址"),
            BreakpointPlan::Address(0x20)
        );
        assert_eq!(
            plan_breakpoint(BreakpointRequest {
                symbol: Some(" main ".into()),
                ..Default::default()
            })
            .expect("只给符号（首尾空白要剪掉）"),
            BreakpointPlan::Symbol("main".into())
        );
        assert!(matches!(
            plan_breakpoint(BreakpointRequest {
                condition: Some("x1 == 0".into()),
                ..Default::default()
            })
            .expect("只给条件"),
            BreakpointPlan::Condition(_)
        ));

        for request in [
            BreakpointRequest::default(),
            BreakpointRequest {
                addr: Some(1),
                symbol: Some("main".into()),
                condition: None,
            },
            BreakpointRequest {
                symbol: Some("   ".into()),
                ..Default::default()
            },
            BreakpointRequest {
                condition: Some("  ".into()),
                ..Default::default()
            },
        ] {
            let err = plan_breakpoint(request).expect_err("非法组合必须拒");
            assert_eq!(err.status(), StatusCode::BAD_REQUEST);
        }
    }

    /// 符号条数：缺省 512、`0` 就是空表、超上限夹到 4096。
    #[test]
    fn plan_symbol_limit_defaults_and_clamps() {
        assert_eq!(plan_symbol_limit(None), 512);
        assert_eq!(plan_symbol_limit(Some(0)), 0);
        assert_eq!(plan_symbol_limit(Some(16)), 16);
        assert_eq!(plan_symbol_limit(Some(99_999)), 4_096);
    }

    /// 前缀判据：字节级子串（与 `--prefix` 同一口径）。
    #[test]
    fn boot_contains_matches_a_byte_substring() {
        assert!(contains(b"abcOSZ$ def", b"OSZ$ "));
        assert!(!contains(b"abc", b"OSZ$ "));
        assert!(contains(b"abc", b""));
    }

    /// R-12：回执头 `x-boot-ram-window` = 板子的 RAM 窗口（`base:size`，十进制）。
    ///
    /// ⚠️ **取哪段 RAM** 走契约层的 <code>ram_window()</code>（STAGE-19 裁决 I 的
    ///    唯一落点）⇒ ⇒ 这条用例钉的是「头里那串数就是它」——不是本服务端另算
    ///    一遍（另算一遍就会与内核映射的那一段漂）。
    #[test]
    fn boot_ram_window_comes_from_the_board_description() {
        let format_window = |board: &str| {
            boot_ram_window(board)
                .map(|(base, size)| format!("{base}:{size}"))
                .unwrap_or_default()
        };
        // qemu-virt 的高段 RAM 是 0x8000_0000 + 128 MiB ⇒ 十进制两串数。
        assert_eq!(format_window("qemu-virt"), "2147483648:134217728");
        // default 的高段是 0x8000_0000 + 4 MiB。
        assert_eq!(format_window("default"), "2147483648:4194304");
        // 未知模板 ⇒ 空串（头照样在，脚本会判成「这块板没 RAM」）。
        assert_eq!(format_window("nope"), "");
    }

    /// R-05 的 `prefix` / `until` 折算：合法取值通过，冲突与空前缀都拒。
    #[test]
    fn plan_run_accepts_prefix_and_until_but_rejects_conflicts() {
        let prefix = plan_run(
            RunRequest {
                prefix: Some("OSZ$ ".to_string()),
                timeout_ms: Some(30_000),
                ..RunRequest::default()
            },
            5_000,
        );
        // 超上限要拒（不是夹到 5000）。
        assert!(prefix.is_err());

        let ok = plan_run(
            RunRequest {
                prefix: Some("OSZ$ ".to_string()),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect("前缀 + 缺省熔断合法");
        assert_eq!(ok.prefix.as_deref(), Some("OSZ$ "));
        assert_eq!(ok.timeout_ms, Some(5_000));

        let until = plan_run(
            RunRequest {
                until: Some("breakpoint".to_string()),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect("until 合法");
        assert_eq!(until.until, Some(RunUntil::Breakpoint));

        let bad_until = plan_run(
            RunRequest {
                until: Some("trap".to_string()),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect_err("未知 until 必须拒");
        assert_eq!(bad_until.status(), StatusCode::BAD_REQUEST);

        let until_and_steps = plan_run(
            RunRequest {
                until: Some("halt".to_string()),
                steps: Some(3),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect_err("`steps` 与 `until` 不能同时给");
        assert_eq!(until_and_steps.status(), StatusCode::BAD_REQUEST);

        let empty = plan_run(
            RunRequest {
                prefix: Some(String::new()),
                ..RunRequest::default()
            },
            5_000,
        )
        .expect_err("空前缀会立刻命中");
        assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    }

    /// R-07 的 `format` 视图：`u32` 小端字、`asc` 可打印兜底。
    #[test]
    fn memory_view_renders_words_and_ascii() {
        let bytes = [0x01, 0x00, 0x00, 0x00, b'a', b'b', 0x00, 0x7f];
        let (words, ascii) = memory_view(&bytes, "u32").expect("u32 视图");
        assert_eq!(words, Some(vec![1, 0x7F00_6261]));
        assert!(ascii.is_none());

        let (_, ascii) = memory_view(&bytes, "asc").expect("asc 视图");
        assert_eq!(ascii.as_deref(), Some("....ab.."));

        let bad = memory_view(&bytes, "hex").expect_err("未知视图必须拒");
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    }

    /// R-08 的观察点参数：宽度与方向只认那三个词。
    #[test]
    fn watchpoint_size_and_access_are_validated() {
        assert_eq!(parse_write_size(None).expect("缺省 word"), WriteSize::Word);
        assert_eq!(
            parse_write_size(Some("half")).expect("half"),
            WriteSize::Half
        );
        assert!(parse_write_size(Some("quad")).is_err());
        assert_eq!(
            parse_watch_access(None).expect("缺省 write"),
            WatchAccess::Write
        );
        assert_eq!(
            parse_watch_access(Some("either")).expect("either"),
            WatchAccess::Either
        );
        assert!(parse_watch_access(Some("execute")).is_err());
    }

    /// 契约命令面只收命令：事件方向的消息必须 400（否则「事件上送」会静默
    /// 变成一条命令，命令面就成了双向糊在一起的通道）。
    #[test]
    fn contract_command_refuses_event_direction() {
        let command = ContractMessage::command(0, CommandPayload::Nop);
        assert!(ensure_command_direction(&command).is_ok());

        let event = ContractMessage::event(
            0,
            riscv_sim_contract::protocol::EventPayload::Tick {
                elapsed_ps: 0,
                pc: 0,
            },
        );
        let err = ensure_command_direction(&event).expect_err("事件方向必须拒");
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }
}
