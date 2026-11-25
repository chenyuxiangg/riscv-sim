//! `osz-boot` —— **`POST /api/boot` 的命令行客户端**：「跑一个真实 M 模式
//! RTOS、抓它的 console 字节流」这件事，**只有服务端一份实现**（R-12）。
//!
//! ## 为什么改成客户端（而不是自己跑内核）
//!
//! 老版本在本进程里 `KernelHost::new()` + **按镜像地址自动选档**再跑 ——
//! 那就是**第二份**「选哪块板 / 跑到哪算停」的判定。R-12 的要求是
//! 「仓内只保留一份判定口径」⇒ ⇒ 本 runner 现在只做三件事：
//!
//! 1. 把参数拼成 `POST /api/boot` 的请求体；
//! 2. 把回体（**原始 console 字节**）原样吐到 stdout / `--out`；
//! 3. 把回执头（`x-boot-stop` / `x-boot-ram-window` / …）翻译成判决行与
//!    退出码。
//!
//! ⚠️ **停因判定、板的选择、预算与墙钟**全部在服务端（`RunSpec` +
//! `run_until`）⇒ ⇒ 本文件里**没有** `run_until`、**没有** `builtin_descriptions`
//! 的挑板循环 ⇒ ⇒ 两边不可能各判一次（也就不会漂）。
//!
//! ## 流约定（与老版本一致，脚本不用改）
//!
//! - **stdout** = console 的**原始字节**（不是文本、不是转义过的）⇒ 可以
//!   直接 `cmp` 与 Spike 的 stdout 比。
//! - **stderr** = 判决行（`VERDICT ...`）⇒ 不污染字节流。
//!
//! ## 用法
//!
//! ```text
//! osz-boot <elf> [--server <url>] [--board <id>] [--out <path>]
//!           [--prefix <text>] [--budget <n>] [--timeout-ms <n>]
//!           [--print-memory] [-v]
//! ```
//!
//! - `--server <url>`：服务端地址（默认 `http://127.0.0.1:9876`）；也接受
//!   `127.0.0.1:9876` 这种裸 `host:port`。
//! - `--board <id>`：板模板 id（默认 `qemu-virt` —— osz 的 UART0 在
//!   `0x1000_0000`、RAM 在 `0x8000_0000`，`default` 板上没有 UART）。
//! - `--prefix <text>`：console 里出现这个前缀就停（字节级子串）。
//! - `--budget <n>`：指令预算（默认 100 000 000 —— 服务端的 `BOOT_MAX_STEPS`）。
//! - `--timeout-ms <n>`：**墙钟**超时（默认 5 000 ms，与服务端缺省的
//!   `--max-run-ms` 对齐；服务端把 `--max-run-ms` 当硬上限，超了会 400）。
//! - `--print-memory`：**查询**——跑一个最小请求把板子的 RAM 窗口
//!   （`base:size`，十进制，取自回执头 `x-boot-ram-window`）打到 stdout 然后
//!   退出 ⇒ ⇒ 参考侧的内存图**不再**由本 runner 自己挑段算出来。
//!
//! ⚠️ **`--max-console` 已删除**：console 环的上限是**服务端**的启动参数
//!   （`--max-console`），客户端再设一个就是第二个真相。
//!
//! ## 退出码
//!
//! | 码 | 含义 |
//! | --- | --- |
//! | 0 | 跑完了（`prefix` / `budget` / 正常结束），字节流已写出 |
//! | 1 | 用法错误 / 写文件失败 |
//! | 2 | 超时（服务端 `x-boot-stop: timeout`） |
//! | 3 | 服务端拒绝 / 内核报错（trap / watchdog / 其它停点）/ 连不上服务 |
//!
//! ⚠️ **跑完不等于成功**：字节流与参考侧是否**逐字节相同**是**比对脚本**
//! 的事（`tools/boot-osz.sh`）—— 本 runner 只负责「诚实地把字节流吐出来」。

use std::process::ExitCode;

/// 服务端默认地址（与 `riscv_sim_server` 的 `--bind` / `--port` 默认值一致）。
const DEFAULT_SERVER: &str = "http://127.0.0.1:9876";

/// 板模板默认值 —— osz 要 UART0（`0x1000_0000`）与 `0x8000_0000` 的 RAM。
const DEFAULT_BOARD: &str = "qemu-virt";

