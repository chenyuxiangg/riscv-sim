//! 平台装配 / 构建族：重建平台（内存图 + 时间源 + 探针管道 + 外设探针回调）、
//! 探针管道与外设回调装配、镜像解析与符号索引重建。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。被兄弟子模块
//! （`project.rs` / `intc.rs`）回调的方法提为 `pub(crate)`；仅被父模块
//! 调度与同族内部调用的方法保持 `pub(super)`。

use super::degrade;
use super::image::ParsedImage;
use super::platform;
use super::{
    irq_device_label, KernelHost, GPIO_PERIPH_ID, GPIO_SIGNAL_LABEL, GPIO_SIGNAL_NAME,
    GPIO_VALUE_WIDTH, IRQ_CORE_SOURCE, IRQ_DEV_SOURCE, IRQ_LABEL_EXTERNAL, IRQ_LABEL_SOFTWARE,
    IRQ_LABEL_TIMER,
};
use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::csr::{IRQ_SOFTWARE, IRQ_TIMER_COMPARE, IRQ_UART_RX_READY};
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource};
use riscv_sim_contract::platform::PlatformDescription;

use crate::m06_elf::{DefaultLoader, Kind as LoadKind, ParseLimits};
use crate::m11_symbol::{Disassembler, SymbolIndex};
use crate::m18_probe::{
    ChannelDef, ChannelKind, ProbeRegistry, SampleCfg, CH_GPIO0, CH_IRQ_EXTERNAL, CH_IRQ_SOFTWARE,
    CH_IRQ_SRC_BASE, CH_IRQ_TIMER,
};
use crate::m19_waveform::{ChannelMeta, SharedPipeline, WaveformPipeline};

impl KernelHost {
    // -- 平台重建编排 -------------------------------------------------------

    /// 重建整个平台：内存图 + 探针管道 + 外设探针回调。
    ///
    /// 装载新工程时走这里——新工程 = 新会话，内存图与探针管道都要重来。
    pub(crate) fn rebuild_platform(&mut self) -> Result<(), PlatformError> {
        self.backend.reset_memory_map();
        // ⚠️ 借用不能重叠（`&mut self` + `&self.platform`）⇒ 先取一份。
        let description = self.platform.clone();
        self.map_memory(&description)?;
        // 时间驱动链路（D-07）也要重建——新外设是新实例，旧的 tick
        // 事件指着旧实例。
        self.install_peripheral_ticker();
        self.install_probe_pipeline();
        self.attach_peripheral_probes();
        Ok(())
    }

