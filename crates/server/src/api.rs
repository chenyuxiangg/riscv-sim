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
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use riscv_sim_contract::model::{Breakpoint, BreakpointKind};
use riscv_sim_contract::platform::PlatformDescription;
use riscv_sim_contract::protocol::CommandPayload;
use riscv_sim_kernel::kernel_host::platform;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{Config, DEFAULT_MAX_CONSOLE};
use crate::error::ApiError;
use crate::session::{BoardSource, SessionInfo, SessionRegistry};
use crate::upload;

/// 远程模式的令牌头名（R-15）。
const TOKEN_HEADER: &str = "x-auth-token";

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
    /// `run` 一次请求的墙钟上限（`--max-run-ms`）。
    pub max_run_ms: u64,
    /// 远程模式令牌；`None` ⇒ 不校验。
    pub token: Option<String>,
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
            max_run_ms: cfg.max_run_ms,
            token: cfg.token.clone(),
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
        .route("/api/sessions/{id}/board", put(set_board))
        .route("/api/sessions/{id}/image", post(load_image))
        .route("/api/sessions/{id}/run", post(run_session))
        .route("/api/sessions/{id}/step", post(step_session))
        .route("/api/sessions/{id}/console", get(read_console))
        .route("/api/sessions/{id}/state", get(session_state))
        .route("/api/sessions/{id}/memory", get(read_memory_range))
        .route("/api/sessions/{id}/symbols", get(list_symbols))
        .route("/api/sessions/{id}/disasm", get(disassemble))
        .route(
            "/api/sessions/{id}/breakpoints",
            get(list_breakpoints).post(set_breakpoint),
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

/// `POST …/run` 的请求体：**整份可选**。
#[derive(Debug, Default, Deserialize)]
pub struct RunRequest {
    /// 指令上限（不给 ⇒ 只看墙钟）。
    #[serde(default)]
    pub steps: Option<u32>,
    /// 墙钟上限（毫秒）；不给 ⇒ 用 `--max-run-ms`。
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// 把请求折成 `run_until` 的两个参数（**纯函数** ⇒ ⇒ 三条校验可单测）。
///
/// 规则：
/// - `steps = 0` ⇒ 400（「跑 0 条」没有意义，且会让调用方以为跑过）；
/// - **`steps` 与 `timeout_ms` 同时给 ⇒ 400**：`StepN` 是「恰好 n 条」的
///   调试器语义，套墙钟会让它变成「跑了多少算多少」⇒ ⇒ 两件事不能混；
/// - `timeout_ms = 0` ⇒ 400；`> --max-run-ms` ⇒ 400（**不静默夹到上限**，
///   否则客户端以为它要的 30 s 生效了）。
pub fn plan_run(
    request: RunRequest,
    max_run_ms: u64,
) -> Result<(Option<u32>, Option<u64>), ApiError> {
    if let (Some(_), Some(_)) = (request.steps, request.timeout_ms) {
        return Err(ApiError::bad_request(
            "give either `steps` or `timeout_ms`, not both (step semantics are exact-n)",
        ));
    }
    if let Some(steps) = request.steps {
        if steps == 0 {
            return Err(ApiError::bad_request("field `steps` must be >= 1"));
        }
        return Ok((Some(steps), None));
    }
    let timeout_ms = request.timeout_ms.unwrap_or(max_run_ms);
    if timeout_ms == 0 {
        return Err(ApiError::bad_request("field `timeout_ms` must be >= 1"));
    }
    if timeout_ms > max_run_ms {
        return Err(ApiError::bad_request(format!(
            "timeout_ms {timeout_ms} exceeds --max-run-ms ({max_run_ms}); raise the limit or lower the request"
        )));
    }
    Ok((None, Some(timeout_ms)))
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
    let (steps, timeout_ms) = plan_run(request, state.max_run_ms)?;
    let report = state.registry.run(&id, steps, timeout_ms)?;
    if report.get("stop_reason").and_then(|value| value.as_str()) == Some("timeout") {
        let mut timeout_body = report;
        timeout_body["code"] = json!(format!("E_{}-06", riscv_sim_contract::error::ErrorFamily::Session.as_str()));
        let limit_ms = timeout_ms.unwrap_or(state.max_run_ms);
        timeout_body["detail"] = json!(format!(
            "run hit the wall-clock limit ({limit_ms} ms); the guest is stopped, see `executed` / `pc`"
        ));
        timeout_body["status"] = json!(504);
        return Ok((StatusCode::GATEWAY_TIMEOUT, Json(timeout_body)).into_response());
    }
    Ok(Json(report).into_response())
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
    let report = state.registry.run(&id, Some(request.n), None)?;
    Ok(Json(report))
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
    let base = body.get("base").and_then(|value| value.as_u64()).unwrap_or(0);
    let total = body.get("total").and_then(|value| value.as_u64()).unwrap_or(0);
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
    Ok(Json(state.registry.debug(
        &id,
        CommandPayload::DumpMemory { addr, len },
    )?))
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
    Ok(Json(state.registry.debug(
        &id,
        CommandPayload::Disasm { addr, count },
    )?))
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
        .map_err(|err| ApiError::bad_request(format!("image path {path:?} cannot be read: {err}")))?;
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
        .create(board, crate::session::BoardSource::Request)?;
    let id = info.id.clone();

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
    let mut stop = String::from("budget");

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
        let report = state.registry.run(id, Some(chunk), None)?;
        let ran = report
            .get("executed")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        executed += ran;
        remaining = remaining.saturating_sub(u32::try_from(ran).unwrap_or(chunk));

        // 取这一窗口新产生的 console 字节。
        let console = state.registry.console(id, console_offset, BOOT_CHUNK as usize)?;
        let base = console.get("base").and_then(|value| value.as_u64()).unwrap_or(0);
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
        .body(Body::from(collected))
        .map_err(|err| ApiError::bad_request(format!("cannot build boot response: {err}")))?;
    Ok(response)
}

/// 把 console 字节写到 `write_to`（**必须**落在 `--allow-image-root` 内）。
///
/// ⚠️ 目标文件通常**还不存在** ⇒ ⇒ 不能直接 `canonicalize` 它；规范化
/// **父目录**，再把文件名接上 ⇒ ⇒ `..` 穿越照样被拒。
fn write_console_bytes(root: Option<&std::path::Path>, target: &str, bytes: &[u8]) -> Result<(), ApiError> {
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
        ApiError::bad_request(format!("write_to parent {parent:?} cannot be resolved: {err}"))
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
    haystack.windows(needle.len()).any(|window| window == needle)
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

    #[test]
    fn resolve_board_prefers_request_then_startup_default() {
        let (board, source) =
            resolve_board(Some("qemu-virt".into()), Some("minimal".into()));
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
        let mut cfg = Config::default();
        cfg.board = Some("minimal".to_string());
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
            },
            5_000,
        )
        .expect_err("steps 与 timeout 不能同时给");
        assert_eq!(both.status(), StatusCode::BAD_REQUEST);

        let zero = plan_run(
            RunRequest {
                steps: Some(0),
                timeout_ms: None,
            },
            5_000,
        )
        .expect_err("steps=0 没有意义");
        assert_eq!(zero.status(), StatusCode::BAD_REQUEST);

        let too_long = plan_run(
            RunRequest {
                steps: None,
                timeout_ms: Some(30_000),
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
            (None, Some(5_000))
        );
        assert_eq!(
            plan_run(
                RunRequest {
                    steps: Some(7),
                    timeout_ms: None
                },
                5_000
            )
            .expect("步进合法"),
            (Some(7), None)
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

    /// 前缀判据：字节级子串（与 `osz-boot --prefix` 同一口径）。
    #[test]
    fn boot_contains_matches_a_byte_substring() {
        assert!(contains(b"abcOSZ$ def", b"OSZ$ "));
        assert!(!contains(b"abc", b"OSZ$ "));
        assert!(contains(b"abc", b""));
    }
}