/// 指令预算默认值 —— 服务端的 `BOOT_MAX_STEPS`（再多会被 400 拒）。
const DEFAULT_BUDGET: u32 = 100_000_000;

/// 墙钟超时默认值（毫秒）—— 与服务端缺省的 `--max-run-ms`（5 000）一致；
/// 要跑更久就把两边**一起**抬（服务端抬上限、这里抬 `--timeout-ms`）。
const DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// 运行配置。
struct Config {
    path: String,
    server: String,
    board: String,
    out: Option<String>,
    prefix: Option<String>,
    budget: u32,
    timeout_ms: u64,
    verbose: bool,
    /// 只查询板子的 RAM 窗口，不跑 guest。
    print_memory: bool,
}

/// 解析命令行。**绝不 panic** —— 每个畸形输入都变成一条错误消息。
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut path: Option<String> = None;
    let mut server = DEFAULT_SERVER.to_string();
    let mut board = DEFAULT_BOARD.to_string();
    let mut out: Option<String> = None;
    let mut prefix: Option<String> = None;
    let mut budget = DEFAULT_BUDGET;
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    let mut verbose = false;
    let mut print_memory = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--server" => {
                let raw = it.next().ok_or("--server needs a value")?;
                server = normalize_server(raw)?;
            }
            "--board" => {
                let raw = it.next().ok_or("--board needs a value")?;
                if raw.trim().is_empty() {
                    return Err("--board needs a non-empty template id".to_string());
                }
                board = raw.trim().to_string();
            }
            "--out" => {
                let raw = it.next().ok_or("--out needs a value")?;
                out = Some(raw.to_string());
            }
            "--prefix" => {
                let raw = it.next().ok_or("--prefix needs a value")?;
                prefix = Some(raw.to_string());
            }
            "--budget" => {
                let raw = it.next().ok_or("--budget needs a value")?;
                budget = raw
                    .parse::<u32>()
                    .map_err(|_| format!("bad --budget value: {raw}"))?;
                if budget == 0 {
                    return Err("--budget must be >= 1".to_string());
                }
            }
            "--timeout-ms" => {
                let raw = it.next().ok_or("--timeout-ms needs a value")?;
                timeout_ms = raw
                    .parse::<u64>()
                    .map_err(|_| format!("bad --timeout-ms value: {raw}"))?;
                if timeout_ms == 0 {
                    return Err("--timeout-ms must be >= 1".to_string());
                }
            }
            "--print-memory" => print_memory = true,
            "-v" | "--verbose" => verbose = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other}"));
            }
            other => {
                if path.is_some() {
                    return Err(format!("unexpected extra argument {other}"));
                }
                path = Some(other.to_string());
            }
        }
    }

    let path = path.ok_or("missing <elf> argument")?;
    Ok(Config {
        path,
        server,
        board,
        out,
        prefix,
        budget,
        timeout_ms,
        verbose,
        print_memory,
    })
}

/// 把用户给的地址规范成 `http://host:port`（去掉尾斜杠；裸 `host:port` 补 scheme）。
fn normalize_server(raw: &str) -> Result<String, String> {
    let text = raw.trim().trim_end_matches('/');
    if text.is_empty() {
        return Err("--server needs a non-empty URL".to_string());
    }
    let with_scheme = if text.starts_with("http://") || text.starts_with("https://") {
        text.to_string()
    } else if text.contains("://") {
        return Err(format!(
            "--server must be an http:// or https:// URL: {raw}"
        ));
    } else {
        format!("http://{text}")
    };
    Ok(with_scheme)
}

/// 判决（与老版本的 `VERDICT ...` 行保持一致，脚本照旧可 grep）。
enum Verdict {
    Done { reason: String },
    Error(String),
    Timeout,
    KernelError(String),
}