    /// 建立内存图（按**描述**）。
    ///
    /// ⚠️ **零逻辑改动**（B-6 的判别式）：这个函数里**没有**任何地址
    /// 字面量、没有任何器件构造调用、没有 `match profile` ⇒ 「换描述、
    /// 宿主零改动」在**源码层面**成立。
    ///
    /// ⚠️ 校验与建图**都**在 [`platform`] 模块里（校验器在**契约层**，
    /// 器件工厂在**宿主**）⇒ 这里只做「把描述交给它」这一件事。
    pub(super) fn map_memory(
        &mut self,
        description: &PlatformDescription,
    ) -> Result<(), PlatformError> {
        // ⚠️ 借用的作用域要收窄：`bus` 是 `self.backend` 的可变借用，
        // 而下面要把新外设的句柄写回 `self` 的字段 —— 两个借用不能重叠。
        let devices = platform::map_platform(self.backend.bus_mut(), description)?;
        // ⚠️ **v0.72（STAGE-48 · D-177 B）**：句柄**全部**进**按 id 索引**
        // 的实例表 ⇒ ⇒ 「第二个 UART」**也有句柄** ⇒ ⇒ 它的线**真的**能
        // 通电（裁决 E）。
        self.devices = devices.handles;
        // ⚠️ **角色**（STAGE-49 · D-177 的 C）：描述里声明了就用它，没声明就
        // **今天那个口径**（第一个 UART / 第一个 CLINT）⇒ ⇒ **逐字不变**。
        self.roles = devices.roles;
        // ⚠️ **逐实例**清单（STAGE-43 · v0.66 · D-175）—— **建图时**顺带
        // 产出 ⇒ ⇒ 与总线上的实际布局**同源** ⇒ ⇒ 不另算一遍。
        self.device_instances = devices.instances;
        // ⚠️ **D-151**：把**同一个** CLINT 适配成**只读**时钟端口，装进后端
        // ⇒ ⇒ guest 读 `time` 与宿主调试读 `time` **同源**（STAGE-26 验收
        // B-9）⇒ ⇒ 平台时钟的真相**只有一份**（CLINT 自己那份）。
        //
        // ⚠️ **角色 = id，句柄 = 从表里查出来的**（裁决 B）⇒ ⇒ 时间角色
        // 仍是「**第一个** CLINT」（裁决 D：逐字不变）。
        //
        // ⚠️ **没有 CLINT 的板 ⇒ 没有时钟** ⇒ ⇒ `time` / `timeh` 报 **0**
        // ⇒ ⇒ `default` / `minimal` 两档与黄金 runner 的行为**逐字不变**。
        // ⚠️ **时间源角色**（STAGE-49 · D-177 的 C）：声明了就用它，没声明
        // ⇒ ⇒ **第一个 CLINT**（今天的口径，逐字不变）。
        // ⚠️ ⚠️ **只走 `resolve_roles` 那一个落点**（v0.73 施工期订正）：这里
        //   **原来自己**又做了一遍「缺省 ⇒ 第一个 CLINT」的回落 ⇒ ⇒ ⇒
        //   「缺省行为不变」这条性质被**两道**护栏保证 ⇒ ⇒ ⇒ **结构上不可
        //   证伪** ⇒ ⇒ ⇒ 变异 T-4 **实测零红** ⇒ ⇒ ⇒ ⇒ 现在这里**不**再
        //   自己回落 ⇒ ⇒ ⇒ 「回落」这条**只有一处**（一个约束一个落点）。
        let time_source = self
            .roles
            .time_source
            .as_deref()
            .and_then(|id| self.devices.clint(id))
            .map(|clint| platform::clint_time_source(&clint));
        self.backend.install_time_source(time_source);
        self.platform = description.clone();
        // ⚠️ **v0.34（D-125）**：**连线表在建图时从生效的描述编译**（冻结
        // 规格 STAGE-19 §3.1）⇒ ⇒ 「换一块板改连线」不用改宿主 ⇒ ⇒
        // 宿主抬线时读的是**这张表**，不是源码里的一个数。
        //
        // ⚠️ **v0.72（STAGE-48 · D-177 A/B）**：**表与丢线报告一次编译**
        // ⇒ ⇒ 两者**同源** ⇒ ⇒ 报告说的**就是**表里的那些线 ⇒ ⇒ 不会
        // 出现「报告说丢了、表里还在」这种自相矛盾。
        self.peripheral_irqs = platform::compile_peripheral_irqs(description);
        Ok(())
    }

    /// 装上外设 tick 接缝，并把总线上的外设登记到引擎里（D-07）。
    ///
    /// **只有宿主知道它往总线上挂了哪些外设**——这正是这一步必须住在
    /// 宿主、而不是后端里的原因：后端看不见宿主映射的外设。
    pub(super) fn install_peripheral_ticker(&mut self) {
        self.backend.install_peripheral_ticker();
    }

    // -- 探针管道装配 -------------------------------------------------------

