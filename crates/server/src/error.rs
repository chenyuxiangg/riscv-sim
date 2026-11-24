//! 统一错误体与 HTTP 映射（M-24 · R-09 · 表 9）。
//!
//! 线形状固定为 `{code, detail, status}`：`code` 是**契约里已有的**场景码
//! （`E_<FAMILY>-<NN>`），`detail` 是人能读的定位信息，`status` 与 HTTP 状态
//! 码同值 —— 客户端只看 body 就能决定重试还是重建，不必解析 header。
//!
//! ⚠️ **「码 → 状态」不是纯函数**。表 9 里 `E_PROTO-08` 既是 400（载荷形状
//! 不对）又是 413（体积超限），`E_PLT-22` 既是 404（未知模板）又是 409（无板
//! 装载）⇒ ⇒ 状态由**发生点在构造时显式给出**，不做「按码反查状态」的映射表
//! ⇒ ⇒ 反查表会把两件事重新合并成一件。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use riscv_sim_contract::error::{
    ErrorFamily, E_SESS_01_NOT_FOUND, E_SESS_02_BUSY, E_SESS_03_POISONED,
    E_SESS_05_QUOTA_EXCEEDED, E_SESS_07_UNAUTHORIZED,
};
use serde::Serialize;

/// 线上错误体。字段顺序固定，便于 `jq` 与逐字节对拍。
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// 契约场景码，例如 `E_PROTO-08`。
    pub code: String,
    /// 定位信息。**必须有内容** —— 空 detail 的 500 是禁止形状（R-09）。
    pub detail: String,
    /// 与 HTTP 状态码同值。
    pub status: u16,
}

/// 一个已定位的 API 错误。
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: String,
    detail: String,
}

impl ApiError {
    /// 显式给出状态与码。**不要**在别处按码反查状态，理由见模块注释。
    pub fn new(status: StatusCode, code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            detail: detail.into(),
        }
    }

    /// 404 · 路由不存在。**不是**「会话不存在」—— 那个是 `E_SESS-01`，
    /// 见 [`ApiError::session_not_found`]；这里只是没人接这条路径。
    pub fn unknown_route(method: &str, path: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "E_PROTO-08",
            format!("no endpoint for {method} {path} (see GET /api/version)"),
        )
    }

    /// 404 · 会话不存在（R-02：从未存在、已删除、或 TTL 回收）。
    pub fn session_not_found(id: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            session_code(E_SESS_01_NOT_FOUND),
            format!("session {id} does not exist; POST /api/sessions to create one"),
        )
    }

    /// 409 · 会话忙（已有推进者）。
    pub fn session_busy(id: &str) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            session_code(E_SESS_02_BUSY),
            format!("session {id} is already running a command; wait or POST .../pause first"),
        )
    }

    /// 500 · 会话中毒（线程 panic / 已退出）。**detail 非空**是硬要求。
    pub fn session_poisoned(id: &str) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            session_code(E_SESS_03_POISONED),
            format!("session {id}'s worker is gone (panicked or terminated); recreate the session"),
        )
    }

    /// 429 · 并发会话配额用尽。
    pub fn session_quota(max: usize) -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            session_code(E_SESS_05_QUOTA_EXCEEDED),
            format!("session quota {max} is in use; DELETE an idle session, then retry"),
        )
    }

    /// 401 · 远程模式缺令牌（detail 不回显期望值）。
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            session_code(E_SESS_07_UNAUTHORIZED),
            "this endpoint requires an X-Auth-Token header".to_string(),
        )
    }

    /// 本文档用的 HTTP 状态码（**仅测试用**：生产路径只写不读，
    /// 免得同一份判据出现第二个消费方）。
    #[cfg(test)]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// 本文档用的场景码（**仅测试用**）。
    #[cfg(test)]
    pub fn code(&self) -> &str {
        &self.code
    }
}

/// 会话族的码**从契约常量拼出来**，不在这里抄字面量 —— 抄一份就是第二个
/// 真相，改名时必漂。
fn session_code(scenario: u16) -> String {
    format!(
        "E_{}-{:02}",
        ErrorFamily::Session.as_str(),
        scenario
    )
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = ErrorBody {
            code: self.code,
            detail: self.detail,
            status: status.as_u16(),
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_route_points_at_the_version_endpoint() {
        let err = ApiError::unknown_route("GET", "/api/nope");
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert_eq!(err.code, "E_PROTO-08");
        assert!(err.detail.contains("/api/version"));
    }

    #[test]
    fn body_carries_status_code_and_detail() {
        let err = ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_PROTO-08",
            "field `steps` must be a u64",
        );
        let body = ErrorBody {
            code: err.code.clone(),
            detail: err.detail.clone(),
            status: err.status.as_u16(),
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"code\":\"E_PROTO-08\""));
        assert!(json.contains("\"status\":400"));
        assert!(json.contains("steps"));
    }

    #[test]
    fn detail_is_never_empty_in_the_constructed_bodies() {
        // 空 detail 的 500 是禁止形状（R-09）；`internal` 构造器要等
        // `E_SESS-03` 落地之后再提供 —— 现在不发明新码。
        let bad = ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_PROTO-08",
            "field `steps` must be a u64",
        );
        let missing = ApiError::unknown_route("POST", "/api/boot");
        assert!(!bad.detail.trim().is_empty());
        assert!(!missing.detail.trim().is_empty());
        assert_eq!(bad.status, StatusCode::BAD_REQUEST);
        assert_eq!(missing.status, StatusCode::NOT_FOUND);
    }
}
