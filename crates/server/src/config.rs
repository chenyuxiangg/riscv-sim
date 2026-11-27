//! 启动配置与安全闸门（M-24 · R-01 / R-15 · 表 12 的 `Config::parse` 与 `guard_bind`）。
//!
//! ⚠️ **`parse` 是纯函数**：只把 `argv` 变成结构，不碰文件系统、不 bind 端口
//! ⇒ ⇒ 每一条畸形输入都能用一条单测钉住，不需要起进程。
//! 路径白名单的「存在性 + 规范化」放在 [`Config::prepare`]，因为那一步真的要
//! 看磁盘；分开放是为了让单测不依赖测试机上的目录布局。

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

/// 默认监听端口（需求 R-01：`127.0.0.1:9876`）。
pub const DEFAULT_PORT: u16 = 9876;

/// 默认会话空闲回收时间（秒）。取 15 分钟：够一次人工调试，又不至于让
/// 忘关的会话把额度占满。
pub const DEFAULT_SESSION_TTL_SECS: u64 = 900;

/// 默认并发会话上限（配额）。取 8：一个人调 Agent 用不满，失控的重试风暴
/// 又会被它挡住。
pub const DEFAULT_MAX_SESSIONS: usize = 8;

/// 一次上传的镜像字节上限（默认 16 MiB，对齐 M-06 的解析限值）。
///
/// ⚠️ 超限**原样拒**（413 + `E_PROTO-08`），不静默截断 —— 截断一半的 ELF
/// 会被解析器当畸形，报出来的错跟真原因（太大）对不上。
pub const DEFAULT_MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// 一次 `restore` 的默认载荷上限（32 MiB）。
///
/// 快照含整页 RAM（六段），比镜像大得多；两个界分开设，避免为了快照把
/// 镜像的上限一起放大。
pub const DEFAULT_MAX_SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;

/// `run` 的墙钟上限默认值（毫秒）。R-11：不给上限就等于允许「一个请求把
/// 进程占死」。
pub const DEFAULT_MAX_RUN_MS: u64 = 5_000;

/// 默认 console 缓冲上限（字节）。与 `osz-boot` 的 `--max-console` 同量级
/// ⇒ ⇒ 两条入口在「溢出」这件事上不会给出不同的字节数。
pub const DEFAULT_MAX_CONSOLE: usize = 1 << 20;

/// 进程级配置。**一次解析，全局只读**（建会话时只读它，不改它）。
#[derive(Debug, Clone)]
pub struct Config {
    /// 监听地址。默认回环 —— R-15 的第一条。
    pub bind: IpAddr,
    /// 监听端口。
    pub port: u16,
    /// 启动默认板（D4 的裁决：只作默认，建会话时仍要显式执行一次配板）。
    pub board: Option<String>,
    /// 会话空闲回收时长。
    pub session_ttl: Duration,
    /// 每会话 console 缓冲上限。
    pub max_console: usize,
    /// 并发会话上限。
    pub max_sessions: usize,
    /// 镜像路径白名单的根（`--allow-image-root`）。`None` ⇒ 只接受上传的字节。
    pub allow_image_root: Option<PathBuf>,
    /// 单次上传的镜像字节上限（`--max-image-bytes`）。
    pub max_image_bytes: usize,
    /// 一次 `restore` 的载荷上限（`--max-snapshot-bytes`）。
    ///
    /// ⚠️ **不能**沿用 `--max-image-bytes`：快照带整页 RAM，比镜像大两三个
    /// 数量级 ⇒ ⇒ 两个界分开设。
    pub max_snapshot_bytes: usize,
    /// `run` 一次请求的墙钟上限（`--max-run-ms`）。
    pub max_run_ms: u64,
    /// 是否允许绑定非回环地址。默认 `false`。
    pub allow_remote: bool,
    /// 远程模式令牌。回环模式下即使给了也只在请求校验时生效。
    pub token: Option<String>,
    /// 显式放行的 **浏览器** origin（`--cors-origins`，逗号分隔）。
    ///
    /// ⚠️ 回环 origin（`http://127.0.0.1[:port]` / `http://localhost[:port]`）
    /// **默认就放行**（服务默认只绑回环 ⇒ 那一组 origin 只可能是本机页面）；
    /// 这个表只用来补「前端不在本机 / 换了个主机名」的场合。
    pub cors_origins: Vec<String>,
    /// 前端静态资源目录（`--front-dir`）。`None` ⇒ 不托管页面（只服务 `/api`）。
    ///
    /// ⚠️ 给了它，服务端就是**一个端口**：页面与 API 同源 ⇒ 不需要第二个进程，
    /// 也不需要 CORS 放行（同源请求根本不走跨域检查）。
    pub front_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: IpAddr::from([127, 0, 0, 1]),
            port: DEFAULT_PORT,
            board: None,
            session_ttl: Duration::from_secs(DEFAULT_SESSION_TTL_SECS),
            max_console: DEFAULT_MAX_CONSOLE,
            max_sessions: DEFAULT_MAX_SESSIONS,
            allow_image_root: None,
            max_image_bytes: DEFAULT_MAX_IMAGE_BYTES,
            max_snapshot_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
            max_run_ms: DEFAULT_MAX_RUN_MS,
            allow_remote: false,
            token: None,
            cors_origins: Vec::new(),
            front_dir: None,
        }
    }
}

