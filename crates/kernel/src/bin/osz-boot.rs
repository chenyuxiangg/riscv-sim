//! `osz-boot` —— **headless** 启动一个真实 M 模式 RTOS，抓它的 console
//! 字节流，并与参考侧（Spike）**逐字节**比对。
//!
//! ## 它为什么存在
//!
//! 黄金对拍的老路（`riscv-golden`）是给 **riscv-tests** 用的：它要求
//! 镜像里有一个 `tohost` 符号（HTIF 的约定），跑到底后 guest 往
//! `tohost` 写一个判决码。**真实 RTOS 没有 `tohost`** —— osz 是自己写的
//! RTOS，它的「判决」是 **console 上打出来的字节**。
//!
//! ⇒ 本 runner 的判决口径是「**guest 打到 UART0 的字节流**」，
//! 与 Spike 参考侧（`spike ... <elf>` 把 UART0 打到 stdout）**逐字节**
//! 相同。
//!
//! ## 流约定（很重要）
//!
//! - **stdout** = console 的**原始字节**（不是文本、不是转义过的）⇒ 可以
//!   直接 `cmp` 与 Spike 的 stdout 比。
//! - **stderr** = 判决行（`VERDICT ...`）⇒ 不污染字节流。
//!
//! ## 用法
//!
//! ```text
//! osz-boot <elf> [--out <path>] [--prefix <text>] [--budget <n>]
//!           [--timeout-ms <n>] [--max-console <n>] [-v]
//! ```
//!
//! - `--prefix <text>`：console 字节流里**出现**这个前缀就停（默认不停，
//!   跑到预算或超时）。
//! - `--budget <n>`：指令预算（默认 200 000 000）。
//! - `--timeout-ms <n>`：**墙钟**超时（默认 30 000 ms）——真实 RTOS 是
//!   死循环，没有墙钟兜底会永远跑下去。
//!
//! ## 退出码
//!
//! | 码 | 含义 |
//! | --- | --- |
//! | 0 | 跑完了（达到 `--prefix` 或预算耗尽），字节流已写出 |
//! | 1 | 用法错误 / 读文件失败 |
//! | 2 | 超时（墙钟） |
//! | 3 | 内核报错（平台故障） |
//!
//! ⚠️ **跑完不等于成功**：字节流与参考侧是否**逐字节相同**是**比对脚本**
//! 的事（`tools/boot-osz.sh`）。本 runner 只负责「诚实地把字节流吐出来」。

use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource, ProductTree, Project};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use riscv_sim_kernel::kernel_host::platform;
use riscv_sim_kernel::kernel_host::KernelHost;
use riscv_sim_kernel::m14_bus::REGION_KIND_RAM;

/// 指令预算（默认）。
const DEFAULT_BUDGET: u64 = 200_000_000;

/// 墙钟超时（默认，毫秒）。
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// console 字节流的上限（默认）——防止死循环的 guest 把 stdout 灌爆。
const DEFAULT_MAX_CONSOLE: usize = 1 << 20;

/// 运行配置。
struct Config {
    path: String,
    out: Option<String>,
    prefix: Option<Vec<u8>>,
    budget: u64,
    timeout_ms: u64,
    max_console: usize,
    verbose: bool,
    /// 只打印 RAM 窗口（`base:size`，十进制）然后退出 —— **查询**，不是运行。
    print_memory: bool,
}

/// 解析命令行。**绝不 panic** —— 每个畸形输入都变成一条错误消息。
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut path: Option<String> = None;
    let mut out: Option<String> = None;
    let mut prefix: Option<Vec<u8>> = None;
    let mut budget = DEFAULT_BUDGET;
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    let mut max_console = DEFAULT_MAX_CONSOLE;
    let mut verbose = false;
    let mut print_memory = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--out" => {
                let raw = it.next().ok_or("--out needs a value")?;
                out = Some(raw.to_string());
            }
            "--prefix" => {
                let raw = it.next().ok_or("--prefix needs a value")?;
                prefix = Some(raw.as_bytes().to_vec());
            }
            "--budget" => {
                let raw = it.next().ok_or("--budget needs a value")?;
                budget = raw
                    .parse::<u64>()
                    .map_err(|_| format!("bad --budget value: {raw}"))?;
            }
            "--timeout-ms" => {
                let raw = it.next().ok_or("--timeout-ms needs a value")?;
                timeout_ms = raw
                    .parse::<u64>()
                    .map_err(|_| format!("bad --timeout-ms value: {raw}"))?;
            }
            "--max-console" => {
                let raw = it.next().ok_or("--max-console needs a value")?;
                max_console = raw
                    .parse::<usize>()
                    .map_err(|_| format!("bad --max-console value: {raw}"))?;
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
        out,
        prefix,
        budget,
        timeout_ms,
        max_console,
        verbose,
        print_memory,
    })
}