    /// 装上探针管道（D-46）：M-14 总线 → M-18 判沿 → M-19 环形缓冲。
    ///
    /// 装的是**四根线**：
    ///
    /// 1. M-14 的总线 hook → M-18 的访存 trace 环（M-11 导出用）；
    /// 2. M-18 → M-19 的 `EdgeSink` 握手（每条边沿同时进 M-19 的环）；
    /// 3. M-18 的通道定义（与 M-19 的通道号一一对应）；
    /// 4. M-19 的通道定义 + `SharedPipeline` 句柄。
    ///
    /// ⚠️ 必须在**总线建好之后**调用：`reset_memory_map`（装载新工程时）
    /// 会换上一条**全新的**总线，总线上挂的 hook 一并不见了——所以那里
    /// 也要重装一遍。
    pub(super) fn install_probe_pipeline(&mut self) {
        let registry = Rc::new(RefCell::new(ProbeRegistry::new()));

        // M-19 的环形缓冲：边沿的长期历史 + CSV/VCD 导出。
        //
        // 通道元数据必须与 M-18 那一侧**一致**：GPIO 的 `OUT` 信号是
        // **32 位引脚位图**（M-15 `gpio::SIGNAL_OUT` 的定义），不是
        // 单个引脚的电平。所以它是 **multi-bit / 32 位**——M-18 LLD
        // 表 68 的「valueWidth = 按端口位宽」。把它当单比特通道，
        // 32 个引脚就只剩 pin 0 有信息了（M-15 的 `gpio.rs` 头注释
        // 明确写了这条）。
        // ---- IRQ 电平通道表：核 3 条 + 每台已接线设备 1 条 ----
        //
        // ⚠️ **按平台现算**：`peripheral_irqs` 是建图时从描述编译出来的
        // ⇒ 产品视图新增器件 / 改中断号 ⇒ `rebuild_platform()` ⇒ 这里重跑
        // ⇒ 波形自动多一条；删器件同理自动消失。
        //
        // ⚠️ `ch` 只是**内部键**（0..4 已被表 68 占，且 cause 与 PLIC 源号
        // 会撞号）：核用 16/17/18，设备用 19 + 声明序号。对外身份是流 id
        // （`irq.core.software` / `irq.dev.uart0`）与 label（`uart0 · IRQ 10`）。
        let mut irq_defs: Vec<(u8, String, String, String)> = vec![
            (
                CH_IRQ_SOFTWARE,
                IRQ_CORE_SOURCE.to_string(),
                "software".to_string(),
                IRQ_LABEL_SOFTWARE.to_string(),
            ),
            (
                CH_IRQ_TIMER,
                IRQ_CORE_SOURCE.to_string(),
                "timer".to_string(),
                IRQ_LABEL_TIMER.to_string(),
            ),
            (
                CH_IRQ_EXTERNAL,
                IRQ_CORE_SOURCE.to_string(),
                "external".to_string(),
                IRQ_LABEL_EXTERNAL.to_string(),
            ),
        ];
        let mut irq_src_channels: Vec<(u32, u8)> = Vec::new();
        for (i, irq) in self.peripheral_irqs.iter().enumerate() {
            let ordinal = match u8::try_from(i) {
                Ok(v) => v,
                // 设备多到 `u8` 装不下是平台错误（不是运行时错误）⇒ 记一次
                // 桥接失败，后面的设备不再建通道（可见，不静默）。
                Err(_) => {
                    self.probe_bridge_failures += 1;
                    break;
                }
            };
            let ch = CH_IRQ_SRC_BASE.saturating_add(ordinal);
            irq_defs.push((
                ch,
                IRQ_DEV_SOURCE.to_string(),
                irq.id.clone(),
                irq_device_label(&irq.id, irq.source),
            ));
            irq_src_channels.push((u32::from(irq.source), ch));
        }

        let shared = SharedPipeline::new({
            let mut p = WaveformPipeline::new();
            p.define_channel(
                CH_GPIO0,
                ChannelMeta::multi_bit(GPIO_SIGNAL_LABEL, GPIO_VALUE_WIDTH),
            );
            // ⚠️ 中断通道一律 **single_bit**：注册表只在 0↔1 变化时发边沿
            // ⇒ 画出来就是「有中断=1 / 没有=0」的阶梯，不会像旧脉冲通道
            // 那样把**线号**当电平。
            for (ch, _source, _signal, label) in irq_defs.iter() {
                p.define_channel(*ch, ChannelMeta::single_bit(label.clone()));
            }
            p
        });

        // M-18 → M-19 握手：每条边沿同时进 M-19 的环形缓冲。
        registry
            .borrow_mut()
            .set_edge_sink(Some(Box::new(shared.clone())));

        // M-18 的通道定义（通道号与 M-19 的**必须**一致）。
        //
        // ⚠️ 失败**不能静默吞掉**：通道没接上时波形视图就是空的。失败计入
        // `probe_bridge_failures`，由 [`KernelHost::probe_bridge_failures`] 暴露。
        if registry
            .borrow_mut()
            .define_channel(ChannelDef::multi_bit(
                CH_GPIO0,
                GPIO_PERIPH_ID,
                GPIO_SIGNAL_NAME,
                GPIO_VALUE_WIDTH,
            ))
            .is_err()
        {
            self.probe_bridge_failures += 1;
        }

        for (ch, source, signal, _label) in irq_defs.iter() {
            if registry
                .borrow_mut()
                .define_channel(ChannelDef::single_bit(*ch, source.clone(), signal.clone()))
                .is_err()
            {
                self.probe_bridge_failures += 1;
            }
        }

        // 打开探针：master + 通道。
        let mut cfg = SampleCfg {
            master_enable: true,
            ..Default::default()
        };
        cfg.enable_channel(CH_GPIO0);
        // ⚠️ 全部中断通道都**开着采样**：筛选是**前端**的事 —— 后端若按
        // 勾选关采样，用户中途勾上一项就永远没有历史可看。
        for (ch, _source, _signal, _label) in irq_defs.iter() {
            cfg.enable_channel(*ch);
        }
        registry.borrow_mut().set_rate(cfg);
        self.irq_src_channels = irq_src_channels;

        // M-16 → M-18 的**电平**回调：抬线写 1、撤线写 0。
        //
        // ⚠️ 为什么不是一条「脉冲」通道：脉冲把**线号当值**画 —— 既看不出
        // 「有/无」，也表达不了「落了」。抬/撤两条回调合起来，每条线就有了
        // 自己的 0/1 边沿流；注册表按值去重 ⇒ 重复请求不产生额外边沿。
        //
        // ⚠️ 回调里**不许回头碰控制器**（`request()` / `clear()` 期间控制器
        // 是借出的）⇒ 只碰探针表，且 `try_borrow_mut` 争用时丢弃（与访存
        // hook 同一口径）。
        let rise_reg = Rc::clone(&registry);
        let rise = Box::new(move |now_ps: u64, line: u16| {
            let Some(ch) = irq_ch_for_line(line) else {
                return;
            };
            let Ok(mut reg) = rise_reg.try_borrow_mut() else {
                return;
            };
            let _ = reg.sample(now_ps, ch, 1);
        });
        match self.intc.controller.try_borrow_mut() {
            Ok(mut intc) => intc.set_pulse_sink(rise),
            Err(_) => {
                // 控制器正被占用 ⇒ 抬线回调没装上：这一轮不会上升沿。
                self.probe_bridge_failures += 1;
            }
        }

        let fall_reg = Rc::clone(&registry);
        let fall = Box::new(move |now_ps: u64, line: u16| {
            let Some(ch) = irq_ch_for_line(line) else {
                return;
            };
            let Ok(mut reg) = fall_reg.try_borrow_mut() else {
                return;
            };
            let _ = reg.sample(now_ps, ch, 0);
        });
        match self.intc.controller.try_borrow_mut() {
            Ok(mut intc) => intc.set_clear_sink(fall),
            Err(_) => {
                self.probe_bridge_failures += 1;
            }
        }

        // M-14 总线 → M-18 的访存 trace 环。
        // `make_bus_hook` 自带争用处理：注册表被占用时丢弃这条访存并
        // 计数，**不阻塞**总线派发路径。
        self.backend
            .bus_mut()
            .add_hook(ProbeRegistry::make_bus_hook(Rc::clone(&registry)));

        self.probes = Some(Rc::clone(&registry));
        self.waveform = Some(shared.clone());

        // ⚠️ v0.46 · STAGE-31（裁决 **I**）：降级执行器**随句柄一起装** ⇒ ⇒
        // 句柄换新时执行器**一起换** ⇒ ⇒ **结构上不可能**持有过期句柄。
        //
        // ⚠️ **装在这一段的末尾**：句柄（`registry` / `shared`）**已经**是
        // 最终的那一份 ⇒ ⇒ 执行器拿到的与宿主**永远是同一份**（一个真相）。
        //
        // ⚠️ 这一行是「降级**走派发路径**」的**唯一**前提：没有它 ⇒ ⇒
        // M-22 只会走自己的内置实现 ⇒ ⇒ `degrade_dispatch_last()` 恒空。
        self.hms
            .install_executor(Box::new(degrade::HostDegradeExecutor::new(
                Rc::clone(&registry),
                shared,
                Rc::clone(&self.degrade_dispatch),
            )));
    }