impl Config {
    /// 解析 `argv`（**不含**程序名）。**绝不 panic**：每条畸形输入都变成一条
    /// 可读的错误消息 —— 由 `main` 折成退出码 1。
    ///
    /// 规则（与设计文档表 12 / 表 19 对齐）：
    /// - 每个旗标**只能出现一次**（重复 ⇒ 错误，防止「后者悄悄覆盖前者」）；
    /// - `--port 0` 是错误（0 会让客户端拿到一个不可预先告知的端口）；
    /// - `--session-ttl 0` / `--max-console 0` 是错误（0 等于功能没开）；
    /// - 不接受位置参数。
    pub fn parse(argv: &[String]) -> Result<Self, String> {
        let mut cfg = Self::default();
        let mut seen: Vec<&'static str> = Vec::new();

        let mut it = argv.iter();
        while let Some(arg) = it.next() {
            let flag: &'static str = match arg.as_str() {
                "--bind" => "--bind",
                "--port" => "--port",
                "--board" => "--board",
                "--session-ttl" => "--session-ttl",
                "--max-console" => "--max-console",
                "--max-sessions" => "--max-sessions",
                "--allow-image-root" => "--allow-image-root",
                "--max-image-bytes" => "--max-image-bytes",
                "--max-run-ms" => "--max-run-ms",
                "--allow-remote" => "--allow-remote",
                "--token" => "--token",
                "--cors-origins" => "--cors-origins",
                "--front-dir" => "--front-dir",
                other if other.starts_with('-') => {
                    return Err(format!("unknown option {other}"));
                }
                other => {
                    return Err(format!(
                        "unexpected positional argument {other:?} (all inputs are flags)"
                    ));
                }
            };
            if seen.contains(&flag) {
                return Err(format!("{flag} given more than once"));
            }
            seen.push(flag);

            match flag {
                "--bind" => {
                    let raw = value(&mut it, flag)?;
                    cfg.bind = raw
                        .parse::<IpAddr>()
                        .map_err(|_| format!("bad --bind value {raw:?}: not an IP literal"))?;
                }
                "--port" => {
                    let raw = value(&mut it, flag)?;
                    let port = raw
                        .parse::<u16>()
                        .map_err(|_| format!("bad --port value {raw:?}: not 1..=65535"))?;
                    if port == 0 {
                        return Err("bad --port value 0: a listen port cannot be 0".to_string());
                    }
                    cfg.port = port;
                }
                "--board" => cfg.board = Some(value(&mut it, flag)?.to_string()),
                "--session-ttl" => {
                    let raw = value(&mut it, flag)?;
                    let secs = raw
                        .parse::<u64>()
                        .map_err(|_| format!("bad --session-ttl value {raw:?}: not seconds"))?;
                    if secs == 0 {
                        return Err("bad --session-ttl value 0: TTL must be positive".to_string());
                    }
                    cfg.session_ttl = Duration::from_secs(secs);
                }
                "--max-console" => {
                    let raw = value(&mut it, flag)?;
                    let bytes = raw
                        .parse::<usize>()
                        .map_err(|_| format!("bad --max-console value {raw:?}: not bytes"))?;
                    if bytes == 0 {
                        return Err(
                            "bad --max-console value 0: buffer must be positive".to_string()
                        );
                    }
                    cfg.max_console = bytes;
                }
                "--max-sessions" => {
                    let raw = value(&mut it, flag)?;
                    let sessions = raw
                        .parse::<usize>()
                        .map_err(|_| format!("bad --max-sessions value {raw:?}: not a count"))?;
                    if sessions == 0 {
                        return Err(
                            "bad --max-sessions value 0: quota must be positive".to_string()
                        );
                    }
                    cfg.max_sessions = sessions;
                }
                "--allow-image-root" => {
                    cfg.allow_image_root = Some(PathBuf::from(value(&mut it, flag)?));
                }
                "--max-image-bytes" => {
                    let raw = value(&mut it, flag)?;
                    let bytes = raw
                        .parse::<usize>()
                        .map_err(|_| format!("bad --max-image-bytes value {raw:?}: not bytes"))?;
                    if bytes == 0 {
                        return Err(
                            "bad --max-image-bytes value 0: limit must be positive".to_string()
                        );
                    }
                    cfg.max_image_bytes = bytes;
                }
                "--max-snapshot-bytes" => {
                    let raw = value(&mut it, flag)?;
                    let bytes = raw.parse::<usize>().map_err(|_| {
                        format!("bad --max-snapshot-bytes value {raw:?}: not bytes")
                    })?;
                    if bytes == 0 {
                        return Err(
                            "bad --max-snapshot-bytes value 0: limit must be positive".to_string()
                        );
                    }
                    cfg.max_snapshot_bytes = bytes;
                }
                "--max-run-ms" => {
                    let raw = value(&mut it, flag)?;
                    let ms = raw
                        .parse::<u64>()
                        .map_err(|_| format!("bad --max-run-ms value {raw:?}: not milliseconds"))?;
                    if ms == 0 {
                        return Err("bad --max-run-ms value 0: limit must be positive".to_string());
                    }
                    cfg.max_run_ms = ms;
                }
                "--allow-remote" => cfg.allow_remote = true,
                "--token" => {
                    let raw = value(&mut it, flag)?;
                    if raw.trim().is_empty() {
                        return Err("bad --token value: token must not be blank".to_string());
                    }
                    cfg.token = Some(raw.to_string());
                }
                "--cors-origins" => {
                    let raw = value(&mut it, flag)?;
                    let mut list = Vec::new();
                    for item in raw.split(',') {
                        if item.trim().is_empty() {
                            continue;
                        }
                        list.push(normalize_origin(item).map_err(|msg| {
                            format!("bad --cors-origins entry in {raw:?}: {msg}")
                        })?);
                    }
                    if list.is_empty() {
                        return Err("bad --cors-origins value: nothing but separators".to_string());
                    }
                    cfg.cors_origins = list;
                }
                "--front-dir" => {
                    cfg.front_dir = Some(PathBuf::from(value(&mut it, flag)?));
                }
                // 上面已经穷举过；这一支只为让 match 完整（不可达）。
                _ => return Err(format!("unknown option {flag}")),
            }
        }

