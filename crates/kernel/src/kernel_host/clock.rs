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
//! | wasm32-unknown-unknown | JS 的 `performance.now()` | 亚毫秒 | **返回 `NaN`**（不 trap）⇒ ⇒ `available == false` |
//!
//! ⚠️ **两边的纪元不同**（native = 进程启动，wasm = 页面加载 / Worker 启动）
//! ⇒ 但本模块**只暴露差值**（`elapsed_ms_since` / `HostClock::elapsed_ms`）
//! ⇒ 纪元差异**不可观测**。
//!
//! ⚠️ **优雅降级的代价**：若运行环境**没有** `performance`（裸 wasm 运行时、
//! 无 JS 宿主的 wasmtime 等）⇒ 读数为 **`NaN`** ⇒ ⇒ `available == false`
//! ⇒ ⇒ 宿主时间预算这一路判据**失效**（只剩「引擎停滞」那一路）⇒ ⇒
//! **不 trap、不 panic**，如实降级。
//!
//! ⚠️ **「读不到时钟」不许用数值 0 兼职**（台账 **D-135** · 规格 **裁决 G**）：
//! ⇒ 「**读不到时钟**」（`NaN`）/「**读数为 0**」（`0.0`）/「**刚取过
//! 读数**」三种含义**必须分得开** ⇒ ⇒ 读数带**显式**的 `available`
//! ⇒ ⇒ JS 侧的「没有 `performance`」也编码成 **`NaN`**（**不是 `0`**）
//! ⇒ ⇒ 三种含义在**值**这一层就**已经**分开了。

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
    /// 宿主单调时钟**是否可用**（**显式**的一等公民 · 裁决 G）。
    ///
    /// ⚠️ 它**不是**「读数是不是 0」的代理 ⇒ ⇒ 三种含义**分得开**：
    ///
    /// | 情形 | `available` | 数值 |
    /// | --- | --- | --- |
    /// | **读不到**时钟（wasm 侧环境**没有** `performance`） | `false` | `NaN`（wasm） |
    /// | 读到了、读数**就是** 0 | `true` | `0.0` |
    /// | **刚取过**读数 | `true` | 任意**有限**值（差值 ≈ 0） |
    available: bool,
}

