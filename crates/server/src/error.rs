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
    ErrorFamily, E_SESS_01_NOT_FOUND, E_SESS_02_BUSY, E_SESS_03_POISONED, E_SESS_05_QUOTA_EXCEEDED,
    E_SESS_06_TIMEOUT, E_SESS_07_UNAUTHORIZED,
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

    /// 429 · **单账号**并发会话配额用尽（M-25）。
    ///
    /// ⚠️ 与全局配额**共用场景码** `E_SESS-05`（契约里只有一个「配额」概念），
    /// 靠 detail 区分是「进程满了」还是「你自己满了」—— 客户端要不要重试的
    /// 判断是同一件事：先删自己的空闲会话。
    pub fn account_quota(max: usize) -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            session_code(E_SESS_05_QUOTA_EXCEEDED),
            format!(
                "account session quota {max} is in use; DELETE one of your sessions, then retry"
            ),
        )
    }

    /// 504 · 作业回执超时（R-11）。
    ///
    /// ⚠️ **不是** `run` 的 `timeout_ms`（那个是内核窗口判定回的
    /// `Halted { timeout }`）⇒ ⇒ 这里是**服务端**等不到回执的兜底，两层分开。
    pub fn session_timeout(id: &str) -> Self {
        Self::new(
            StatusCode::GATEWAY_TIMEOUT,
            session_code(E_SESS_06_TIMEOUT),
            format!("session {id}'s worker did not answer in time; the session may be wedged"),
        )
    }

    /// 400 · 请求载荷形状不对（`E_PROTO-08` 在「坏请求」这个发生点）。
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "E_PROTO-08", detail)
    }

    /// 413 · 上传的镜像超过上限（`E_PROTO-08` 在「太大」这个发生点）。
    pub fn image_too_large(limit: usize) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "E_PROTO-08",
            format!("image exceeds --max-image-bytes ({limit} bytes); raise the limit or trim the image"),
        )
    }

    /// 404 · 板模板不存在（`E_PLT-22` 在「未知模板」这个发生点）。
    pub fn template_not_found(id: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            format!("E_{}-22", ErrorFamily::Platform.as_str()),
            format!(
                "no builtin board template {id:?}; GET /api/board/templates lists the known ids"
            ),
        )
    }

    /// 403 · 镜像路径不在 `--allow-image-root` 白名单里。
    ///
    /// ⚠️ 契约层**没有** IO 族 ⇒ ⇒ 借用既有的 `STORE` 族第 7 号
    /// （`E_STORE-07`，本 crate 内定义）⇒ ⇒ **不**新开 IO 族（设计裁决）。
    pub fn image_path_outside_root(path: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "E_STORE-07",
            format!("image path {path:?} is outside --allow-image-root; upload the bytes instead"),
        )
    }

    /// 401 · 远程模式缺令牌（detail 不回显期望值）。
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            session_code(E_SESS_07_UNAUTHORIZED),
            "this endpoint requires a token (Authorization: Bearer or X-Auth-Token) \
             or a browser login (POST /api/login)"
                .to_string(),
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

    /// 本文档用的定位信息（**仅测试用**）。
    #[cfg(test)]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// 配板发生点：内核码 ⇒ 状态。
///
/// ⚠️ `E_PLT-13`（修订号不符）是**并发冲突** ⇒ 409；其余（描述校验失败
/// `E_PLT-01..16`、未知板 `E_PLT-22` 之外的平台错误）都是「请求不对」⇒ 400。
///
/// ⚠️ **未知模板是 404**，但那条由 [`ApiError::template_not_found`] 在
/// 服务端查模板时就给出（内核根本不会被问到）⇒ ⇒ 这里不会收到那个场景。
pub fn board_failure(code: &str, detail: &str) -> ApiError {
    let status = if code == "E_PLT-13" {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    ApiError::new(status, code, detail)
}

/// 复位发生点（R-14）：**没装过镜像** ⇒ `E_SESS-02` ⇒ **409**（状态冲突，
/// 不是请求错）；其余沿用 [`load_failure`] 的装载口径（镜像还在，只是重装
/// 失败 ⇒ 与首次装载同一个码、同一个状态）。
pub fn reset_failure(code: &str, detail: &str) -> ApiError {
    match code {
        "E_SESS-02" => ApiError::new(StatusCode::CONFLICT, code, detail),
        _ => load_failure(code, detail),
    }
}

/// 读 console 发生点：`E_SESS-04`（窗口丢失）⇒ **409**（客户端要重新对齐，
/// 不是请求错）⇒ 其余 400。
pub fn console_failure(code: &str, detail: &str) -> ApiError {
    let status = if code == "E_SESS-04" {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    ApiError::new(status, code, detail)
}

/// 装载发生点：内核码 ⇒ 状态（表 9）。
///
/// - `E_PLT-22`（无板装载）⇒ **409**（不是 404：板不存在不在这里说，见上）；
/// - `E_PLT-17`（镜像落在板外）⇒ **422**：请求合法，内容装不上；
/// - `E_LDR-*`（ELF 畸形 / 超限 / 段溢出）⇒ **422**：同上；
/// - `E_PLT-13`（修订号不符）⇒ **409**；
/// - 其余 ⇒ **400**。
pub fn load_failure(code: &str, detail: &str) -> ApiError {
    let status = if code == "E_PLT-22" || code == "E_PLT-13" {
        StatusCode::CONFLICT
    } else if code == "E_PLT-17" || code.starts_with("E_LDR-") {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::BAD_REQUEST
    };
    ApiError::new(status, code, detail)
}

/// 调试读面（R-07）：`E_DBG-03`（转储长度越界 / 非法）⇒ **400**，
/// `E_DBG-02`（符号名 / 表达式非法）⇒ **400**，其余 ⇒ 400。
///
/// ⚠️ 没有 422：读面**不改任何状态** ⇒ ⇒ 「请求不成立」只能是 400，
/// 不该借用「内容装不上」那个档。
pub fn debug_failure(code: &str, detail: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, code, detail)
}

/// 快照发生点：`capture_session` / `restore_session` 的失败 ⇒ **400**。
///
/// ⚠️ 快照族的码（`E_SNAP-01` 版本不支持、队列不合法…）都是「**载荷本身**
/// 不对」⇒ ⇒ 400 而不是 409。**409 只留给并发的修订号冲突**（`E_PLT-13`）。
pub fn snapshot_failure(code: &str, detail: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, code, detail)
}

/// 会话族的码**从契约常量拼出来**，不在这里抄字面量 —— 抄一份就是第二个
/// 真相，改名时必漂。
fn session_code(scenario: u16) -> String {
    format!("E_{}-{:02}", ErrorFamily::Session.as_str(), scenario)
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