        Ok(cfg)
    }

    /// 把 `--allow-image-root` 变成**规范化的绝对路径**，并确认它真的存在。
    ///
    /// ⚠️ 规范化**必须**发生在启动期：请求期再 `canonicalize` 就等于「先信任
    /// 客户端给的路径再校验」，`..` 穿越会在规范化之前就被当成合法前缀。
    pub fn prepare(&mut self) -> Result<(), String> {
        if let Some(root) = self.allow_image_root.take() {
            let canonical = root
                .canonicalize()
                .map_err(|e| format!("bad --allow-image-root {}: {e}", root.display()))?;
            if !canonical.is_dir() {
                return Err(format!(
                    "bad --allow-image-root {}: not a directory",
                    canonical.display()
                ));
            }
            self.allow_image_root = Some(canonical);
        }

        // ⚠️ `--front-dir` 与 `--allow-image-root` **同样**必须在启动期规范化：
        // 请求期再 `canonicalize` 等于「先信任客户端给的路径再校验」。
        // 且这里就要求 `index.html` 存在 —— 打错目录要在启动时失败，而不是
        // 起好了、浏览器打开却是 404。
        if let Some(dir) = self.front_dir.take() {
            let canonical = dir
                .canonicalize()
                .map_err(|e| format!("bad --front-dir {}: {e}", dir.display()))?;
            if !canonical.is_dir() {
                return Err(format!(
                    "bad --front-dir {}: not a directory",
                    canonical.display()
                ));
            }
            if !canonical.join("index.html").is_file() {
                return Err(format!(
                    "bad --front-dir {}: no index.html inside (point it at the front/ directory)",
                    canonical.display()
                ));
            }
            self.front_dir = Some(canonical);
        }

        Ok(())
    }
}

/// 安全闸门：非回环绑定必须**同时**有 `--allow-remote` 与 `--token`（R-15）。
///
/// ⚠️ 两条**缺一不可**：只给开关不给令牌，等于把内核的执行能力暴露给同网段；
/// 只给令牌不给开关，说明操作者并不知道自己在对外监听。
pub fn guard_bind(cfg: &Config) -> Result<(), String> {
    if cfg.bind.is_loopback() {
        return Ok(());
    }
    if !cfg.allow_remote {
        return Err(format!(
            "refusing to bind {} without --allow-remote (default is loopback only)",
            cfg.bind
        ));
    }
    match cfg.token.as_deref() {
        Some(token) if !token.trim().is_empty() => Ok(()),
        _ => Err(format!(
            "refusing to bind {} in remote mode: --token is required as well",
            cfg.bind
        )),
    }
}