impl HostInstant {
    /// 取当前读数。
    pub fn now() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let native = NativeInstant::now();
            // ⚠️ native 的时钟**永远**可用 ⇒ ⇒ 这个读数恒为**有限值**
            // ⇒ ⇒ 「读不到时钟」那一支在 native 上**结构上走不到**
            // ⇒ ⇒ 而那条**判据本身**（[`reading_available`]）在 native 上
            // **可被单测**（喂 `NaN` 即可）⇒ ⇒ 不必起浏览器、不必跑 wasm。
            let reading_ms = native.elapsed().as_secs_f64() * 1000.0;
            HostInstant::from_parts(native, reading_available(reading_ms))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let wasm_ms = wasm_now_ms();
            // ⚠️ **单调**：一旦见过 `performance`，就一直当它**在**
            // ⇒ ⇒ `available` **不会**从真变假。
            let available = wasm_clock::sticky_available(reading_available(wasm_ms));
            HostInstant::from_parts(wasm_ms, available)
        }
    }

    /// **唯一的构造点**（平台相关的两处都走它 ⇒ ⇒ 「可用性」只有一处定义）。
    #[cfg(not(target_arch = "wasm32"))]
    fn from_parts(native: NativeInstant, available: bool) -> Self {
        HostInstant { native, available }
    }

    /// **唯一的构造点**（平台相关的两处都走它 ⇒ ⇒ 「可用性」只有一处定义）。
    #[cfg(target_arch = "wasm32")]
    fn from_parts(wasm_ms: f64, available: bool) -> Self {
        HostInstant { wasm_ms, available }
    }

    /// 宿主单调时钟**可用吗**（**显式** · 裁决 G）。
    ///
    /// ⚠️ **它不是**「读数是不是 0」的代理 ⇒ ⇒ 「读不到时钟」与「耗时
    /// 0 ms」与「刚取过读数」三种含义**分得开**。
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// 距 `self`（一个**更早**的读数）过去了多少毫秒。
    ///
    /// ⚠️ 返回值**饱和**在 `0` ⇒ 读数乱序（理论上不可能，因为两边都是单调
    /// 时钟）不会给出「负的耗时」。
    ///
    /// ⚠️ **宿主时钟不可用** ⇒ ⇒ 返回 **0**（**没有**任何时间信息）⇒ ⇒
    /// 「读不到时钟」与「耗时 0 ms」在**数值**上一样，但 [`is_available`]
    /// 把两者**分得开**（裁决 G）。
    pub fn elapsed_ms_since(&self) -> u64 {
        // ⚠️ **不可用 ⇒ 恒 0**：没有时钟就没有「时间信息」⇒ ⇒ 绝不能把
        // 「读不到」当成「耗时 0」（那是**兼职**，裁决 G 禁止）。
        if !self.available {
            return 0;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.native.elapsed().as_millis() as u64
        }
        #[cfg(target_arch = "wasm32")]
        {
            let delta = wasm_now_ms() - self.wasm_ms;
            if delta <= 0.0 || delta.is_nan() {
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

    /// 宿主单调时钟**可用吗**（**显式** · 裁决 G）。
    ///
    /// ⚠️ **它不是**「读数是不是 0」的代理 ⇒ ⇒ 「读不到时钟」与「耗时
    /// 0 ms」与「刚取过读数」三种含义**分得开**。
    pub fn is_available(&self) -> bool {
        self.origin.is_available()
    }
}

impl Default for HostClock {
    fn default() -> Self {
        Self::new()
    }
}

/// **纯函数**（两种 target **共用**）：原始读数（毫秒）⇒ 「宿主时钟读到了吗」。
///
/// ⚠️ **`NaN` = 「读不到时钟」** ⇒ ⇒ 「读不到时钟」与「读数为 0」在
/// **值**这一层就**分得开**（裁决 G）⇒ ⇒ 「**没有 `performance`**」那个
/// 宿主**可以被直接造出来**（喂 `NaN`）⇒ ⇒ 这条判据**不依赖浏览器**、
/// 不依赖 wasm 运行时就能单测。
///
/// ⚠️ **它是 native 侧的同一条判据**（native 的读数恒为**有限值** ⇒ ⇒
/// 恒 `true`）⇒ ⇒ native 上也能单测「读不到」那一支 ⇒ ⇒ 判据只有**一处**
/// 定义 ⇒ ⇒ 不会两处漂移。
fn reading_available(reading_ms: f64) -> bool {
    !reading_ms.is_nan()
}

// ---------------------------------------------------------------------------
// wasm32 的时钟源
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod wasm_clock {
    use std::sync::atomic::{AtomicBool, Ordering};

    use wasm_bindgen::prelude::wasm_bindgen;

    /// 「**见过** `performance`」的闩锁 —— 保证 `available` **单调**。
    ///
    /// ⚠️ **为什么需要它**：`performance` 在一个 JS 环境里**通常**不会消失，
    /// 但「读到了就一定读得到」这条**不变量**不该靠「通常」⇒ ⇒ 一旦见过
    /// 就**闩住**（`available` **不会**从真变假）⇒ ⇒ 单调是**结构性**的。
    static SEEN_PRESENT: AtomicBool = AtomicBool::new(false);

    /// 宿主单调时钟（毫秒，亚毫秒精度）。
    ///
    /// ⚠️ `wasm32-unknown-unknown` **没有** `std::time::Instant`
    /// ⇒ 这里走 JS 侧的 `performance.now()`。
    ///
    /// ⚠️ **优雅降级**：若运行环境**没有** `performance` ⇒ 返回 **`NaN`**
    /// （**不是 `0`**！）⇒ ⇒ 「读不到时钟」与「读数为 0」在**值**这一层
    /// 就**分得开**（裁决 G）⇒ ⇒ 判定落在 Rust 侧的 `reading_available`
    /// ⇒ ⇒ 单测直接喂 `NaN` 就能覆盖「没有 `performance`」那个宿主。
    ///
    /// ⚠️ **不 trap、不 panic** ⇒ 如实降级。
    #[wasm_bindgen(inline_js = r#"
export function __riscv_host_now_ms() {
  return (typeof performance !== "undefined" && typeof performance.now === "function")
    ? performance.now()
    : NaN;
}
"#)]
    extern "C" {
        fn __riscv_host_now_ms() -> f64;
    }

    pub(super) fn now_ms() -> f64 {
        __riscv_host_now_ms()
    }

    /// 可用性的**单调闩锁**：一旦见过 `performance`，就一直当它**在**。
    pub(super) fn sticky_available(present: bool) -> bool {
        if present {
            SEEN_PRESENT.store(true, Ordering::Relaxed);
            true
        } else {
            SEEN_PRESENT.load(Ordering::Relaxed)
        }
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

    /// 造一个「宿主时钟**不可用**」的读数 —— **模拟** wasm 侧「环境**没有**
    /// `performance`」那个宿主。
    ///
    /// ⚠️ 喂 **`NaN`** ⇒ ⇒ 走**同一条**判据（`reading_available`）⇒ ⇒
    /// **不必起浏览器、不必跑 wasm**就能覆盖那个宿主。
    ///
    /// ⚠️ native 侧的数值部分故意「**很旧**」（1.5 秒前）⇒ ⇒ 若实现忘了
    /// 「不可用 ⇒ 耗时恒 0」这条 ⇒ 本夹具会**暴露**它（耗时 ≈ 1 500 ms）。
    #[cfg(not(target_arch = "wasm32"))]
    fn unavailable_reading() -> HostInstant {
        HostInstant::from_parts(
            NativeInstant::now() - std::time::Duration::from_millis(1_500),
            reading_available(f64::NAN),
        )
    }

    /// wasm 侧的同一个夹具（数值部分同样是「读不到」的 `NaN`）。
    #[cfg(target_arch = "wasm32")]
    fn unavailable_reading() -> HostInstant {
        HostInstant::from_parts(f64::NAN, reading_available(f64::NAN))
    }

    /// native 侧：宿主时钟**永远可用** ⇒ ⇒ `available` 恒 `true`。
    ///
    /// ⚠️ **判别力**：把 native 的 `available` 写死成别的值 ⇒ 本条变红。
    #[test]
    fn a_native_reading_is_always_available() {
        assert!(
            HostInstant::now().is_available(),
            "native 的时钟（`std::time::Instant`）永远可用"
        );
        assert!(
            HostClock::new().is_available(),
            "native 的时钟永远可用 ⇒ 会话级钟也一样"
        );
    }

    /// 「**没有 `performance`**」那个宿主 = 读数是 `NaN` ⇒ ⇒ 判据**不依赖
    /// 浏览器**（台账 D-135 的判别面）。
    ///
    /// ⚠️ **判别力**：把 `!is_nan()` 写成 `true` ⇒ 本条变红。
    #[test]
    fn a_nan_reading_means_the_clock_cannot_be_read() {
        assert!(
            !reading_available(f64::NAN),
            "`NaN` = **读不到时钟**（wasm 侧 = 环境没有 `performance`）"
        );
        assert!(
            reading_available(0.0),
            "读数为 0 **不是**读不到时钟（那正是裁决 G 要防的「兼职」）"
        );
        assert!(reading_available(12.5), "读到一个有限的数 = 读到了");
    }

    /// 「**读不到时钟**」与「**刚取过读数**」**分得开**（裁决 G）。
    ///
    /// ⚠️ **判别力**：把 `available` 换成「数值是不是 0」⇒ 本条变红。
    #[test]
    fn an_unavailable_clock_is_not_the_same_as_a_fresh_reading() {
        let absent = unavailable_reading();
        assert!(!absent.is_available(), "「读不到时钟」必须**显式**说不可用");
        // ⚠️ 不可用 ⇒ **没有**时间信息 ⇒ 耗时恒 0（夹具的数值部分故意很旧
        // ⇒ ⇒ 若实现忘了这条，本断言会看到 ≈ 1 500 ms）。
        assert_eq!(
            absent.elapsed_ms_since(),
            0,
            "「读不到时钟」⇒ 耗时恒 0（**没有**时间信息）"
        );

        // 对照：**刚取过**的读数 —— 耗时也是 ≈ 0，但**可用** ⇒ ⇒ 两者
        // **分得开**（这正是「不许用数值 0 兼职」的意思）。
        let fresh = HostInstant::now();
        assert!(fresh.is_available(), "刚取过的读数**可用**");
        assert!(
            fresh.elapsed_ms_since() < 50,
            "刚取过的读数耗时极小，实际 {} ms",
            fresh.elapsed_ms_since()
        );
    }

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
