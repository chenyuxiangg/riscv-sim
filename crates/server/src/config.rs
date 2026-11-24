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
    /// 镜像路径白名单的根（`--allow-image-root`）。`None` ⇒ 只接受上传的字节。
    pub allow_image_root: Option<PathBuf>,
    /// 是否允许绑定非回环地址。默认 `false`。
    pub allow_remote: bool,
    /// 远程模式令牌。回环模式下即使给了也只在请求校验时生效。
    pub token: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: IpAddr::from([127, 0, 0, 1]),
            port: DEFAULT_PORT,
            board: None,
            session_ttl: Duration::from_secs(DEFAULT_SESSION_TTL_SECS),
            max_console: DEFAULT_MAX_CONSOLE,
            allow_image_root: None,
            allow_remote: false,
            token: None,
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
                "--allow-image-root" => "--allow-image-root",
                "--allow-remote" => "--allow-remote",
                "--token" => "--token",
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
                        return Err("bad --max-console value 0: buffer must be positive".to_string());
                    }
                    cfg.max_console = bytes;
                }
                "--allow-image-root" => {
                    cfg.allow_image_root = Some(PathBuf::from(value(&mut it, flag)?));
                }
                "--allow-remote" => cfg.allow_remote = true,
                "--token" => {
                    let raw = value(&mut it, flag)?;
                    if raw.trim().is_empty() {
                        return Err("bad --token value: token must not be blank".to_string());
                    }
                    cfg.token = Some(raw.to_string());
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
        let Some(root) = self.allow_image_root.take() else {
            return Ok(());
        };
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
        assert_eq!(cfg.session_ttl, Duration::from_secs(DEFAULT_SESSION_TTL_SECS));
        assert_eq!(cfg.max_console, DEFAULT_MAX_CONSOLE);
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
            args(&["--port"]),                       // 缺值
            args(&["--port", "0"]),                  // 0 不是合法端口
            args(&["--port", "70000"]),              // 越界
            args(&["--port", "x"]),                  // 不是数
            args(&["--session-ttl", "0"]),           // TTL 为 0
            args(&["--max-console", "0"]),           // 缓冲为 0
            args(&["--token", "   "]),               // 空令牌
            args(&["--port", "1", "--port", "1"]),   // 重复旗标
            args(&["--nope"]),                       // 未知旗标
            args(&["osz.elf"]),                      // 位置参数
            args(&["--bind", "localhost"]),          // 主机名不是 IP 字面量
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
    fn prepare_canonicalizes_and_checks_the_whitelist_root() {
        let mut cfg = Config::default();
        cfg.allow_image_root = Some(PathBuf::from("/tmp"));
        cfg.prepare().unwrap();
        let root = cfg.allow_image_root.clone().unwrap();
        assert!(root.is_absolute(), "the whitelist root must be absolute");
        assert!(root.is_dir());

        let mut missing = Config::default();
        missing.allow_image_root = Some(PathBuf::from("/definitely/not/here"));
        assert!(missing.prepare().is_err());
    }
}