/// 校验并规范化一个 CORS origin（`<scheme>://<host>[:<port>]`）。
///
/// ⚠️ **不接受 `*`**：通配 origin 等于把内核的执行能力交给任何网页 ⇒ 要放开
/// 就逐条列出来（R-15 的同一思路：宽的东西必须是人**显式**要的）。
/// 规范化 = 小写 scheme/host、去掉末尾 `/` ⇒ ⇒ 与浏览器发来的 `Origin` 头
/// 逐字可比，不必在请求期再做模糊匹配。
fn normalize_origin(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("origin is blank".to_string());
    }
    if trimmed == "*" {
        return Err("wildcard `*` is not supported — list the exact origins".to_string());
    }
    let (scheme, rest) = trimmed
        .split_once("://")
        .ok_or_else(|| format!("origin {raw:?} has no scheme (http:// or https://)"))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("origin {raw:?}: scheme must be http or https"));
    }
    if rest.is_empty()
        || rest.contains('/')
        || rest.contains('?')
        || rest.contains('#')
        || rest.contains('@')
    {
        return Err(format!(
            "origin {raw:?}: only <scheme>://<host>[:<port>] is allowed (no path, query, user or fragment)"
        ));
    }
    let (host, port) = split_host_port(rest)
        .ok_or_else(|| format!("origin {raw:?}: host is blank or the port is not a u16"))?;
    if port == Some(0) {
        return Err(format!("origin {raw:?}: port 0 is not a real origin"));
    }
    let host = if host.starts_with('[') {
        // IPv6 字面量（`[::1]`）：方括号是 URL 语法的一部分，原样保留。
        host.to_ascii_lowercase()
    } else {
        host.to_ascii_lowercase()
    };
    Ok(match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

/// 把 `host[:port]`（含 `[::1]:port`）拆成 `(host, Option<port>)`。
fn split_host_port(rest: &str) -> Option<(&str, Option<u16>)> {
    if let Some(tail) = rest.strip_prefix('[') {
        let (host, after) = tail.split_once(']')?;
        if host.is_empty() {
            return None;
        }
        let bracket_host = &rest[..rest.len() - after.len()];
        return match after.strip_prefix(':') {
            None if after.is_empty() => Some((bracket_host, None)),
            Some(port) => Some((bracket_host, Some(port.parse::<u16>().ok()?))),
            None => None,
        };
    }
    match rest.split_once(':') {
        None => Some((rest, None)),
        Some((host, port)) => {
            if host.is_empty() {
                return None;
            }
            Some((host, Some(port.parse::<u16>().ok()?)))
        }
    }
}

fn value<'a>(it: &mut std::slice::Iter<'a, String>, flag: &str) -> Result<&'a str, String> {
    it.next()
        .map(|s| s.as_str())
        .ok_or_else(|| format!("{flag} needs a value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_are_loopback_and_documented() {
        let cfg = Config::parse(&[]).unwrap();
        assert_eq!(cfg.bind, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(
            cfg.session_ttl,
            Duration::from_secs(DEFAULT_SESSION_TTL_SECS)
        );
        assert_eq!(cfg.max_console, DEFAULT_MAX_CONSOLE);
        assert_eq!(cfg.max_sessions, DEFAULT_MAX_SESSIONS);
        assert!(cfg.board.is_none());
        assert!(cfg.allow_image_root.is_none());
        assert!(!cfg.allow_remote);
    }

    #[test]
    fn parse_accepts_every_documented_flag() {
        let cfg = Config::parse(&args(&[
            "--bind",
            "0.0.0.0",
            "--port",
            "7000",
            "--board",
            "qemu-virt",
            "--session-ttl",
            "60",
            "--max-console",
            "4096",
            "--max-sessions",
            "3",
            "--allow-image-root",
            "/tmp",
            "--allow-remote",
            "--token",
            "s3cret",
        ]))
        .unwrap();
        assert_eq!(cfg.bind, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(cfg.port, 7000);
        assert_eq!(cfg.board.as_deref(), Some("qemu-virt"));
        assert_eq!(cfg.session_ttl, Duration::from_secs(60));
        assert_eq!(cfg.max_console, 4096);
        assert_eq!(cfg.max_sessions, 3);
        assert_eq!(
            cfg.allow_image_root.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
        assert!(cfg.allow_remote);
        assert_eq!(cfg.token.as_deref(), Some("s3cret"));
    }

    #[test]
    fn parse_rejects_bad_input_without_panicking() {
        for bad in [
            args(&["--port"]),                     // 缺值
            args(&["--port", "0"]),                // 0 不是合法端口
            args(&["--port", "70000"]),            // 越界
            args(&["--port", "x"]),                // 不是数
            args(&["--session-ttl", "0"]),         // TTL 为 0
            args(&["--max-console", "0"]),         // 缓冲为 0
            args(&["--max-sessions", "0"]),        // 配额为 0
            args(&["--token", "   "]),             // 空令牌
            args(&["--port", "1", "--port", "1"]), // 重复旗标
            args(&["--nope"]),                     // 未知旗标
            args(&["osz.elf"]),                    // 位置参数
            args(&["--bind", "localhost"]),        // 主机名不是 IP 字面量
        ] {
            assert!(
                Config::parse(&bad).is_err(),
                "argv {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn cors_origins_are_normalized_and_wildcards_are_refused() {
        let cfg = Config::parse(&args(&[
            "--cors-origins",
            " HTTP://LocalHost:8000/ , https://sim.example.com ",
        ]))
        .unwrap();
        assert_eq!(
            cfg.cors_origins,
            vec![
                "http://localhost:8000".to_string(),
                "https://sim.example.com".to_string()
            ]
        );

        for bad in [
            args(&["--cors-origins", "*"]),                 // 通配
            args(&["--cors-origins", "http://a.com/path"]), // 带路径
            args(&["--cors-origins", "a.com"]),             // 没 scheme
            args(&["--cors-origins", "ftp://a.com"]),       // 非 http(s)
            args(&["--cors-origins", "http://a.com:0"]),    // 端口 0
            args(&["--cors-origins", " , "]),               // 只有分隔符
        ] {
            assert!(
                Config::parse(&bad).is_err(),
                "argv {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn guard_bind_allows_loopback_out_of_the_box() {
        let cfg = Config::default();
        assert!(guard_bind(&cfg).is_ok());
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // 逐字段推进是本用例的写法
    fn guard_bind_needs_switch_and_token_both() {
        let mut cfg = Config::default();
        cfg.bind = IpAddr::from([0, 0, 0, 0]);
        assert!(guard_bind(&cfg).is_err(), "no switch, no token ⇒ refuse");

        cfg.allow_remote = true;
        assert!(
            guard_bind(&cfg).is_err(),
            "switch without token ⇒ still refuse"
        );

        cfg.token = Some("t".to_string());
        assert!(guard_bind(&cfg).is_ok(), "switch + token ⇒ allowed");
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // 逐字段推进是本用例的写法
    fn prepare_canonicalizes_and_checks_the_whitelist_root() {
        let mut cfg = Config::default();
        cfg.allow_image_root = Some(PathBuf::from("/tmp"));
        cfg.prepare().unwrap();
        let root = cfg.allow_image_root.clone().unwrap();
        assert!(root.is_absolute(), "the whitelist root must be absolute");
        assert!(root.is_dir());

        #[allow(clippy::field_reassign_with_default)] // 同上
        let mut missing = Config::default();
        missing.allow_image_root = Some(PathBuf::from("/definitely/not/here"));
        assert!(missing.prepare().is_err());
    }

    #[test]
    fn front_dir_is_parsed_and_is_off_by_default() {
        let cfg = Config::parse(&[]).unwrap();
        assert!(
            cfg.front_dir.is_none(),
            "no --front-dir ⇒ the server stays API-only (loopback run is unchanged)"
        );

        let cfg = Config::parse(&args(&["--front-dir", "front"])).unwrap();
        assert_eq!(cfg.front_dir, Some(PathBuf::from("front")));
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // 逐字段推进是本用例的写法
    fn prepare_requires_an_index_html_inside_front_dir() {
        // 打错目录要在**启动时**失败，而不是起好了、浏览器一开却是 404。
        let mut cfg = Config::default();
        cfg.front_dir = Some(PathBuf::from("/tmp"));
        assert!(cfg.prepare().is_err(), "/tmp has no index.html");

        let mut missing = Config::default();
        missing.front_dir = Some(PathBuf::from("/definitely/not/here"));
        assert!(missing.prepare().is_err());

        let mut real = Config::default();
        real.front_dir = Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../front")
                .to_path_buf(),
        );
        real.prepare().unwrap();
        let kept = real.front_dir.clone().expect("kept");
        assert!(kept.is_absolute(), "canonicalized ⇒ absolute");
        assert!(kept.join("index.html").is_file());
    }
}
