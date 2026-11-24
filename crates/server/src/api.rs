//! HTTP 面（M-24 · §3.1）。P0-1 只提供两条端点：`GET /api/version` 与
//! `GET /healthz`，以及一条把「没人接的路径」变成**标准错误体**的兜底。
//!
//! ⚠️ 兜底**不用** axum 的默认 404（那是空 body 的纯文本）—— R-09 要的是
//! `{code, detail, status}`，客户端拿到默认 404 只能再猜一次。

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::Method;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;

use crate::config::{Config, DEFAULT_MAX_CONSOLE};
use crate::error::ApiError;

/// 全局只读状态。**没有**会话表 —— 那是 P0-2 的事；这里只放「进程身份」，
/// 让 `/api/version` 与 `/healthz` 能自证「我是谁、绑在哪、等多久回收」。
#[derive(Debug, Clone)]
pub struct AppState {
    /// 进程启动时刻（算 uptime 用）。
    pub started: Instant,
    /// **实际**绑定的地址。要是 `--port` 被系统改写（P0-1 不允许 0，但仍以
    /// `local_addr` 为准）⇒ ⇒ 回给客户端的是真地址，不是请求的那个。
    pub bind: SocketAddr,
    /// 启动默认板（可能为 `None`）。
    pub board: Option<String>,
    /// 会话空闲回收时长。
    pub session_ttl: Duration,
    /// 每会话 console 缓冲上限。
    pub max_console: usize,
}

impl AppState {
    /// 由配置与实际绑定地址构造。
    pub fn new(cfg: &Config, bind: SocketAddr) -> Self {
        Self {
            started: Instant::now(),
            bind,
            board: cfg.board.clone(),
            session_ttl: cfg.session_ttl,
            max_console: cfg.max_console,
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
    /// 实际监听地址，方便 Agent 发现「我连的是不是我以为的那个」。
    pub bind: String,
}

/// 组装 `/api/version` 的回体。
///
/// ⚠️ 三个版本字段**各自取自己的真相源**，不在这里抄字面量：
/// `kernel` ← 内核 crate、`schema_version` ← 契约信封、`contract_version` ←
/// 契约 crate 版本。抄字面量就会在下次升版时静默过期。
pub fn version_payload(state: &AppState) -> VersionPayload {
    VersionPayload {
        kernel: riscv_sim_kernel::VERSION,
        schema_version: riscv_sim_contract::SCHEMA_VERSION,
        contract_version: riscv_sim_contract::CONTRACT_VERSION,
        server: env!("CARGO_PKG_VERSION"),
        bind: state.bind.to_string(),
    }
}

/// 装配路由。P0-2 之后会话端点挂在这里，**不动**这两条。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/version", get(version))
        .route("/healthz", get(healthz))
        .fallback(unknown_route)
        .with_state(state)
}

async fn version(State(state): State<AppState>) -> Json<VersionPayload> {
    Json(version_payload(&state))
}

async fn healthz(State(state): State<AppState>) -> Json<serde_json::Value> {
    let uptime_ms = state.started.elapsed().as_millis() as u64;
    // 会话数目前恒为 0（会话表在 P0-2）；`sessions` 先占位，避免客户端按
    // 「字段不存在」写分支。
    Json(json!({
        "status": "ok",
        "uptime_ms": uptime_ms,
        "sessions": 0,
        "board": state.board,
        "session_ttl_s": state.session_ttl.as_secs(),
        "max_console": if state.max_console == 0 { DEFAULT_MAX_CONSOLE } else { state.max_console },
        "bind": state.bind.to_string(),
    }))
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
}