    /// 给已映射的外设装上探针回调（M-18 `attach_signal`）。
    ///
    /// 这一步把「外设状态变化」接到「M-18 的通道」上——波形流是从**这里**
    /// 来的，不是从总线访存来的（总线访存只进 trace 环）。
    pub(crate) fn attach_peripheral_probes(&mut self) {
        let Some(registry) = self.probes.clone() else {
            return;
        };
        for (_base, handle) in self.backend.bus().peripheral_instances() {
            // 外设正被总线派发路径占用时跳过——下一次 attach 会补上。
            //
            // ⚠️ 但「跳过」是**有代价**的：这一轮里该外设的写不会产生
            // 任何边沿，而宿主**不吭声**（V-A 检视 · 🟡-7）。失败计入
            // `probe_bridge_failures`。
            let Ok(mut periph) = handle.try_borrow_mut() else {
                self.probe_bridge_failures += 1;
                continue;
            };
            if periph.id() != GPIO_PERIPH_ID {
                continue;
            }
            if ProbeRegistry::attach_signal(
                Rc::clone(&registry),
                &mut *periph,
                GPIO_SIGNAL_NAME,
                CH_GPIO0,
                ChannelKind::MultiBit,
                GPIO_VALUE_WIDTH,
            )
            .is_err()
            {
                self.probe_bridge_failures += 1;
            }
        }
    }

