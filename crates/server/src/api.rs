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

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use riscv_sim_contract::platform::PlatformDescription;
use riscv_sim_kernel::kernel_host::platform;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{Config, DEFAULT_MAX_CONSOLE};
use crate::error::ApiError;
use crate::session::{BoardSource, SessionInfo, SessionRegistry};
use crate::upload;

/// 远程模式的令牌头名（R-15）。
const TOKEN_HEADER: &str = "x-auth-token";

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
            token: cfg.token.clone(),
            registry: Arc::new(SessionRegistry::new(cfg.max_sessions, cfg.session_ttl)),
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
}
