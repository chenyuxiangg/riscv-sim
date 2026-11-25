//! 前端静态资源托管（M-24 · 「一个端口」的那一半）。
//!
//! ## 它解决什么
//!
//! 页面（`front/`）与 API 天然是两个 origin：不带这一层，用户得**另起**一个
//! 静态服务器（`tools/serve-front.py`），还会撞上 CORS。`--front-dir` 一给，
//! 同一个端口既回页面又回 API ⇒ 同源请求不走跨域检查，`?server=` 也不必手写
//! （前端会探测同源的 `GET /api/version`，见 `front/services/m07_servermode.js`）。
//!
//! ## 三条硬约束
//!
//! 1. **不遮蔽 API**：`/api/*` 与 `/healthz` 走各自的路由；落到这里的请求才看文件。
//! 2. **不出根**：路径按段规范化（拒 `..`、拒反斜杠、拒任何 `%` 转义），
//!    解析后还要 `canonicalize` 并确认仍在根内 —— 软链接也不许把根外的文件带进来。
//! 3. **不许陈旧**：一律 `Cache-Control: no-store`。前端是**零构建**的 ES module，
//!    缓存旧模块的后果是「改了没生效」甚至整页空白（链接期全有或全无），
//!    这一点在 `docs/USER_GUIDE.md` 第 6 章记过账 ⇒ 宁可多传几 KB。
//!
//! ⚠️ **只认 `GET` / `HEAD`**：静态目录不该有写语义。`HEAD` 回字节数但不回体。

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Method, StatusCode};
use axum::response::Response;

/// 命中一个静态文件；`None` 表示「这个路径没有对应文件」（由调用方回 404）。
///
/// 返回的路径**已经**`canonicalize`过，且保证落在根内 ⇒ 调用方可以放心读。
pub fn resolve(root: &Path, raw_path: &str) -> Option<PathBuf> {
    // 1) 路径段规范化。`%` 一律拒：前端产物里没有任何需要转义的字符，
    //    放行等于把「解码」这件事交给下游，而解码器正是穿越漏洞的老家。
    if raw_path.contains('%') || raw_path.contains('\\') {
        return None;
    }
    let mut rel = PathBuf::new();
    for seg in raw_path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return None;
        }
        rel.push(seg);
    }
    // 2) `/` 与空路径都落到 `index.html`（页面是 `#/xxx` 哈希路由，不需要 SPA 回退）。
    if rel.as_os_str().is_empty() {
        rel.push("index.html");
    }

    let target = root.join(&rel);
    // 3) 软链接也可能指向根外 ⇒ 规范化后必须仍在根内。
    let canonical = target.canonicalize().ok()?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return None;
    }
    Some(canonical)
}

/// 按扩展名给 `Content-Type`。未知扩展名一律 `application/octet-stream`
/// ——**不猜**（猜错的 MIME 比不猜更糟：浏览器会按错的类型解释字节）。
pub fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "map" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 把一份字节包成响应。`head_only` ⇒ 只给头（`HEAD` 的语义）。
fn file_response(bytes: Vec<u8>, content_type: &'static str, head_only: bool) -> Response {
    let len = bytes.len();
    let body = if head_only {
        Body::empty()
    } else {
        Body::from(bytes)
    };
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        header::HeaderValue::from_static("nosniff"),
    );
    if head_only {
        // `HEAD` 仍要报出「本来有多少字节」（`Content-Length` 不许为空）。
        headers.insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from_str(&len.to_string()).unwrap_or_else(|_| {
                header::HeaderValue::from_static("0")
            }),
        );
    }
    // 状态码留在最后写，避免上面几个 `insert` 之前有人误读默认值。
    *response.status_mut() = StatusCode::OK;
    response
}

/// 读一个已解析的文件并回给客户端。读失败 ⇒ 500（**不**伪装成 404：
/// 「文件不见了」与「读不出来」是两件事，后者要人去看权限/磁盘）。
pub async fn serve(root: &Path, raw_path: &str, method: &Method) -> Response {
    if method != Method::GET && method != Method::HEAD {
        let mut response = Response::new(Body::from("method not allowed\n"));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
        return response;
    }
    let head_only = method == Method::HEAD;
    let Some(file) = resolve(root, raw_path) else {
        let mut response = Response::new(Body::from("not found\n"));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        *response.status_mut() = StatusCode::NOT_FOUND;
        return response;
    };
    match tokio::fs::read(&file).await {
        Ok(bytes) => file_response(bytes, content_type_for(&file), head_only),
        Err(err) => {
            let mut response =
                Response::new(Body::from(format!("cannot read {}: {err}\n", file.display())));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("text/plain; charset=utf-8"),
            );
            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        // 用仓内真实的 `front/`：测的是「这份目录能不能被托管」，不是假目录。
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../front")
            .canonicalize()
            .expect("front/ must exist")
    }

    #[test]
    fn root_path_maps_to_index_html() {
        let root = dir();
        assert_eq!(
            resolve(&root, "/").expect("index.html"),
            root.join("index.html")
        );
    }

    #[test]
    fn nested_asset_path_is_served() {
        let root = dir();
        assert_eq!(
            resolve(&root, "/services/m07_transport.js").expect("module"),
            root.join("services/m07_transport.js")
        );
    }

    #[test]
    fn parent_segments_are_refused_not_normalized_away() {
        let root = dir();
        assert!(resolve(&root, "/../Cargo.toml").is_none());
        assert!(resolve(&root, "/assets/../../Cargo.toml").is_none());
    }

    #[test]
    fn percent_escapes_are_refused() {
        let root = dir();
        // `%2e%2e` 解出来就是 `..` —— 在**解码之前**就拒掉。
        assert!(resolve(&root, "/%2e%2e/Cargo.toml").is_none());
        assert!(resolve(&root, "/a%20b.js").is_none());
    }

    #[test]
    fn backslashes_are_refused() {
        let root = dir();
        assert!(resolve(&root, "/..\\Cargo.toml").is_none());
    }

    #[test]
    fn missing_file_is_a_miss_not_a_directory_leak() {
        let root = dir();
        assert!(resolve(&root, "/no-such-file.js").is_none());
        assert!(resolve(&root, "/services").is_none(), "a directory is not a file");
    }

    #[test]
    fn content_types_follow_the_extension() {
        assert_eq!(content_type_for(Path::new("a/index.html")), "text/html; charset=utf-8");
        assert_eq!(content_type_for(Path::new("a/app.js")), "text/javascript; charset=utf-8");
        assert_eq!(content_type_for(Path::new("a/x.wasm")), "application/wasm");
        assert_eq!(content_type_for(Path::new("a/style.CSS")), "text/css; charset=utf-8");
        assert_eq!(
            content_type_for(Path::new("a/weird.bin")),
            "application/octet-stream"
        );
    }

    #[tokio::test]
    async fn static_responses_are_never_cached() {
        let root = dir();
        let response = serve(&root, "/index.html", &Method::GET).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store",
            "a cached ES module is the 'I changed it but nothing happened' bug"
        );
        assert_eq!(
            response.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
    }

    #[tokio::test]
    async fn head_reports_length_without_a_body() {
        let root = dir();
        let response = serve(&root, "/index.html", &Method::HEAD).await;
        assert_eq!(response.status(), StatusCode::OK);
        let len: usize = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .expect("HEAD must report its length")
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!(len > 0);
    }

    #[tokio::test]
    async fn writes_and_misses_are_refused() {
        let root = dir();
        assert_eq!(
            serve(&root, "/index.html", &Method::POST).await.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            serve(&root, "/nope.js", &Method::GET).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}
