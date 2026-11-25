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
//! 会话表、配板、装载、run、console、事件流（WS）、trace 都已落地；**打任何别的路径
//! 都会拿到 `404 + E_PROTO-08`**，而不是连不上 —— 这样客户端能区分「服务没
//! 起来」与「路径打错」。
//!
//! ⚠️ 例外：给了 `--front-dir <dir>` 时，同一端口的非 `/api` 路径交给静态资源
//! （`front/` 页面 + API 同源 ⇒ 一个地址打开就能用，见 `front.rs`）。
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
mod front;
mod session;
mod upload;

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

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
  --max-sessions <n>       concurrent session quota (default 8)
  --allow-image-root <dir> whitelist root for image paths (default: uploads only)
  --max-image-bytes <n>    per-upload image byte cap (default 16777216)
  --max-snapshot-bytes <n> per-restore snapshot body cap (default 33554432)
  --max-run-ms <ms>        wall-clock cap for one run (default 5000)
  --allow-remote           allow a non-loopback bind (needs --token too)
  --token <secret>         bearer token checked in remote mode
  --cors-origins <list>    extra browser origins allowed to read the API
                           (comma separated; loopback origins are always allowed)
  --front-dir <dir>        serve a front/ directory on this port as well
                           (same-origin page + API: no second process, no CORS)
  -h, --help               print this message

  GET    /api/version          kernel / schema_version / contract_version / server / bind
  GET    /healthz              liveness + uptime + effective limits
  POST   /api/sessions         {board?} -> 201 {session_id, board, board_source, ...}
  GET    /api/sessions         list + quota
  GET    /api/sessions/<id>    detail + a live probe (pc / executed)
  DELETE /api/sessions/<id>    cancel, join, unregister; a second delete is 404
  PUT    /api/sessions/<id>/board   {template|description, expected_revision?} -> current map
  POST   /api/sessions/<id>/image   octet-stream (+X-Image-Name) or {path} -> load receipt
  POST   /api/boot                  {path, prefix?, steps?, timeout_ms?, write_to?} -> console bytes + X-Boot-*
  GET    /api/board/templates       builtin template metadata
  GET    /api/board/templates/<id>  full PlatformDescription (404 E_PLT-22 if unknown)
  POST   /api/sessions/<id>/run     {steps|timeout_ms, async?}? -> stop_reason / executed / pc / console
                                     async:true -> 202 {job_id, poll}
  GET    /api/sessions/<id>/jobs/<job> -> {state: running|done|failed, report|code}
  POST   /api/sessions/<id>/step    {n} -> exactly n instructions
  GET    /api/sessions/<id>/console ?offset&limit -> raw bytes + X-Console-Base/Total
  GET    /api/sessions/<id>/state   -> pc / registers[32] / executed
  GET    /api/sessions/<id>/memory  ?addr&len -> {addr, len, bytes}
  GET    /api/sessions/<id>/symbols ?limit -> {symbols, returned}
  GET    /api/sessions/<id>/disasm  ?addr&count -> {entries} (may be short)
  GET    /api/sessions/<id>/breakpoints -> {breakpoints, pc}
  POST   /api/sessions/<id>/breakpoints {addr|symbol|condition} -> 201 + table
  DELETE /api/sessions/<id>/breakpoints/<bp> -> table (idempotent)
  PUT    /api/sessions/<id>/breakpoints/<bp>/enabled {enabled} -> table
  GET    /api/sessions/<id>/events  ?since&limit -> {frames[{seq,event}], next, lost}
  GET    /api/sessions/<id>/events/ws  WebSocket: {since, next, lost, frames} 一帧一拍
  GET    /api/sessions/<id>/trace   ?source(execution|waveform)&format(csv|json|vcd)&offset&limit
  PUT    /api/sessions/<id>/trace/enabled {enabled} -> {enabled, len, dropped}
  POST   /api/sessions/<id>/snapshot  {id?, base_seq?} -> {snapshot, bytes} 六段齐全
  POST   /api/sessions/<id>/restore   SessionSnapshotDto -> {restored, pc, executed}
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

/// 等一个终止信号，回一句**人话**说明是哪一个。
///
/// ⚠️ **SIGTERM 必须处理**：`kill <pid>`（systemd、docker stop、CI 的清理
/// 步骤）发的都是它，而 `tokio::signal::ctrl_c()` **只**认 SIGINT ⇒ ⇒
/// 只等 ctrl-c 的话，脚本发 SIGTERM 之后进程会一直挂在端口上（实测过：
/// 端口占着，要用 `kill -9` 才退）。
///
/// ⚠️ **非 Unix 平台**没有 SIGTERM 的概念 ⇒ 只等 ctrl-c（编译期分支，
/// 不是运行期猜）。
async fn shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        let mut term = match tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        ) {
            Ok(signal) => signal,
            Err(err) => {
                // 装不上 SIGTERM 处理器不是致命错误：退化成只认 ctrl-c，
                // 但要**说出来**（否则用户以为 SIGTERM 生效了）。
                eprintln!(
                    "riscv_sim_server: cannot hook SIGTERM ({err}); only ctrl-c stops the server"
                );
                let _ = tokio::signal::ctrl_c().await;
                return "ctrl-c";
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "ctrl-c",
            _ = term.recv() => "SIGTERM",
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "ctrl-c"
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
        "riscv_sim_server {} listening on http://{local} (board={}, session_ttl={}s, max_console={}B, front={})",
        env!("CARGO_PKG_VERSION"),
        cfg.board.as_deref().unwrap_or("-"),
        cfg.session_ttl.as_secs(),
        cfg.max_console,
        cfg.front_dir
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".to_string()),
    );

    // TTL 回收器：每 min(ttl/2, 30 s) 扫一次。放在单独任务里 ⇒ ⇒ 回收不会
    // 卡在任何一次请求后面。
    let registry = Arc::clone(&state.registry);
    let tick = std::cmp::min(cfg.session_ttl / 2, Duration::from_secs(30));
    let tick = if tick.is_zero() {
        Duration::from_secs(1)
    } else {
        tick
    };
    let sweeper = tokio::spawn(async move {
        let mut timer = tokio::time::interval(tick);
        loop {
            timer.tick().await;
            let reaped = registry.sweep();
            if reaped > 0 {
                eprintln!("riscv_sim_server: TTL reaped {reaped} idle session(s)");
            }
        }
    });

    let serve_result = tokio::select! {
        result = axum::serve(listener, api::router(state.clone())) => {
            result.map_err(|e| format!("serve {local}: {e}"))
        }
        signal = shutdown_signal() => {
            eprintln!("riscv_sim_server: {signal}, shutting sessions down");
            Ok(())
        }
    };

    // 退出前把会话线程收干净 ⇒ ⇒ 不留孤儿线程（R-11）。
    sweeper.abort();
    state.registry.shutdown();
    serve_result
}
