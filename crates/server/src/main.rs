//! `riscv_sim_server` —— Agent / CI 的进程级操作入口（M-24 · R-01）。
//!
//! ## 它是什么
//!
//! 内核（`riscv_sim_kernel`）的**宿主壳**：把已有命令以 HTTP 暴露出去，客户端
//! 不再接触 wasm 产物与 `front/` 目录。默认只绑回环 `127.0.0.1:9876`。
//!
//! ## P0-1 的边界（本文件当前范围）
//!
//! 只提供进程入口、参数闸门、`GET /api/version`、`GET /healthz` 与标准错误体。
//! 会话表、配板、装载、run、console、WS 都在后续切片；**现在打任何别的路径
//! 都会拿到 `404 + E_PROTO-08`**，而不是连不上 —— 这样客户端能区分「服务没
//! 起来」与「路径打错」。
//!
//! ## 退出码
//!
//! | 码 | 含义 |
//! | --- | --- |
//! | 0 | 正常退出 |
//! | 1 | 用法 / 参数错误（含 `--allow-image-root` 不存在） |
//! | 2 | 闸门拒绝（非回环却没有开关或令牌）或绑定失败 |
//!
//! ⚠️ **退出码 1 与 2 分开**：前者是「你打错了」，后者是「环境/策略不让绑」，
//! 脚本据此决定是修参数还是换环境。

mod api;
mod config;
mod error;

use std::net::SocketAddr;
use std::process::ExitCode;

use crate::api::AppState;
use crate::config::Config;

/// 帮助文本。**与 `--help` 同源**，不另写一份。
const USAGE: &str = "\
usage: riscv_sim_server [options]

  --bind <ip>              listen address (default 127.0.0.1)
  --port <n>               listen port (default 9876, 1..=65535)
  --board <id>             startup default board template
  --session-ttl <secs>     idle session reaping window (default 900)
  --max-console <bytes>    per-session console buffer cap (default 1048576)
  --allow-image-root <dir> whitelist root for image paths (default: uploads only)
  --allow-remote           allow a non-loopback bind (needs --token too)
  --token <secret>         bearer token checked in remote mode
  -h, --help               print this message

  GET /api/version   kernel / schema_version / contract_version / server / bind
  GET /healthz       liveness + uptime + effective limits
";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let mut cfg = match Config::parse(&argv) {
        Ok(cfg) => cfg,
        Err(msg) => {
            eprintln!("riscv_sim_server: {msg}");
            eprintln!("  run with --help for the full option list");
            return ExitCode::from(1);
        }
    };

    // 白名单根的规范化放这里：`parse` 是纯函数，碰磁盘的只有这一步。
    if let Err(msg) = cfg.prepare() {
        eprintln!("riscv_sim_server: {msg}");
        return ExitCode::from(1);
    }

    // 安全闸门：非回环必须同时有开关与令牌（R-15）。
    if let Err(msg) = config::guard_bind(&cfg) {
        eprintln!("riscv_sim_server: {msg}");
        return ExitCode::from(2);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("riscv_sim_server: cannot start the async runtime: {err}");
            return ExitCode::from(2);
        }
    };

    match runtime.block_on(serve(cfg)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("riscv_sim_server: {msg}");
            ExitCode::from(2)
        }
    }
}

/// 绑定、打印**实际**地址、开始服务。
///
/// ⚠️ 打印的是 `local_addr()`，不是 `SocketAddr::new(bind, port)`：后者是
/// **请求**的地址 ⇒ ⇒ 一旦两者不同（端口被占、被改写），日志就会撒谎，
/// 而客户端照着日志连会连到别人。
async fn serve(cfg: Config) -> Result<(), String> {
    let requested = SocketAddr::new(cfg.bind, cfg.port);
    let listener = tokio::net::TcpListener::bind(requested)
        .await
        .map_err(|e| format!("cannot bind {requested}: {e}"))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("cannot read the bound address: {e}"))?;

    let state = AppState::new(&cfg, local);
    eprintln!(
        "riscv_sim_server {} listening on http://{local} (board={}, session_ttl={}s, max_console={}B)",
        env!("CARGO_PKG_VERSION"),
        cfg.board.as_deref().unwrap_or("-"),
        cfg.session_ttl.as_secs(),
        cfg.max_console,
    );

    axum::serve(listener, api::router(state))
        .await
        .map_err(|e| format!("serve {local}: {e}"))
}