    /// 采一次**设备级**中断电平（PLIC 网关电平），喂给该设备的 0/1 通道。
    ///
    /// ⚠️ **只在电平真的变了**才产生边沿（注册表按值去重）⇒ 每拍无条件调用
    /// 即可，开销只多一次比较。
    ///
    /// ⚠️ 画的是**网关电平**（设备有没有在请求 PLIC），**不是**「核有没有
    /// 看到」—— 后者是 `external · MEIP` 那条通道。PLIC 源落下**不 retract**
    /// 已送进 PLIC 的请求（Renode 口径），所以两条通道**允许**不同步。
    pub(crate) fn sample_irq_source(&self, source: u32, level: bool) {
        let Some(ch) = self
            .irq_src_channels
            .iter()
            .find(|(s, _)| *s == source)
            .map(|(_, ch)| *ch)
        else {
            return;
        };
        let Some(registry) = self.probes.as_ref() else {
            return;
        };
        let Ok(mut reg) = registry.try_borrow_mut() else {
            return;
        };
        let now_ps = self.backend.engine().now_ps();
        let _ = reg.sample(now_ps, ch, u64::from(level));
    }

    // -- 镜像解析 / 符号索引重建 --------------------------------------------

    /// **解析**一个镜像（**不写总线**），返回入口地址、段内容与调试节区。
    ///
    /// ⚠️ **为什么拆成「解析」与「写入」两步**：内存图档位要按镜像
    /// 用到的地址挑 ⇒ 必须先知道地址，才知道该建哪张图。
    ///
    /// ⚠️ 调试节区**不过契约层**（契约只传查询结果，不传节区字节）⇒
    /// 它在**内核内部**从 M-06 交给 M-11（STAGE-5 §4）。
    pub(crate) fn parse_image(&mut self, image: &ImageRef) -> Result<ParsedImage, PlatformError> {
        let bytes = match &image.source {
            ImageSource::Bytes(bytes) => bytes.clone(),
            ImageSource::Url(url) => {
                return Err(PlatformError::new(
                    ErrorFamily::Loader,
                    1,
                    format!(
                        "镜像 {} 的来源是 URL（{url}）：内核不联网，宿主必须先把字节取回来",
                        image.name
                    ),
                ));
            }
        };
        let loader = DefaultLoader;
        let limits = ParseLimits::default();
        let loaded = match image.kind {
            ImageKind::Elf => loader.load_with_limits(&bytes, LoadKind::Elf, &limits)?,
            // The contract's `ImageRef` carries no base address, so a raw
            // blob always lands at 0 — which is the reset vector.
            ImageKind::RawHex => loader.load_raw_at(&bytes, 0, &limits)?,
        };
        Ok(ParsedImage {
            entry: loaded.entry,
            segments: loaded.segments,
            debug_sections: loaded.debug_sections,
            elf_isa: loaded.elf_isa,
        })
    }