impl Verdict {
    fn line(&self) -> String {
        match self {
            Self::Done { reason } => format!("VERDICT DONE {reason}"),
            Self::Error(msg) => format!("VERDICT ERROR {msg}"),
            Self::Timeout => "VERDICT TIMEOUT".to_string(),
            Self::KernelError(msg) => format!("VERDICT KERNEL-ERROR {msg}"),
        }
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::Done { .. } => 0,
            Self::Error(_) => 1,
            Self::Timeout => 2,
            Self::KernelError(_) => 3,
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "usage: osz-boot <elf> [--server <url>] [--board <id>] [--out <path>]\n\
             \x20             [--prefix <text>] [--budget <n>] [--timeout-ms <n>]\n\
             \x20             [--print-memory] [-v]\n\
             \n\
             A client of `POST /api/boot`: the server decides the board, the run window\n\
             and the stop reason; this runner only prints the console bytes it got back.\n\
             Console bytes go to **stdout**, the `VERDICT ...` line to **stderr**."
        );
        return ExitCode::SUCCESS;
    }

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("osz-boot: {msg}");
            return ExitCode::from(1);
        }
    };

    let verdict = if cfg.print_memory {
        print_memory(&cfg)
    } else {
        run(&cfg)
    };
    eprintln!("{}", verdict.line());
    ExitCode::from(verdict.exit_code())
}

/// 一次 `/api/boot` 的回执：状态码 + 头 + 回体（原始 console 字节）。
struct BootReply {
    http_code: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl BootReply {
    /// 头名大小写不敏感（HTTP 头本来就不区分大小写）。
    fn header(&self, name: &str) -> Option<&str> {
        let want = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| key.to_ascii_lowercase() == want)
            .map(|(_, value)| value.as_str())
    }

    fn stop(&self) -> Option<&str> {
        self.header("x-boot-stop")
    }
}

/// 把参数拼成 `POST /api/boot` 的请求体（`steps` / `timeout_ms` 都显式给）。
fn request_body(cfg: &Config, steps: u32, with_prefix: bool) -> String {
    let mut body = format!(
        "{{\"path\":\"{}\",\"board\":\"{}\",\"steps\":{},\"timeout_ms\":{}",
        json_escape(&cfg.path),
        json_escape(&cfg.board),
        steps,
        cfg.timeout_ms
    );
    if with_prefix {
        if let Some(prefix) = &cfg.prefix {
            body.push_str(&format!(",\"prefix\":\"{}\"", json_escape(prefix)));
        }
    }
    body.push('}');
    body
}

