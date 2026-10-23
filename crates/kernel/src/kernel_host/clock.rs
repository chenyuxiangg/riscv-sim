//! 宿主单调时钟 —— 宿主时间预算与日志时间戳的**唯一**时钟源。
//!
//! ⚠️ **为什么不能直接用 `std::time::Instant`**：
//! `Instant::now()` 在 `wasm32-unknown-unknown` 上**直接 panic**
//! （该 target 没有可用的系统时钟 ⇒ `std` 的实现是
//! `panic!("time not implemented on this platform")`）。
//! ⇒ 一旦宿主直接用它，**WASM 构建一启动就崩**——实测 panic 落在
//! `KernelHost::with_description` 的栈里，顶帧是
//! `<std::time::Instant>::now` ⇒ 前端的 Worker **整个起不来**。
//!
//! ⇒ 这里给两种 target 各一份实现，**对外形状完全一样**：
//!
//! | target | 时钟源 | 精度 | 无时钟时 |
//! | --- | --- | --- | --- |
//! | native | `std::time::Instant` | 纳秒 | —— |
//! | `wasm32-unknown-unknown` | JS 的 `performance.now()` | 亚毫秒 | **返回 0**（不 trap） |
//!
//! ⚠️ **两边的纪元不同**（native = 进程启动，wasm = 页面加载 / Worker 启动）
//! ⇒ 但本模块**只暴露差值**（`elapsed_ms_since` / `HostClock::elapsed_ms`）
//! ⇒ 纪元差异**不可观测**。
//!
//! ⚠️ **优雅降级的代价**：若运行环境**没有** `performance`（裸 wasm 运行时、
//! 无 JS 宿主的 wasmtime 等）⇒ 读数恒为 `0` ⇒ 宿主时间预算这一路判据
//! **失效**（只剩「引擎停滞」那一路）⇒ **不 trap、不 panic**，如实降级。

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant as NativeInstant;

/// 宿主单调时钟的一个**读数**。
///
/// ⚠️ 它是 `Copy` 的：读数只是一个**时刻**，复制它不改变含义。
#[derive(Debug, Clone, Copy)]
pub struct HostInstant {
    #[cfg(not(target_arch = "wasm32"))]
    native: NativeInstant,
    #[cfg(target_arch = "wasm32")]
    wasm_ms: f64,
}

impl HostInstant {
    /// 取当前读数。
    pub fn now() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            HostInstant {
                native: NativeInstant::now(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            HostInstant {
                wasm_ms: wasm_now_ms(),
            }
        }
    }

    /// 距 `self`（一个**更早**的读数）过去了多少毫秒。
    ///
    /// ⚠️ 返回值**饱和**在 `0` ⇒ 读数乱序（理论上不可能，因为两边都是单调
    /// 时钟）不会给出「负的耗时」。
    pub fn elapsed_ms_since(&self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.native.elapsed().as_millis() as u64
        }
        #[cfg(target_arch = "wasm32")]
        {
            let delta = wasm_now_ms() - self.wasm_ms;
            if delta <= 0.0 {
                0
            } else {
                delta as u64
            }
        }
    }
}

/// 一个**会话级**的宿主时钟（原点在构造时）。
///
/// ⚠️ 宿主用它给 M-21 的日志条目与 M-22 的健康信号打 `time_host_ms` 戳。
#[derive(Debug, Clone, Copy)]
pub struct HostClock {
    origin: HostInstant,
}

impl HostClock {
    /// 开一个钟（原点 = 现在）。
    pub fn new() -> Self {
        HostClock {
            origin: HostInstant::now(),
        }
    }

    /// 距本钟原点过去了多少毫秒。
    pub fn elapsed_ms(&self) -> u64 {
        self.origin.elapsed_ms_since()
    }
}

impl Default for HostClock {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// wasm32 的时钟源
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod wasm_clock {
    use wasm_bindgen::prelude::wasm_bindgen;

    /// 宿主单调时钟（毫秒，亚毫秒精度）。
    ///
    /// ⚠️ `wasm32-unknown-unknown` **没有** `std::time::Instant`
    /// ⇒ 这里走 JS 侧的 `performance.now()`。
    ///
    /// ⚠️ **优雅降级**：若运行环境**没有** `performance` ⇒ 返回 `0`
    /// ⇒ 宿主时间预算这一路判据失效，但**不 trap、不 panic**。
    #[wasm_bindgen(inline_js = r#"
export function __riscv_host_now_ms() {
  return (typeof performance !== "undefined" && typeof performance.now === "function")
    ? performance.now()
    : 0;
}
"#)]
    extern "C" {
        fn __riscv_host_now_ms() -> f64;
    }

    pub(super) fn now_ms() -> f64 {
        __riscv_host_now_ms()
    }
}

#[cfg(target_arch = "wasm32")]
use wasm_clock::now_ms as wasm_now_ms;

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 刚取的读数 ⇒ 距它自己过去 **0** 毫秒（或极小的非负值）。
    #[test]
    fn a_fresh_reading_has_no_elapsed_time() {
        let reading = HostInstant::now();
        assert!(
            reading.elapsed_ms_since() < 50,
            "刚取的读数不该有可观的耗时，实际 {} ms",
            reading.elapsed_ms_since()
        );
    }

    /// 两个读数之间的差 **≥ 0** 且**单调**。
    #[test]
    fn elapsed_time_is_monotonic_and_never_negative() {
        let first = HostInstant::now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = HostInstant::now();
        let d1 = first.elapsed_ms_since();
        let d2 = second.elapsed_ms_since();
        // 同一个「现在」看两个读数：**先**取的读数看到的耗时**不小于**后取的
        // （后取的读数离「现在」更近 ⇒ 耗时更小）。
        assert!(d1 >= d2, "先取的读数看到的耗时应不小于后取的：{d1} vs {d2}");
        assert!(d1 >= 5, "睡过 5 ms ⇒ 至少看到 5 ms，实际 {d1}");
    }

    /// 会话级时钟的读数随时间**增长**。
    #[test]
    fn the_session_clock_advances() {
        let clock = HostClock::new();
        let before = clock.elapsed_ms();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let after = clock.elapsed_ms();
        assert!(after >= before, "时钟不该倒退：{before} → {after}");
        assert!(after >= 5, "睡过 5 ms ⇒ 至少看到 5 ms，实际 {after}");
    }
}