/// 判决。
enum Verdict {
    /// 跑完了。
    Done { reason: String },
    /// 用法 / IO 错误。
    Error(String),
    /// 墙钟超时。
    Timeout,
    /// 内核报错（平台故障）。
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
            "usage: osz-boot <elf> [--out <path>] [--prefix <text>] [--budget <n>]\n\
             \x20             [--timeout-ms <n>] [--max-console <n>] [-v]\n\
             \n\
             Runs a real RTOS headless and prints its console bytes on **stdout**.\n\
             The verdict line goes to **stderr** so stdout stays byte-exact."
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

    // ⚠️ `--print-memory` 是**查询**：打印窗口然后退出 ⇒ ⇒ **不跑** guest
    // ⇒ ⇒ stdout 上只有那一个 `base:size`（console 一个字节也没有）。
    if cfg.print_memory {
        return print_memory(&cfg);
    }

    let verdict = run(&cfg);
    eprintln!("{}", verdict.line());
    ExitCode::from(verdict.exit_code())
}

/// 装载镜像 —— **自动选档**要靠镜像的地址区间。
///
/// ⚠️ 装载**不跑** guest ⇒ ⇒ 只是把描述挑出来、把内存图建好。
fn load_host(bytes: Vec<u8>) -> Result<KernelHost, String> {
    let project = Project {
        id: "osz-boot".to_string(),
        name: "osz-boot".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "image.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(bytes),
        }],
        version: Default::default(),
    };

    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(0, CommandPayload::LoadProject(project)));
    if let Some(err) = first_error(&events) {
        return Err(err);
    }
    Ok(host)
}

/// 打印**实际会用的**那份描述的 RAM 窗口（`base:size`，**十进制**）。
///
/// ⚠️ **必须装载镜像**：自动选档（按镜像用到的地址挑描述）只有在**装载
/// 之后**才知道挑了哪一份 ⇒ ⇒ 打印的窗口才是「**它实际会用的**那一份」
/// 的窗口（STAGE-19 §6.2）。
///
/// ⚠️ **十进制**：Spike 的多段 `-m` 只吃十进制 ⇒ ⇒ 参考侧直接拼，不做
/// 进制转换。
fn print_memory(cfg: &Config) -> ExitCode {
    let bytes = match std::fs::read(&cfg.path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("osz-boot: read:{}:{}", cfg.path, e);
            return ExitCode::from(1);
        }
    };
    let host = match load_host(bytes) {
        Ok(host) => host,
        Err(err) => {
            eprintln!("osz-boot: {err}");
            return ExitCode::from(3);
        }
    };

    // ⚠️ 生效的那份描述 = **宿主选出来的那一份** ⇒ ⇒ 按 **id** 取回**同一份**
    // 内置描述 ⇒ ⇒ 「取哪个 RAM 段」这条规则仍然只有**契约层**那一个落点。
    let id = host.platform_id().to_string();
    let description = match platform::builtin_description(&id) {
        Ok(description) => description,
        Err(err) => {
            eprintln!("osz-boot: {err}");
            return ExitCode::from(3);
        }
    };
    let Some((base, size)) = description.ram_window() else {
        eprintln!("osz-boot: the effective description declares no RAM region");
        return ExitCode::from(3);
    };

    // The line the reference side reads.
    let printed = format!("{base}:{size}");

    // ⚠️ **T-5**：打印的那一行**必须**是宿主**实际映射**的窗口 ⇒ ⇒ 判据
    // 取自**总线自己的导出**（`host.memory_map()`），并把**打印的那一行**
    // **解回来**再比 ⇒ ⇒ 不是「我打印的 == 我打印的」那种同义反复 ⇒ ⇒
    // 谁把打印的值换成手写常量，**这里**就红。
    let (printed_base, printed_size) = match parse_window(&printed) {
        Ok(parsed) => parsed,
        Err(msg) => {
            eprintln!("osz-boot: {msg}");
            return ExitCode::from(3);
        }
    };
    let map = host.memory_map();
    let mapped = map.regions.iter().any(|region| {
        region.kind == REGION_KIND_RAM
            && region.base == printed_base
            && u64::from(region.size) == printed_size
    });
    if !mapped {
        eprintln!(
            "osz-boot: T-5: the RAM window we print ({printed}) is not the window the host mapped"
        );
        return ExitCode::from(3);
    }

    println!("{printed}");
    ExitCode::SUCCESS
}

/// 把 `base:size`（**十进制**）解回 `(base, size)`。
///
/// ⚠️ 这是 T-5 那条断言的**另一半**：判据必须取自「**真的**打印出去的那
/// 一行」，而不是「我们打算打印的那个变量」⇒ ⇒ 谁改了打印的内容，这里
/// 就解不出 / 解错 ⇒ ⇒ 断言红。
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