    /// 重建 M-11 的索引（装载工程时**无条件**重建，STAGE-5 §9.2）。
    ///
    /// ⚠️ **必须重建，不能缓存**：装载新工程 = 新会话（D-45 的教训：装载
    /// 后内核侧的订阅 / 索引会失效，幂等的「已就绪」检查不够）。
    ///
    /// ⚠️ **节区可能为空**（裸机镜像 / 无节区表）⇒ 那也要能构造出**空的**
    /// 符号表 / 行表：M-11 的 API 表达「空」的方式是**空表 + 降级态**，
    /// 不是错误。
    ///
    /// ⚠️ **反汇编缓存必须清空**：旧工程的机器码留在缓存里，新工程在
    /// 同一个地址上的指令会被**旧机器码**顶掉。
    ///
    /// ⚠️ **轨迹环也要清空**：轨迹是**会话级**的数据，两个会话的轨迹
    /// 混在一起就是「两个真相」。
    pub(crate) fn rebuild_symbol_index(
        &mut self,
        sections: &[(String, Vec<u8>)],
    ) -> Result<(), PlatformError> {
        // 先把索引**算出来**再落位 ⇒ 要么都换掉，要么都不换
        // （不会留下「符号是新的、行表是旧的」这种拼接体）。
        self.index = SymbolIndex::from_sections(sections)?;
        self.disassembler = Disassembler::new();
        self.trace.clear();
        Ok(())
    }
}

/// 线号 → 核中断通道。未知名（已退役的线 2）⇒ `None`。
fn irq_ch_for_line(line: u16) -> Option<u8> {
    // ⚠️ 必须用 `if` 比**值**，不能 `match` 常量名 —— 常量若不在作用域里，
    // `match` 会把它当成**绑定**（irrefutable）⇒ 第一条臂吞掉所有线号，
    // 编译器只给一句"unused variable"，波形却全画到一条通道上。
    if line == IRQ_SOFTWARE {
        Some(CH_IRQ_SOFTWARE)
    } else if line == IRQ_TIMER_COMPARE {
        Some(CH_IRQ_TIMER)
    } else if line == IRQ_UART_RX_READY {
        Some(CH_IRQ_EXTERNAL)
    } else {
        None
    }
}