/// 最小 JSON 字符串转义（只处理 `"` `\` 与不可见控制字符）。
fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 调一次 `POST <server>/api/boot`。
///
/// ⚠️ 用 `curl`（`tools/boot-osz.sh` 本来就依赖它）⇒ ⇒ 本仓**不**再长出
///    第二个手写 HTTP 客户端（TLS / 分块 / 重定向这些坑只在一个地方）。
fn post_boot(cfg: &Config, body: &str) -> Result<BootReply, String> {
    let dir = scratch_dir()?;
    let body_path = dir.join("request.json");
    let reply_path = dir.join("reply.bin");
    let headers_path = dir.join("reply.headers");
    std::fs::write(&body_path, body)
        .map_err(|err| format!("cannot write {}: {err}", body_path.display()))?;

    // curl 的墙钟给「服务端自己的 timeout」再留 10 秒余量。
    let curl_timeout = cfg.timeout_ms.saturating_div(1000) + 10;
    let url = format!("{}/api/boot", cfg.server);
    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            &curl_timeout.to_string(),
            "-o",
            &reply_path.display().to_string(),
            "-D",
            &headers_path.display().to_string(),
            "-w",
            "%{http_code}",
            "-X",
            "POST",
            &url,
            "-H",
            "content-type: application/json",
            "--data-binary",
            &format!("@{}", body_path.display()),
        ])
        .output()
        .map_err(|err| format!("cannot run curl: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("POST {url} failed: {}", stderr.trim()));
    }
    let http_code = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()
        .map_err(|_| format!("cannot read the HTTP status of POST {url}"))?;

    let raw_headers = std::fs::read_to_string(&headers_path).unwrap_or_default();
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in raw_headers.lines() {
        let line = line.trim_end_matches('\r');
        // 每块响应都以 `HTTP/...` 开头：**只留最后一块**（重定向 / 100-continue
        // 会把前面的头也带出来）。
        if line.starts_with("HTTP/") {
            headers.clear();
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }

    let body_bytes = std::fs::read(&reply_path).unwrap_or_default();
    let _ = std::fs::remove_file(&body_path);
    let _ = std::fs::remove_file(&reply_path);
    let _ = std::fs::remove_file(&headers_path);
    let _ = std::fs::remove_dir(&dir);
    Ok(BootReply {
        http_code,
        headers,
        body: body_bytes,
    })
}

/// 一次性的暂存目录（`temp_dir/osz-boot-<pid>-<nanos>`）。
fn scratch_dir() -> Result<std::path::PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("osz-boot-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    Ok(dir)
}

/// 把回体（原始 console 字节）写到 stdout 与 `--out`。
fn emit_bytes(cfg: &Config, bytes: &[u8]) -> Result<(), Verdict> {
    use std::io::Write;

    let mut out = std::io::stdout();
    if out.write_all(bytes).is_err() {
        let _ = out.flush();
        return Err(Verdict::Error(
            "cannot write console bytes to stdout".to_string(),
        ));
    }
    let _ = out.flush();
    if let Some(path) = &cfg.out {
        if let Err(err) = std::fs::write(path, bytes) {
            return Err(Verdict::Error(format!("write:{path}:{err}")));
        }
    }
    Ok(())
}

/// `--print-memory`：跑一次**最小**请求（`steps=1`、不判前缀），只读回执头里的
/// RAM 窗口 ⇒ ⇒ 打印的那一行是**服务端那块板**的窗口（`ram_window()` 的
/// 唯一落点），不是本 runner 自己挑段算的。
fn print_memory(cfg: &Config) -> Verdict {
    let body = request_body(cfg, 1, false);
    let reply = match post_boot(cfg, &body) {
        Ok(reply) => reply,
        Err(err) => return Verdict::Error(err),
    };
    if reply.http_code != 200 && reply.http_code != 504 {
        return Verdict::KernelError(format!("HTTP {}: {}", reply.http_code, reply.body_text()));
    }
    let Some(window) = reply.header("x-boot-ram-window") else {
        return Verdict::KernelError("the reply carries no x-boot-ram-window header".to_string());
    };
    if window.trim().is_empty() {
        return Verdict::KernelError(format!("board {:?} declares no RAM region", cfg.board));
    }
    // 形状必须是 `base:size` 且**十进制** ⇒ ⇒ Spike 的 `-m` 才吃得下。
    if let Err(msg) = parse_window(window.trim()) {
        return Verdict::KernelError(msg);
    }
    println!("{}", window.trim());
    Verdict::Done {
        reason: format!("ram-window={}", window.trim()),
    }
}

/// 跑一遍：一次 `/api/boot`，把回体原样吐出去，按回执头翻译停因。
fn run(cfg: &Config) -> Verdict {
    let body = request_body(cfg, cfg.budget, true);
    let reply = match post_boot(cfg, &body) {
        Ok(reply) => reply,
        Err(err) => {
            let _ = emit_bytes(cfg, &[]);
            return Verdict::Error(err);
        }
    };

    // ⚠️ 超时也把**已经拿到的**字节吐出去 —— 半个 console 也比什么都没有
    //   有用（比对脚本能看出「跑到哪一步停了」）。
    if let Err(verdict) = emit_bytes(cfg, &reply.body) {
        return verdict;
    }

    if reply.http_code != 200 && reply.http_code != 504 {
        return Verdict::KernelError(format!("HTTP {}: {}", reply.http_code, reply.body_text()));
    }

    let stop = reply.stop().unwrap_or("<missing x-boot-stop>").to_string();
    if cfg.verbose {
        eprintln!(
            "[osz-boot] server={} board={} stop={} console={} bytes",
            cfg.server,
            cfg.board,
            stop,
            reply.body.len()
        );
    }

    match stop.as_str() {
        "prefix" => Verdict::Done {
            reason: "prefix-matched".to_string(),
        },
        "budget" | "steps" => Verdict::Done {
            reason: "budget-exhausted".to_string(),
        },
        "halt" => Verdict::Done {
            reason: "end-of-execution".to_string(),
        },
        "timeout" => Verdict::Timeout,
        other => Verdict::KernelError(format!(
            "stopped by {other} (console={} bytes)",
            reply.body.len()
        )),
    }
}

impl BootReply {
    /// 错误回体的可读文本（`--out` 之类不该把它当 console 字节写出去）。
    fn body_text(&self) -> String {
        let text = String::from_utf8_lossy(&self.body);
        let text = text.trim();
        if text.len() > 400 {
            format!("{}…", &text[..400])
        } else {
            text.to_string()
        }
    }
}

/// 把 `base:size`（**十进制**）解回 `(base, size)`。
fn parse_window(text: &str) -> Result<(u32, u64), String> {
    let (base, size) = text
        .split_once(':')
        .ok_or_else(|| format!("the RAM window {text:?} is not `base:size`"))?;
    let base = base
        .parse::<u32>()
        .map_err(|_| format!("the RAM window's base {base:?} is not a decimal u32"))?;
    let size = size
        .parse::<u64>()
        .map_err(|_| format!("the RAM window's size {size:?} is not a decimal u64"))?;
    Ok((base, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_accepts_the_documented_forms() {
        let cfg = parse_args(&[
            "a.elf".to_string(),
            "--server".to_string(),
            "127.0.0.1:9876".to_string(),
            "--board".to_string(),
            "qemu-virt".to_string(),
            "--out".to_string(),
            "o.bin".to_string(),
            "--prefix".to_string(),
            "OSZ$ ".to_string(),
            "--budget".to_string(),
            "1234".to_string(),
            "--timeout-ms".to_string(),
            "500".to_string(),
            "-v".to_string(),
        ])
        .unwrap();
        assert_eq!(cfg.path, "a.elf");
        assert_eq!(cfg.server, "http://127.0.0.1:9876");
        assert_eq!(cfg.board, "qemu-virt");
        assert_eq!(cfg.out.as_deref(), Some("o.bin"));
        assert_eq!(cfg.prefix.as_deref(), Some("OSZ$ "));
        assert_eq!(cfg.budget, 1234);
        assert_eq!(cfg.timeout_ms, 500);
        assert!(cfg.verbose);
    }

    #[test]
    fn parse_args_rejects_bad_input_without_panicking() {
        for args in [
            vec![],
            vec!["--budget".to_string()],
            vec!["--budget".to_string(), "x".to_string()],
            vec!["--budget".to_string(), "0".to_string()],
            vec!["a.elf".to_string(), "b.elf".to_string()],
            vec!["--nope".to_string()],
            vec!["--max-console".to_string(), "16".to_string()],
        ] {
            assert!(
                parse_args(&args).is_err(),
                "args {args:?} should be rejected"
            );
        }
    }

    #[test]
    fn parse_args_accepts_print_memory() {
        let cfg = parse_args(&["a.elf".to_string(), "--print-memory".to_string()]).unwrap();
        assert!(cfg.print_memory);
        assert_eq!(cfg.path, "a.elf");
    }

    #[test]
    fn server_url_is_normalised() {
        assert_eq!(
            normalize_server("127.0.0.1:9876").unwrap(),
            "http://127.0.0.1:9876"
        );
        assert_eq!(
            normalize_server("http://127.0.0.1:9876/").unwrap(),
            "http://127.0.0.1:9876"
        );
        assert!(normalize_server("ftp://x").is_err());
        assert!(normalize_server("  ").is_err());
    }

    #[test]
    fn request_body_carries_the_documented_fields() {
        let cfg = parse_args(&[
            "a b.elf".to_string(),
            "--prefix".to_string(),
            "OSZ$ \"".to_string(),
            "--board".to_string(),
            "qemu-virt".to_string(),
        ])
        .unwrap();
        let body = request_body(&cfg, 7, true);
        assert!(
            body.contains("\"path\":\"a b.elf\""),
            "path 没进请求体：{body}"
        );
        assert!(body.contains("\"board\":\"qemu-virt\""));
        assert!(body.contains("\"steps\":7"));
        assert!(
            body.contains("\"prefix\":\"OSZ$ \\\"\""),
            "引号没被转义：{body}"
        );
        // `--print-memory` 那条**不带** prefix（查询不该受前缀影响）。
        let probe = request_body(&cfg, 1, false);
        assert!(!probe.contains("prefix"));
    }

    #[test]
    fn parse_window_rejects_hex_and_shapes() {
        assert_eq!(
            parse_window("2147483648:134217728").unwrap(),
            (2147483648, 134217728)
        );
        assert!(parse_window("0x80000000:134217728").is_err());
        assert!(parse_window("no-colon").is_err());
    }
}