/// 跑一遍，把 console 字节流写到 stdout（以及可选的文件）。
fn run(cfg: &Config) -> Verdict {
    let bytes = match std::fs::read(&cfg.path) {
        Ok(b) => b,
        Err(e) => return Verdict::Error(format!("read:{}:{}", cfg.path, e)),
    };

    let mut host = match load_host(bytes) {
        Ok(host) => host,
        Err(err) => return Verdict::KernelError(err),
    };
    let mut seq: u64 = 1;
    if cfg.verbose {
        eprintln!(
            "[osz-boot] profile={:?} entry-pc=0x{:08x}",
            host.memory_profile(),
            host.pc()
        );
    }

    // ---- 跑 ----
    // ⚠️ 两份去处，职责不同：
    // * `console` —— **全部**捕获到的字节（写 `--out` 文件用）；
    // * stdout   —— **边跑边写**的原始字节（超时时也能看到跑到哪一步）。
    let mut console: Vec<u8> = Vec::new();
    let mut out = std::io::stdout();
    let started = Instant::now();
    let deadline = Duration::from_millis(cfg.timeout_ms);
    // ⚠️ 两个 `break` 分支都会给它赋值 ⇒ 这里不给初值（给初值会报
    // 「这个值从没被读过」）。
    let reason: &str;

    loop {
        if host.total_executed() >= cfg.budget {
            reason = "budget-exhausted";
            break;
        }
        if started.elapsed() >= deadline {
            // ⚠️ 超时也要把已经拿到的字节**吐出去** —— 半个 console 也比
            // 什么都没有有用（比对脚本能看出「跑到哪一步停了」）。
            let _ = out.flush();
            return Verdict::Timeout;
        }

        let events = host.handle_message(&Message::command(seq, CommandPayload::Continue));
        seq += 1;

        if let Some(err) = first_error(&events) {
            let _ = out.flush();
            return Verdict::KernelError(err);
        }

        // 取走这一批的 console 字节。
        let chunk = host.drain_console();
        if !chunk.is_empty() {
            console.extend_from_slice(&chunk);
            // ⚠️ 立刻写 stdout —— **原始字节**，不做任何转义 ⇒ 可以直接
            // 与 Spike 的 stdout 用 `cmp` 比。
            let _ = out.write_all(&chunk);
            let _ = out.flush();
            if console.len() > cfg.max_console {
                console.truncate(cfg.max_console);
            }
            if let Some(prefix) = &cfg.prefix {
                if contains(&console, prefix) {
                    reason = "prefix-matched";
                    break;
                }
            }
        }
    }

    let _ = out.flush();
    let console_len = console.len();

    if let Some(path) = &cfg.out {
        if let Err(e) = std::fs::write(path, &console) {
            return Verdict::Error(format!("write:{path}:{e}"));
        }
    }

    if cfg.verbose {
        eprintln!(
            "[osz-boot] executed={} console={} bytes pc=0x{:08x} now_ps={}",
            host.total_executed(),
            console_len,
            host.pc(),
            host.now_ps()
        );
    }

    Verdict::Done {
        reason: reason.to_string(),
    }
}

/// 子串查找（字节级）。
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// 从一批事件里找**第一个**错误。
fn first_error(events: &[EventPayload]) -> Option<String> {
    for e in events {
        if let EventPayload::Error { code, message } = e {
            return Some(format!("{code}:{message}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_accepts_the_documented_forms() {
        let cfg = parse_args(&[
            "a.elf".to_string(),
            "--out".to_string(),
            "o.bin".to_string(),
            "--prefix".to_string(),
            "OSZ$ ".to_string(),
            "--budget".to_string(),
            "1234".to_string(),
            "--timeout-ms".to_string(),
            "500".to_string(),
            "--max-console".to_string(),
            "16".to_string(),
            "-v".to_string(),
        ])
        .unwrap();
        assert_eq!(cfg.path, "a.elf");
        assert_eq!(cfg.out.as_deref(), Some("o.bin"));
        assert_eq!(cfg.prefix.as_deref(), Some(&b"OSZ$ "[..]));
        assert_eq!(cfg.budget, 1234);
        assert_eq!(cfg.timeout_ms, 500);
        assert_eq!(cfg.max_console, 16);
        assert!(cfg.verbose);
    }

    #[test]
    fn parse_args_rejects_bad_input_without_panicking() {
        for args in [
            vec![],
            vec!["--budget".to_string()],
            vec!["--budget".to_string(), "x".to_string()],
            vec!["a.elf".to_string(), "b.elf".to_string()],
            vec!["--nope".to_string()],
        ] {
            assert!(
                parse_args(&args).is_err(),
                "args {args:?} should be rejected"
            );
        }
    }

    #[test]
    fn parse_args_accepts_print_memory() {
        // ⚠️ `--print-memory` **仍然要 ELF** —— 自动选档要靠镜像的地址区间。
        let cfg = parse_args(&["a.elf".to_string(), "--print-memory".to_string()]).unwrap();
        assert!(cfg.print_memory);
        assert_eq!(cfg.path, "a.elf");
    }

    #[test]
    fn contains_finds_a_byte_substring() {
        assert!(contains(b"abcOSZ$ def", b"OSZ$ "));
        assert!(!contains(b"abc", b"OSZ$ "));
        assert!(contains(b"abc", b""));
    }
}
