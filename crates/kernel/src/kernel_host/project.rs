//! 工程 / 板级内存图族：工程装载与卸载、整份 / 差量换图、修订号检查、
//! 内存图 / 模板清单 / 逐实例状态事件，以及差量合成纯函数。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。被父模块调度与
//! 订阅推送路径直接调用的方法提为 `pub(super)`，族内辅助保持私有。

use super::image::{load_segment_overlap_error, ParsedImage};
use super::platform;
use super::{HostWatchdog, KernelHost, MemoryProfile};
use riscv_sim_contract::error::{
    PlatformError, E_PLT_13_REVISION_CONFLICT, E_PLT_14_REGION_NOT_FOUND, E_PLT_17_BOARD_MISMATCH,
    E_PLT_22_NO_BOARD, PLATFORM_FAMILY,
};
use riscv_sim_contract::model::{PlatformTemplateInfo, Project};
use riscv_sim_contract::platform::{PlatformDescription, RegionSpec};
use riscv_sim_contract::port::ExecutionBackend;
use riscv_sim_contract::protocol::{EventPayload, MemoryMapOp};

impl KernelHost {
    /// 装载工程：**判定** → 复位 → 重建平台 → 加载镜像 → 换新会话（清订阅）。
    ///
    /// ⚠️ **复位排在判定之后**（STAGE-47 施工期订正 4）：初版把
    ///   `backend.reset()` 放在**函数开头** ⇒ ⇒ 「未知系统」判不过时
    ///   **机器已经被复位了**（PC 归零、寄存器清空、断点被清）⇒ ⇒ 与
    ///   冻结规格 §4.2 **K-2**（「PC 没动」）**逐字冲突** ⇒ ⇒ ⇒ 那条判据
    ///   在真机上**恒假**（PC 本来就是 0）⇒ ⇒ ⇒ **零判别力**。
    pub(super) fn on_load_project(&mut self, project: Project) -> Vec<EventPayload> {
        // ---- 第一步：**先解析**，不写总线 ----
        //
        // ⚠️ 顺序是刻意的：**档位要按镜像用到的地址挑** ⇒ 必须先知道
        // 镜像落在哪些地址，才知道该建哪一张内存图。
        //
        // 旧顺序是「先建图、再装载」—— 那只对「所有镜像都落在同一张
        // 图里」成立，真实 RTOS（osz）不成立。
        //
        // ⚠️ **解析是纯计算**（M-06）⇒ ⇒ 它**不碰机器状态** ⇒ ⇒ 所以
        //   「解析失败 ⇒ 机器复位」这条**旧行为**随复位一起挪走了 ⇒ ⇒
        //   现在是「**装载没成功 ⇒ 机器一个字都没动**」。
        let mut parsed: Vec<ParsedImage> = Vec::new();
        for image in &project.images {
            match self.parse_image(image) {
                Ok(p) => parsed.push(p),
                Err(err) => return vec![self.error_from(&err)],
            }
        }

        let mut entry = 0u32;
        // M-06 带出的**调试节区原始字节**（裁决 B）⇒ 交给 M-11 解析。
        //
        // ⚠️ 多个镜像的节区**合并**（每个工程可能有好几个镜像）。
        let mut debug_sections: Vec<(String, Vec<u8>)> = Vec::new();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for p in &parsed {
            // A raw blob has entry 0; it must not clobber an ELF's
            // entry point.
            if p.entry != 0 {
                entry = p.entry;
            }
            debug_sections.extend(p.debug_sections.iter().cloned());
            for (addr, bytes) in &p.segments {
                ranges.push((*addr, addr.saturating_add(bytes.len() as u32)));
            }
        }

        // ---- 第二步：挑平台描述（**判定在这之前**）----
        //
        // ⚠️ 跨镜像的装载段重叠必须在这里拒绝：解析是纯计算，拒绝时
        // 机器还没复位、平台还没重建、总线一个字都没写。
        if let Some(err) = load_segment_overlap_error(&parsed) {
            return vec![self.error_from(&err)];
        }
        // ⚠️ **重建平台**：**内存图 + 探针管道 + 外设探针回调**一起重来。
        //
        // ⚠️ 不能只 `reset_memory_map()` + 建图：
        // `reset_memory_map` 换上一条**全新**的总线，总线上挂的 hook
        // （M-17 的观察点 hook + M-18 的访存 trace hook）**一起没了**，
        // 外设的探针回调也随旧外设一起没了。探针管道必须在新的总线上
        // 重新装一遍（D-46）。
        //
        // ⚠️ **挑档判据已经描述化**（裁决 E）：挑出来的是**一份描述**
        // （不是「档位」枚举）⇒ 内存图由它决定。
        //
        // ⚠️ **STAGE-14 §3.2 第 2 步**：**配过板** ⇒ 用**用户的板**建图
        // （跳过自动选档）；**从未配过板** ⇒ 走今天的自动选档（**逐字不变**，
        // 验收 B-5）。
        //
        // ⚠️ **读，不消耗**（§2.2 的施工期订正）：初版这里是
        // `pending_platform.take()` ⇒ 用完就没了 ⇒ **用户装载前配的板会被
        // 紧接着的那次 `LoadProject` 覆盖掉** ⇒「装载前立即生效」成一句
        // 空话 ⇒ 用户的板是**用户意图**，**跨 `LoadProject` 保持**。
        //
        // ⚠️ **v0.31（STAGE-16 裁决 A 第 ③ 条）**：**待生效的板被应用** ⇒
        // 修订号 **+1** ⇒ 「待生效」是一次**生效的内存图变更** ⇒ 前端
        // 手里的修订号必须跟着更新（否则下一次提交会误报冲突）。
        //
        // ⚠️ 「待生效」是**推导量**（用户的板 ≠ 当前生效的图）⇒ 判据与
        // [`KernelHost::memory_map_pending`] **同一口径** ⇒ 这里**不**另立
        // 一个标志位。
        match &self.user_board {
            Some(board) => {
                // ⚠️ **STAGE-47 · D-175 候选 C ·「未知系统」门禁** ⇒ ⇒ 判定
                //   **只**对**用户配的板**做 ⇒ ⇒ 因为两条路各有一半：
                //   · **用户没配板** ⇒ ⇒ 走**自动选档** ⇒ ⇒ 自动选档**就是**按
                //     镜像的地址挑的 ⇒ ⇒ 它**必然**挑一块装得下的 ⇒ ⇒ ⇒
                //     「未知系统」**不该**出现 ⇒ ⇒ ⇒ **存量链路零影响** ✓；
                //   · **用户配了板** ⇒ ⇒ 用户**明确**了「我要用这块板」⇒ ⇒
                //     固件要的地址**完全不在**它上面 ⇒ ⇒ ⇒ **「未知系统」**
                //     ⇒ ⇒ **拒**。
                //
                // ⚠️ **在应用这块板**之前判 ⇒ ⇒ 判不过 ⇒ ⇒ `self.platform`
                //   **没被动过** ⇒ ⇒ **不建图、不写总线**（顺序是硬约束）。
                //
                // ⚠️ **而且机器整个没被动过**（STAGE-47 施工期订正 4）⇒ ⇒
                //   复位排在**这一步之后** ⇒ ⇒ 判不过 ⇒ ⇒ **正在跑的访客
                //   不受影响**（PC 不动、寄存器不动、断点不动）⇒ ⇒ ⇒ 与规格
                //   §4.2 **K-2** 逐字对齐。
                let board_ranges = platform::board_intervals(board);
                let verdict = platform::compatibility(&ranges, &board_ranges);
                // ⚠️ **中档**（用户 2026-11-01 拍板）：只有「**完全落不在**」
                //   才拒 ⇒ ⇒ 「**部分落不在**」**不拒** ⇒ ⇒ 撞的那部分由**总线**
                //   自己报（**既有**的错误码）⇒ ⇒ 本批**不新增**那一档。
                if verdict.missing {
                    // ⚠️ **把第一个落不在的区间说出来** ⇒ ⇒ 用户看得见**差在
                    //   哪**⇒ ⇒ 而不是只看到一个码。
                    let first_missing = ranges.iter().copied().find(|&(lo, hi)| {
                        if hi <= lo {
                            return false;
                        }
                        !board_ranges
                            .iter()
                            .any(|&(b_lo, b_hi)| lo < b_hi && b_lo < hi)
                    });
                    let detail = match first_missing {
                        Some((lo, hi)) => format!(
                            "未知系统：这个固件要用的地址 0x{lo:08x}..0x{:08x} \
                             不在当前这块板「{}」上",
                            hi.saturating_sub(1),
                            board.id
                        ),
                        None => format!(
                            "未知系统：这个固件要用的地址不在当前这块板「{}」上",
                            board.id
                        ),
                    };
                    return vec![self.error_from(&PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_17_BOARD_MISMATCH,
                        detail,
                    ))];
                }
                // ⚠️ **v0.75（STAGE-51 · D-175 的 F）**：**ISA 门禁** —— 在
                // **应用这块板之前**判（同「未知系统」门禁的次序纪律）⇒ ⇒
                // 判不过 ⇒ ⇒ `self.platform` **没被动过** ⇒ ⇒ 修订号不动 ⇒ ⇒
                // **不建图、不写总线** ⇒ ⇒ 机器一个字都没动（正在跑的访客
                // 不受影响）。
                //
                // ⚠️ 判据 = **这块即将生效的板**的 `mcu.isa`；缺省（无 mcu）
                // ⇒ 不校验（存量零风险）。只对 ELF 镜像判（`elf_isa.bits != 0`，
                // raw/hex 无 ELF 头 ⇒ 跳过）。
                if let Some(err) = Self::isa_gate_error(board, &parsed) {
                    return vec![self.error_from(&err)];
                }
                if self.platform != *board {
                    // ⚠️ **待生效**（用户的板 ≠ 当前生效的图）⇒ 它在这里
                    // **被应用** ⇒ 生效的图变了 ⇒ 修订号 +1。
                    self.map_revision += 1;
                }
                // ⚠️ **v0.73（STAGE-49 · B2）**：**分配基址** ⇒ ⇒ 生效图里
                // 那些段**真的有地址**（⇒ ⇒ 与 `on_set_memory_map` 第 ②b 步
                // **同一个落点**：装配期分配）。
                //
                // ⚠️ **装不下 ⇒ ⇒ 报既有码** ⇒ ⇒ **什么都不动**（下面那步
                // `rebuild_platform` 之前就退出了）⇒ ⇒ ⇒ **不静默**。
                let board = match platform::assign_bases(board) {
                    Ok(assigned) => assigned,
                    Err(err) => return vec![self.error_from(&err)],
                };
                self.platform = board.clone();
            }
            // ⚠️ **v0.76（STAGE-52 · B1 决议 / 裁决 C）**：**不兜底生效板** ⇒ ⇒
            //   用户没配板 ⇒ ⇒ **不再自动选档**（`choose_platform` 调用点摘除）
            //   ⇒ ⇒ 装载 ⇒ ⇒ **拒 + 引导**（`E_PLT-22`：先导入模板或配置板）。
            //
            // ⚠️ **次序纪律同款**：这一步在复位**之前** ⇒ ⇒ 拒 ⇒ ⇒ 机器
            //   一个字都没动 ⇒ ⇒ 正在跑的访客不受影响。
            None => {
                let detail = "没有生效板：这台机器还没配置板 —— 请先从模板导入 \
                     （default / minimal / qemu-virt）或配置一块板再装载"
                    .to_string();
                return vec![self.error_from(&PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_22_NO_BOARD,
                    detail,
                ))];
            }
        }
        // ---- 第三步：**换工程 ⇒ 复位机器** ----
        //
        // ⚠️ **复位排在判定之后**（STAGE-47 施工期订正 4）⇒ ⇒ 上面那两条
        //   「判不过」的返回路径上 ⇒ ⇒ **机器一个字都没动** ⇒ ⇒ 正在跑的
        //   访客**不受影响**（PC 不动、寄存器不动、断点不动）⇒ ⇒ ⇒ 与规格
        //   §4.2 **K-2**（「PC 没动」）**逐字对齐**。
        //
        // ⚠️ **为什么不能放回函数开头**：`backend.reset()` 把**引擎状态**
        //   整个清掉（PC、寄存器、虚拟时钟、事件队列）⇒ ⇒ 放前面 ⇒ ⇒ 拒绝
        //   也会把访客**悄悄抹掉** ⇒ ⇒ 用户看到的是「报了错、程序却也停了」
        //   ⇒ ⇒ **界面在骗人**。
        let _ = self.backend.reset();
        self.backend.clear_debug_state();
        self.watch_items.clear();
        self.watch_last.clear();

        // ---- 第四步：重建平台 ----
        if let Err(err) = self.rebuild_platform() {
            return vec![self.error_from(&err)];
        }

        // ---- 第五步：把镜像**写进**总线 ----
        for p in &parsed {
            for (addr, bytes) in &p.segments {
                if let Err(err) = self.backend.bus_mut().load_bytes(*addr, bytes) {
                    return vec![self.error_from(&err)];
                }
            }
        }
        self.backend.set_pc(entry);

        // ⚠️ **无条件重建** M-11 的索引（STAGE-5 §9.2）：装载新工程 = 新
        // 会话 ⇒ 旧的符号表 / 行表 / 反汇编缓存 / 轨迹环**全部**作废。
        //
        // ⚠️ 重建**失败**（符号表畸形 / 超上限）⇒ **拒绝**这次装载
        // （`E_LDR-01` / `E_LDR-02`），**不**截断后静默少给（验收 #8）。
        if let Err(err) = self.rebuild_symbol_index(&debug_sections) {
            return vec![self.error_from(&err)];
        }

        // ⚠️ 镜像装载本身会写总线（`load_bytes` 逐字节写）⇒ M-18 的访存
        // trace 环里多了一批**宿主侧**的写。
        //
        // 它们**不是**访客的执行历史 ⇒ **丢掉**（不是转移）。
        //
        // ⚠️ 为什么这里可以丢，而 D-51 说「不能定期 drain 掉」：D-51 怕的
        // 是把**访客执行历史**冲掉；装载镜像发生在**任何访客执行之前**，
        // 环里此刻只有宿主侧的镜像写。而且轨迹环是**定容**的 ⇒ 把这批噪声
        // 塞进去会把访客的执行历史挤出去。
        self.discard_m18_trace();

        // A new project is a new session: the old topic subscriptions die
        // with it (FR-13 — every session id owns its namespace). The host
        // has to subscribe again after a `LoadProject`.
        self.topics.clear();
        // The next register push must be a **full** snapshot.
        self.published_valid = false;
        self.delta_published_valid = false;

        // ⚠️ **装载成功** ⇒ 本会话**已经装载过镜像**（STAGE-14 §2.3 裁决 C）。
        //
        // ⚠️ 置真的位置在**全部步骤成功之后** ⇒ 装载**失败**（中途
        // `return`）时它**保持假** ⇒ 「有没有装载过」这个判据不会撒谎。
        //
        // ⚠️ **v0.88 订正**：判据是「机器里**有没有镜像字节**」⇒ 一个
        //   `images: []` 的空工程（产品视图「配板」用的就是它：配板＝换图
        //   + 开新会话 ⇒ ⇒ 镜像本来就是空的）**不许**把它置真。
        //   置真会让 `memory.map.loaded` 对界面撒谎（「已装载」但其实 PC=0、
        //   总线上什么都没有），还会让后续 `SetMemoryMap` 误判成「待生效」
        //   （`if !self.image_loaded` 那条立即生效的分支永远进不去）。
        let has_image_bytes = parsed
            .iter()
            .any(|p| p.segments.iter().any(|(_, bytes)| !bytes.is_empty()));
        self.image_loaded = has_image_bytes;

        // ⚠️ **镜像字节留在宿主**（v0.31 · STAGE-16 裁决 C 第 ④ 步）：
        // 记住**最后一个装载成功的工程** ⇒ 卸载之后**不必重新选文件**就能
        // 重装。
        //
        // ⚠️ 置的位置在**全部步骤成功之后** ⇒ 装载失败的工程**不被记住**
        // （记住一个装不上的工程没有意义）。
        self.last_project = Some(project);

        // ⚠️⚠️ **新会话 ⇒ 看门狗与健康状态从头开始**（v0.59 批 · 用户报的现象
        // 「复位后无法再运行」的第三段）。
        //
        // ⚠️ **为什么必须在这儿重置**：看门狗触发 ⇒ ⇒ `observe_continue_window`
        // 与 `step_in_chunks` 的闸门**永久打开** ⇒ ⇒ 内核**再也跑不起来** ⇒ ⇒
        // 而 `LoadProject`（= 复位）**本来不重置**它 ⇒ ⇒ 「复位救不回一个被
        // 看门狗杀死的会话」⇒ ⇒ FR-06 的验收「复位回到初始态」**做不到**。
        //
        // ⚠️ **两个标志必须**一起**重置**（**不是只重置一个**）：
        // - `self.watchdog`（宿主侧）= **闸门本身** ⇒ ⇒ 不重置 ⇒ ⇒ `Continue`
        //   永远返回「内核已被看门狗终止，不再接受新的运行」；
        // - `self.hms`（M-22 侧）= **对外的说法** ⇒ ⇒ 不重置 ⇒ ⇒ 宿主仍然
        //   报「内核已被终止」⇒ ⇒ **内核活着但界面说它死了**（两个真相）⇒ ⇒
        //   而且 `Hms::report` 的降级闸门（`!watchdog_fired`）**关着** ⇒ ⇒
        //   第二次看门狗触发**不会**再终止内核 ⇒ ⇒ 行为**自相矛盾**。
        //
        // ⚠️ **位置在**全部步骤成功之后** ⇒ ⇒ 装载**失败**时**不动**它 ⇒ ⇒
        // 「装载失败 ⇒ 会话没换 ⇒ 健康状态照旧」⇒ ⇒ 判据不会撒谎。
        //
        // ⚠️ **健康状态的会话边界 = 装载新工程**（FR-13：新工程 = 新会话 ⇒ 主题
        // 命名空间随之换）⇒ ⇒ 「会话」这件事在本宿主里**已经有了一个落点** ⇒ ⇒
        // 健康状态跟着它走 ⇒ ⇒ **不是第二个真相**。
        self.watchdog = HostWatchdog::new();
        self.hms.reset_session();

        let mut events = vec![self.tick()];
        if let Some(wave) = self.register_waveform(true) {
            events.push(wave);
        }
        events
    }

    /// 「用户配的板」是不是**还没生效**（STAGE-14 §3.2 ④ 的**推导量**）。
    ///
    /// ⚠️ **推导，不单独存**：用户的板 ≠ 当前生效的图 ⇒ 还没生效。
    ///
    /// ⚠️ **为什么是推导量**：存两个字段（「配的板」+ 一个 `pending` 标志）
    /// ⇒ 「配了板但没生效」与「标志忘了翻」是**两种不同的错** ⇒ 前者推导
    /// 不出来、后者无从发现 ⇒ 只留一个字段就少一处可能不同步的地方。
    ///
    /// ⚠️ `PlatformDescription` 派生了 `PartialEq` ⇒ 这个比较是**类型安全**的。
    fn memory_map_pending(&self) -> bool {
        match &self.user_board {
            Some(board) => self.platform != *board,
            None => false,
        }
    }

    /// **设置板级内存图**（用户在界面上配的那张板）。
    ///
    /// ⚠️ **顺序固定**（STAGE-16 §3.1，冻结规格 —— 在 v0.29 的五步前
    /// **插一步**修订号检查，其余顺序**一字不改**）：
    ///
    /// ```text
    /// ① 修订号检查（expected_revision 若给出且 ≠ 当前 ⇒ E_PLT_13）⇒ 什么都不动
    /// ② description.validate()        ← 契约层那个唯一校验器
    /// ③ user_board = Some(description) ← 记下**用户意图**
    /// ④ 若 !image_loaded ⇒ 立即生效（换图 + 重建总线）
    /// ⑤ map_revision += 1
    /// ⑥ 回 EventPayload::MemoryMap { description, pending, loaded, revision }
    /// ```
    ///
    /// ⚠️ **第 ① 步排在第 ② 步之前**（裁决 B）⇒ 「你是不是基于旧状态」
    /// 比「这份描述合不合法」**更先**回答 ⇒ 两个错不会互相盖住。
    ///
    /// ⚠️ **校验失败时「什么都不动」**：不合规的描述**不能**污染宿主当前的
    /// 内存图 ⇒ `self.platform` 与 `user_board` 都保持原样。
    ///
    /// ⚠️ **已装载镜像时不重建总线**（裁决 B）：正在跑的 guest **一字不受
    /// 影响** ⇒ 新板等下一次 `LoadProject` 生效（事件回 `pending = true`）。
    ///
    /// ⚠️ **重建失败时 `user_board` 保留**：用户的板还在，只是**没生效**
    /// ⇒ `pending` 变真 ⇒ 用户改板这件事**没有丢**。
    pub(super) fn on_set_memory_map(
        &mut self,
        description: PlatformDescription,
        expected_revision: Option<u64>,
    ) -> Vec<EventPayload> {
        // ---- ① 修订号检查（**排在 validate() 之前** —— 裁决 B）----
        //
        // ⚠️ 失败 ⇒ **什么都不动**（图、用户的板、修订号三者都不变）。
        if let Err(err) = self.check_revision(expected_revision) {
            return vec![self.error_from(&err)];
        }

        // ---- ② 校验（契约层那个唯一校验器 ⇒ E_PLT-01..E_PLT-16）----
        //
        // ⚠️ 失败 ⇒ **什么都不动**（当前图与用户的板都不碰）⇒ 只回一条
        // `Error` 事件。
        if let Err(err) = description.validate() {
            return vec![self.error_from(&err)];
        }

        // ---- ②b **分配基址**（v0.73 · STAGE-49 · B2）----
        //
        // ⚠️ **为什么在这里分配**（不是只在建图时）：**生效图**要带**具体
        // 基址** ⇒ ⇒ 前端拿到的 `memory.map` 事件里那些段**真的有地址**
        // ⇒ ⇒ ⇒ 「装配期分配」这件事**看得见**（不是只在内核内部发生）。
        //
        // ⚠️ **老描述 ⇒ 分配是恒等变换**（⇒ ⇒ **一个字节都不变**）⇒ ⇒
        // 存量板档案**零风险**（裁决 K）。
        //
        // ⚠️ **装不下 ⇒ ⇒ 报既有码**（`E_PLT-07`）⇒ ⇒ **零新场景码**。
        let description = match platform::assign_bases(&description) {
            Ok(assigned) => assigned,
            Err(err) => return vec![self.error_from(&err)],
        };

        // ---- ③ 记下**用户配的板**（裁决 A：住在宿主会话内）----
        //
        // ⚠️ 存的是**一份拷贝** ⇒ 第 ④ 步可以把原件直接换进 `self.platform`
        // （`PlatformDescription` 是 `Clone` 的数据，不是什么昂贵的东西）。
        //
        // ⚠️ **不消耗**它：用户的板是**用户意图** ⇒ 跨 `LoadProject` 保持
        // （§2.2 的施工期订正）。
        self.user_board = Some(description.clone());

        // ---- ④ 未装载过镜像 ⇒ **立即生效**（裁决 B）----
        if !self.image_loaded {
            // ⚠️ `rebuild_platform()` 读的是 `self.platform` ⇒ 先换图。
            let old = self.platform.clone();
            self.platform = description;
            if let Err(err) = self.rebuild_platform() {
                // ⚠️ **恢复旧图**：重建的第一步就是 `reset_memory_map()` ⇒
                // 失败后总线可能是**空的** ⇒ 不恢复的话用户拿到的是一张
                // 空总线（比报错更糟）。
                //
                // ⚠️ 恢复是**尽力而为**（`let _ =`）：**原始错误**才是要
                // 报出去的那一个 ⇒ 不被恢复的成败掩盖。
                self.platform = old;
                let _ = self.rebuild_platform();
                // ⚠️ `user_board` **保留**（用户的板还在，只是没生效）。
                //
                // ⚠️ **修订号不 +1**：重建失败 ⇒ 这次变更**没被接受**。
                return vec![self.error_from(&err)];
            }
        }

        // ---- ⑤ 修订号 +1（裁决 A：每次**被接受**的变更）----
        self.map_revision += 1;

        // ---- ⑥ 回「当前生效的图」+「用户的板有没有生效」（裁决 D / M）----
        // ⚠️ **顺带回逐实例状态**（STAGE-43 · 裁决 G）：图**变了** ⇒ 实例
        // 的绑定**也可能变了** ⇒ ⇒ 同一个时机推 ⇒ 前端不必轮询。
        vec![self.memory_map_event(), self.device_instances_event()]
    }

    /// **差量更新**板级内存图（v0.31 · STAGE-16 §3.2 · D-137）。
    ///
    /// ⚠️ **顺序固定**（冻结规格）：
    ///
    /// ```text
    /// ① 修订号检查（⇒ E_PLT_13）⇒ 什么都不动
    /// ② 合成整图：以 self.platform（当前生效的图）为底，逐条施加 ops
    /// ③ 合成结果整体 validate()（⇒ E_PLT-*）⇒ 什么都不动
    /// ④ user_board = Some(合成结果)
    /// ⑤ 若 !image_loaded ⇒ 换图 + rebuild_platform()（失败 ⇒ 恢复旧图、用户的板保留）
    /// ⑥ map_revision += 1
    /// ⑦ 回 EventPayload::MemoryMap { description, pending, loaded, revision }
    /// ```
    ///
    /// ⚠️ **合成期间不碰任何状态**（第 ② 步在**内存里的副本**上做）⇒
    /// 第 ② / ③ 步失败 ⇒ 宿主的图、用户的板、修订号**三者都不变**。
    ///
    /// ⚠️ **整体 `validate()`**（裁决 J）⇒ 差量改完**仍**按整图校验
    /// （V-6 重叠检查、V-7 预算都在其中）。
    ///
    /// ⚠️ **按 `base` 定位 ⇒ 与数组顺序无关**（台账判别力 ③）。
    ///
    /// ⚠️ **v0.32 · STAGE-17 裁决 A**：第 ② 步的合成已剥成**纯函数**
    /// [`compose_memory_map`] ⇒ 它**只读** `self.platform` + **只写**返回值
    /// ⇒ ⇒ 「合成期间不碰任何状态」这条既有语义**一字不改** ⇒ ⇒ 而「两条
    /// 路径产出的图必须一致」这条承诺从此有了一条**可执行的断言**
    /// （`tests_board_delta_equivalence.rs` · B-1 ~ B-7）。
    pub(super) fn on_patch_memory_map(
        &mut self,
        ops: Vec<MemoryMapOp>,
        expected_revision: Option<u64>,
    ) -> Vec<EventPayload> {
        // ---- ① 修订号检查（**排在合成之前** —— 裁决 B）----
        if let Err(err) = self.check_revision(expected_revision) {
            return vec![self.error_from(&err)];
        }

        // ---- ② 合成整图（**纯函数** ⇒ 合成期间不碰任何状态）----
        //
        // ⚠️ **基底 = 当前生效的图**（`self.platform`，裁决 I）⇒ 与前端
        // 算差量的基准（最近一次 `memory.map` 事件带回来的生效图）**是同
        // 一个东西** ⇒ 不需要额外的「草稿基座」状态。
        //
        // ⚠️ **合成失败 ⇒ 什么都不动**（第 ② / ③ 步失败 ⇒ 宿主的图、
        // 用户的板、修订号**三者都不变**）⇒ 只回一条 `Error` 事件。
        //
        // ⚠️ **校验不在纯函数里**（v0.32 · STAGE-17 裁决 A）：纯函数**不跑**
        // `validate()` ⇒ 校验**留在第 ③ 步**（契约层那个唯一校验器）⇒
        // ⇒ **既有语义一字不改**（裁决 M）。
        let synthesized = match compose_memory_map(&self.platform, &ops) {
            Ok(synthesized) => synthesized,
            Err(err) => return vec![self.error_from(&err)],
        };

        // ---- ③ 合成结果**整体**校验（契约层那个唯一校验器）----
        //
        // ⚠️ 失败 ⇒ **什么都不动** ⇒ 只回一条 `Error` 事件。
        if let Err(err) = synthesized.validate() {
            return vec![self.error_from(&err)];
        }

        // ---- ③b **分配基址**（v0.73 · STAGE-49 · B2）----
        //
        // ⚠️ **与整份路径同一处**（[`on_set_memory_map`] 的第 ②b 步）⇒ ⇒
        // 「装配期分配」这条约束**只有一个落点**（一个约束一个落点）。
        //
        // ⚠️ **差量路径的基底 = 生效图**（每段**都带**基址）⇒ ⇒ ⇒ 分配是
        // **恒等变换**（⇒ ⇒ 老板**逐字不变**）。
        //
        // ⚠️ ⚠️ **保留原件**（与整份路径同一纪律）：`user_board` 记的是**用户
        //   意图**⇒ ⇒ ⇒ 存**未分配**的那份。
        let user_intent = synthesized.clone();
        let synthesized = match platform::assign_bases(&synthesized) {
            Ok(assigned) => assigned,
            Err(err) => return vec![self.error_from(&err)],
        };

        // ---- ④ 记下**用户配的板**（合成结果本身就是整图）----
        self.user_board = Some(user_intent);

        // ---- ⑤ 未装载过镜像 ⇒ **立即生效**----
        if !self.image_loaded {
            let old = self.platform.clone();
            self.platform = synthesized;
            if let Err(err) = self.rebuild_platform() {
                // ⚠️ **恢复旧图**（同 `on_set_memory_map` 的恢复路径）。
                self.platform = old;
                let _ = self.rebuild_platform();
                // ⚠️ `user_board` **保留**；修订号**不 +1**（没被接受）。
                return vec![self.error_from(&err)];
            }
        }

        // ---- ⑥ 修订号 +1（裁决 A）----
        self.map_revision += 1;

        // ---- ⑦ 回事件（裁决 D / M）----
        // ⚠️ **顺带回逐实例状态**（STAGE-43 · 裁决 G）—— 与整份替换**同形**。
        vec![self.memory_map_event(), self.device_instances_event()]
    }

    /// **卸载工程**（v0.31 · STAGE-16 §3.3 · D-139 · 裁决 C）。
    ///
    /// ⚠️ **六步固定**（冻结规格）：
    ///
    /// ```text
    /// ① backend.reset() + backend.clear_debug_state()  ← 清寄存器 / 事件队列 / 断点 / 观察点
    /// ② self.topics.clear()          ← 新会话 ⇒ 订阅全部失效 ⇒ 前端必须重订
    /// ③ self.image_loaded = false    ← ⇒ 下一次改板立即生效
    /// ④ 镜像字节留在宿主（last_project）⇒ 不必重新选文件
    /// ⑤ 用户的板（user_board）留着  ← 它是用户意图，不是运行态
    /// ⑥ 回至少一条事件
    /// ```
    ///
    /// ⚠️ **修订号不 +1**（卸载不改图，只改「有没有装载镜像」⇒ 裁决 A）。
    ///
    /// ⚠️ **回事件不能是空数组**：前端 M-05 靠**第一个**带 `reqId` 的事件
    /// 落地 Promise ⇒ 返回空数组会让前端**永远等不到**。
    pub(super) fn on_unload_project(&mut self) -> Vec<EventPayload> {
        // ---- ① 清**运行态**（寄存器 / 事件队列 / 断点 / 观察点）----
        let _ = self.backend.reset();
        self.backend.clear_debug_state();
        self.watch_items.clear();
        self.watch_last.clear();

        // ---- ② 新会话 ⇒ 订阅全部失效（前端**必须重订**）----
        self.topics.clear();

        // ---- ③ 「有没有装载镜像」变假 ⇒ **之后改板立即生效**----
        self.image_loaded = false;

        // ---- ④ 镜像字节**留着**（`last_project` 不清）----
        // ---- ⑤ 用户的板**留着**（`user_board` 不动）----

        // ---- ⑥ 回**至少一条**事件 ----
        vec![self.memory_map_event()]
    }

    /// **修订号检查**（v0.31 · STAGE-16 裁决 B）。
    ///
    /// ⚠️ **`None` ⇒ 不检查**（旧调用方 / 脚本仍能用）。
    ///
    /// ⚠️ **`Some(r)` 且 `r != 当前` ⇒ 明确拒绝**（`E_PLT_13`）⇒
    /// **什么都不动**（图、用户的板、修订号三者都不变）⇒ 并发丢更新
    /// **看得见**。
    fn check_revision(&self, expected_revision: Option<u64>) -> Result<(), PlatformError> {
        match expected_revision {
            None => Ok(()),
            Some(expected) if expected == self.map_revision => Ok(()),
            Some(expected) => Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_13_REVISION_CONFLICT,
                format!(
                    "期望的修订号 {expected} 与宿主当前的修订号 {} 不一致（图已被改过）",
                    self.map_revision
                ),
            )),
        }
    }

    /// 一条 `MemoryMap` 事件 —— **板级内存图的完整状态**（裁决 M）。
    ///
    /// ⚠️ **四个字段**：`description`（当前生效的图）· `pending`（用户的板
    /// 有没有生效 —— 推导量）· `loaded`（有没有装载镜像）· `revision`
    /// （修订号）。
    ///
    /// ⚠️ **只有这一个落点** ⇒ §3.1 第 ⑥ 步 · §3.2 第 ⑦ 步 · §3.3 第 ⑥ 步
    /// · §3.4 的即时推送 —— 四处**同形**。
    pub(crate) fn memory_map_event(&self) -> EventPayload {
        EventPayload::MemoryMap {
            description: Box::new(self.platform.clone()),
            pending: self.memory_map_pending(),
            loaded: self.image_loaded,
            revision: self.map_revision,
        }
    }

    /// 一条 `DeviceInstances` 事件 —— **逐实例状态**（STAGE-43 · v0.66）。
    ///
    /// ⚠️ **逐实例**（裁决 B）—— 不是板级：板级的那一份已经有
    /// [`Self::memory_map_event`] 了 ⇒ ⇒ 这一条说的是「**每个实例**」。
    /// 模板清单事件 —— **订阅即推 / 命令回复共用**（v0.76 · STAGE-52 ·
    /// 裁决 M）。
    ///
    /// ⚠️ 顺序**按 priority 升序** ⇒ 确定性（default 10 → qemu-virt 20 →
    /// minimal 30）⇒ ⇒ **数值不在这份代码里**（那是内置 JSON 的事）。
    pub(crate) fn platform_templates_event(&self) -> EventPayload {
        let ids = [
            platform::PLATFORM_ID_DEFAULT,
            platform::PLATFORM_ID_QEMU_VIRT,
            platform::PLATFORM_ID_MINIMAL,
        ];
        let mut templates: Vec<PlatformTemplateInfo> = ids
            .iter()
            .filter_map(|id| {
                let description = platform::builtin_description(id).ok()?;
                let device_count = description
                    .regions
                    .iter()
                    .filter(|region| matches!(region, RegionSpec::Peripheral { .. }))
                    .count() as u32;
                Some(PlatformTemplateInfo {
                    id: (*id).to_string(),
                    name: description.name.clone(),
                    priority: description.priority,
                    device_count,
                })
            })
            .collect();
        templates.sort_by_key(|entry| entry.priority);
        EventPayload::PlatformTemplates { templates }
    }

    pub(crate) fn device_instances_event(&self) -> EventPayload {
        EventPayload::DeviceInstances {
            instances: self.reported_device_instances(),
        }
    }

    /// **报出去**的逐实例清单（STAGE-43 · 裁决 K / L）。
    ///
    /// ⚠️ **用户的板优先**（裁决 K）：前端定义的那块板才是主角 ⇒ ⇒ 报的是
    /// **它**的实例；**没有**用户板时才报生效图的。
    ///
    /// ⚠️ **「已绑定」只有一个判据**（裁决 L）：该实例（**id + 类型 +
    /// 基址**）在**生效图**里**逐字段相同** ⇒ ⇒ `base = Some(...)`；
    /// 否则 ⇒ `None`（**待生效**）⇒ ⇒ 「已绑定 / 待生效」由**一个字段**
    /// 表达 ⇒ ⇒ 不用两个字段互相推导。
    ///
    /// ⚠️ **窗口大小来自类型注册表**（M-15）⇒ ⇒ 「每个类型多大」**只写
    /// 一处** ⇒ ⇒ 这里**不**从总线反推。
    ///
    /// @returns {Array<object>} — `DeviceInstanceInfo` values
    fn reported_device_instances(&self) -> Vec<riscv_sim_contract::model::DeviceInstanceInfo> {
        let Some(board) = self.user_board.as_ref() else {
            // ⚠️ 没有用户板 ⇒ 报**生效图**的那些（全部已绑定）。
            let mut out = self.device_instances.clone();
            out.sort_by(|a, b| a.id.cmp(&b.id));
            return out;
        };
        let mut out = Vec::new();
        for region in board.regions.iter() {
            let riscv_sim_contract::platform::RegionSpec::Peripheral { base, device } = region
            else {
                continue;
            };
            let id = device.id().to_string();
            let kind = device.kind().to_string();
            // ⚠️ **窗口大小来自类型注册表** ⇒ 「类型多大」只有一处真相。
            let window_bytes = crate::m15_peripheral::types::window_bytes(&kind).unwrap_or(0);
            // ⚠️ **唯一判据**：生效图里有**同一个 id + 同一个类型 + 同一个
            // 基址** ⇒ ⇒ 已绑定；否则 ⇒ 待生效。
            //
            // ⚠️ **v0.73（STAGE-49 · B2）**：**缺 `base`** 的段 ⇒ ⇒ 它**还没
            // 有位置** ⇒ ⇒ ⇒ 判不出「绑在哪儿」⇒ ⇒ ⇒ **不静默**当成
            // 「基址 0」（那会与真的 `base = 0` 混成一样）⇒ ⇒ ⇒ 这里**跳过**
            // （⇒ ⇒ `bound` 留 `None` ⇒ ⇒ 前端看得见「待生效」）。
            let bound = base.and_then(|base| {
                self.device_instances
                    .iter()
                    .find(|inst| inst.id == id && inst.kind == kind && inst.base == Some(base))
                    .and_then(|inst| inst.base)
            });
            out.push(riscv_sim_contract::model::DeviceInstanceInfo {
                id,
                kind,
                base: bound,
                window_bytes,
            });
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// 当前生效的内存图档位（**兼容视图**，裁决 C）。
    ///
    /// ⚠️ 它是当前生效的**平台描述的 `id` 的投影**：`default` ⇒
    /// `Default`、`qemu-virt` ⇒ `QemuVirt`、**其它 id ⇒ `Default`**。
    ///
    /// ⚠️ **诚实但信息量低**：新平台（例如 `minimal`）会返回 `Default`
    /// ⇒ 想知道**真实**的平台请用 [`KernelHost::platform_id`]。
    ///
    /// ⚠️ 这个 API 是**公开**的（5 个调用点）⇒ 既有断言**一条不改**。
    pub fn memory_profile(&self) -> MemoryProfile {
        match self.platform.id.as_str() {
            platform::PLATFORM_ID_QEMU_VIRT => MemoryProfile::QemuVirt,
            _ => MemoryProfile::Default,
        }
    }

    /// 当前生效的**平台 id**（**真实**的 id 字符串）。
    ///
    /// ⚠️ 这是 v0.27 **新增**的接口：内存图不再只有两档 ⇒ 「档位」这个
    /// 概念已经装不下真实的信息 ⇒ 真实 id 由这里给出。
    pub fn platform_id(&self) -> &str {
        &self.platform.id
    }

    /// 当前生效的**外设中断号表**（每个 UART / GPIO 一个 PLIC 源号）。
    pub fn peripheral_irq_table(&self) -> &[platform::PeripheralIrq] {
        &self.peripheral_irqs
    }

    /// 板级内存图的**修订号**（v0.31 · STAGE-16 裁决 A）。
    ///
    /// ⚠️ **单调递增**的计数器 ⇒ 前端提交时把它作为
    /// `expected_revision` 带上 ⇒ 并发丢更新**看得见**。
    pub fn map_revision(&self) -> u64 {
        self.map_revision
    }

    /// 最后一个**装载成功**的工程（v0.31 · STAGE-16 裁决 C 第 ④ 步）。
    ///
    /// ⚠️ **镜像字节住在它里面** ⇒ 卸载之后**不必重新选文件**就能重装。
    ///
    /// ⚠️ `None` = 这个会话**还没装载成功过**任何工程。
    pub fn last_project(&self) -> Option<&Project> {
        self.last_project.as_ref()
    }

    /// 只读地看**器件实例表**（诊断 / 测试用 · v0.74 · STAGE-50）。
    ///
    /// ⚠️ **为什么暴露它**：引脚级连线的判据要**真的去读那台器件**
    ///   （⇒ ⇒ 「内核造的是**那台**器件」不能只靠注册表）⇒ ⇒ ⇒ 测试与
    ///   诊断都需要**按 id 拿句柄**。
    pub fn device_table(&self) -> &crate::kernel_host::platform::DeviceInstanceTable {
        &self.devices
    }
}

// ---------------------------------------------------------------------------
// 差量合成（v0.32 · STAGE-17 裁决 A · D-145）
// ---------------------------------------------------------------------------

/// **差量合成** —— 把一串差量操作施加到一张**基座**图上，返回**整图**
/// （v0.32 · STAGE-17 裁决 A · **D-145** 闭环）。
///
/// ⚠️ **纯函数**：只读 `base` + 只写返回值 ⇒ **不碰宿主任何状态**
/// （生效的图 / 用户的板 / 修订号 / 总线）⇒ ⇒ 合成失败 ⇒ 宿主状态
/// **一个字都不动**。
///
/// ⚠️ **不进契约层**（裁决 A）：它是**宿主内部**的实现细节 ⇒ 契约层的
/// 公开表面**一字未改** ⇒ 前端镜像**不需要**升版。
///
/// ⚠️ **不跑 `validate()`**：校验**留在调用点**
/// （[`KernelHost::on_patch_memory_map`] 的第 ③ 步）⇒ ⇒ **既有语义一字
/// 不改**（裁决 M）⇒ ⇒ 这个函数可以被**单独调用**来测「合成逻辑」。
///
/// ⚠️ **合成顺序与语义照搬 v0.31 的循环体**（裁决 M）：
/// ① `UpsertRegion` —— 按 **`spec.base()`** 定位 ⇒ 命中**替换** / 未命中
///    **追加**（⇒ 与数组顺序无关）
/// ② `RemoveRegion` —— 定位不到 ⇒ **`E_PLT_14`**，**不静默忽略**
/// ③ **不再有 `SetBudget`**（v0.77 · STAGE-53 · 预算概念整体退役 ⇒ ⇒
///    差量只剩「改一段 / 删一段」两种）
pub(crate) fn compose_memory_map(
    base: &PlatformDescription,
    ops: &[MemoryMapOp],
) -> Result<PlatformDescription, PlatformError> {
    // ---- ① 复制 `base` 到工作副本（⇒ 原件一个字都不动）----
    let mut synthesized = base.clone();

    // ---- ② 逐条施加 `ops` ----
    for op in ops {
        match op {
            MemoryMapOp::UpsertRegion { spec } => {
                // ⚠️ 按 **`spec.base`** 定位：命中 ⇒ 替换；未命中 ⇒ 追加。
                //
                // ⚠️ **v0.73（STAGE-49 · B2 · §7.1 订正 6 ②）**：**缺
                // `base`** ⇒ ⇒ **定位不到** ⇒ ⇒ ⇒ **报既有码 `E_PLT-14`**
                // （与「删一段删不到」**同形** ⇒ ⇒ ⇒ **零新场景码**）⇒ ⇒ ⇒
                // ⚠️ **不静默**把它当成「基址 0」（那会与真的 `base = 0`
                // 混成一样 ⇒ ⇒ ⇒ 定位会**指错段**）。
                let Some(region_base) = spec.base() else {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_14_REGION_NOT_FOUND,
                        "差量要改的那一段没有基址 ⇒ 定位不到（差量要求基址存在）",
                    ));
                };
                match synthesized
                    .regions
                    .iter()
                    .position(|region| region.base() == Some(region_base))
                {
                    Some(index) => synthesized.regions[index] = spec.clone(),
                    None => synthesized.regions.push(spec.clone()),
                }
            }
            MemoryMapOp::RemoveRegion { base } => {
                // ⚠️ 定位不到 ⇒ **明确报错**，不静默忽略（裁决 N）。
                match synthesized
                    .regions
                    .iter()
                    .position(|region| region.base() == Some(*base))
                {
                    Some(index) => {
                        synthesized.regions.remove(index);
                    }
                    None => {
                        return Err(PlatformError::new(
                            PLATFORM_FAMILY,
                            E_PLT_14_REGION_NOT_FOUND,
                            format!("差量要删的那一段（基址 {base:#010x}）不在当前生效的图里"),
                        ));
                    }
                }
            }
        }
    }

    // ---- ③ 返回工作副本 ----
    Ok(synthesized)
}
