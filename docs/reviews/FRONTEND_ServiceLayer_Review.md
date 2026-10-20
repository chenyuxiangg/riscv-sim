# 前端服务层 Review（M-04 / M-05 / M-07 / M-10）

- **分支**：`feature/frontend-service-layer`（从 `main@86940ac` 拉出）
- **范围**：把前端从「视图层已就绪、服务层全是空壳」推进到
  「传输 / 命令路由 / 会话 / 寄存器内存四个服务真正跑通一条链路」
- **不在本轮范围**：真实 WASM 内核接入（Wave 2）、`crates/**` 任何改动、
  M-14 内存映射的运行时数据、M-08/M-09/M-11 等调试服务
- **复验方式**：`tools/check-front.sh`（Node 门禁）+ Chrome 真机（CDP 驱动）

---

## 一、结论

四个服务从 66 行空壳变成一条可跑通的链路：**视图动作 → M-05 命令路由 →
M-07 传输 → mock 内核 → 事件回流 → M-10 缓存 → 视图 store → 视图重渲染**。
门禁用例 38 → 109（7 → 19 suite），全绿；Chrome 真机复验通过。

链路里跑的不是「假数据」：每一帧都携带冻结契约
`riscv_sim_contract::protocol::Message` 的 serde JSON 形态，mock 内核按
契约语义应答（未注册命令回 `E_CTX-05`、未映射地址回 `E_BUS-01`、命令
超时回 `E_CTX-06`）。

---

## 二、模块逐项

### M-07 传输适配器（`front/services/m07_transport.js`）

| 项 | 实现 |
| --- | --- |
| 适配器 | `worker`（postMessage，浏览器）/ `websocket`（P1 骨架） |
| 状态 | `{mode, connected, connState, lastError, sessionId, role, seq, label}` |
| 状态分发 | `onStatus(handler)` 订阅式；`getTransportStatus()` 取当前快照 |
| 挂载语义 | **不抛异常**：缺 `Worker` / 缺 URL / `file://` 源 → 明确的「未连接」态 + `lastError` |
| 帧 | `{type, seq, sessionId, role, reqId, payload}`（`front/lib/frame.js`） |

`connState` 用 M-07 LLD §2.1.1 的四态（`connecting/ready/closed/reconnecting`）。

**WebSocket 是骨架，不是半成品**：`mountTransport({mode:"websocket"})` 不带
URL 时落到 `E_TRX-03`（服务端形态为 P1）的「未连接」态；带 URL 时由
`onopen/onclose/onerror` 驱动状态，**握手完成前不算 connected**。

### M-05 命令路由与事件分发（`front/services/m05_commandrouter.js`）

- 出站：`dispatchCommand(payload)` → 契约信封 → `transport.sendCommand(msg, {reqId})`；
  默认 500ms 超时（超时回 `E_CTX-06`，读类/幂等命令可由调用方重试）。
- 入站：transport 帧 → 解包契约信封 → 按 topic 分发。
- 订阅三件套：`subscribe(topic, fn)` / `unsubscribe(topic, fn)` / `once(topic)`，
  另有 `"*"` 通配 topic。
- 关联：以帧上的 `reqId` 配对（乱序响应也能各归各位）；内核回 `Error`
  事件时命令被**拒绝**且错误码透传。
- 健壮性：订阅者抛错不打断其它订阅者；链路断开时在途命令全部失败。

### M-04 会话管理器（`front/services/m04_session.js`）

状态机（非法迁移回 `E_CTX-01`，状态不被改坏）：

```
idle ──create──▶ creating ──link up──▶ ready
                               │          │
                               │          ├─link down──▶ reconnecting
                               │          │                    │
                               │          │◀───link up──────────┘
                               ▼          ▼
                            closing ──▶ closed
```

- 会话 id：默认生成（`sess-<n>-<hex6>`），也可由调用方指定（服务端下发）。
- 绑定：会话 id 经 M-07 落到每一帧的 `sessionId` 上 —— FR-13 的会话隔离
  前提，也是快照命名空间的前提。
- 断线重连：链路断开 → `reconnecting`（**会话 id 保留**，不重新 mint）；
  链路恢复 → `ready` 并重新绑定。
- 关闭不存在的会话回 `E_CTX-11`（M-04 LLD §6.1 一场景一码）。

### M-10 寄存器内存服务（`front/services/m10_registermemory.js`）

- 缓存内容：32 个 GPR、PC、虚拟时间、已读回的内存单元。
- 数据来源：寄存器/PC 走 `waveform.delta`（边沿流），虚拟时间走 `tick`，
  内存走 `memory.read`。
- 请求：读寄存器 = `Subscribe("sim.registers")`（契约没有「读寄存器」命令，
  见第五节）；读内存 = `ReadMemory`；写内存 = `WriteMemory` + **读回确认**
  （前端不猜内核状态）。
- 落视图：合并语义写进 M-03 store（不整块替换 `sim`，反汇编区不会被抹掉），
  ≤200ms 节流触发 `riscv:render`；提供 `flushRender()` 供门禁同步断言。

---

## 三、契约对齐：JS 镜像怎么保证不漂

新增 `front/lib/message.js` —— 冻结契约的 JS 镜像，**唯一**知道线上格式
的地方（信封字段名、serde 的 externally-tagged 表示、全部变体与枚举标签）。

关键点：门禁**直接解析 Rust 源码**比对，而不是抄一份期望值：

```js
const rust = rustVariants(readFileSync("crates/contract/src/protocol.rs"), "CommandPayload");
assert.deepEqual(jsVariants, rust);
```

覆盖面：`CommandPayload`(16) / `EventPayload`(8) / `Direction` /
`WriteSize` / `HaltReason` / `WatchAccess`。**任何一侧单边改名都会撞红**。

---

## 四、这一轮自己踩到并修掉的缺陷

| # | 缺陷 | 发现方式 | 修法 | 回归 |
| --- | --- | --- | --- | --- |
| D-1 | WebSocket 适配器在**握手前**就报 connected | 门禁（transport.test.mjs） | 适配器声明 `ready` 时机，握手前停在 `connecting` | 门禁 |
| D-2 | `bindSession` 同步触发状态回调 → 会话 `ready → ready` 非法迁移，**整个会话握手中止**（内核未就绪、产品树不加载） | **Chrome 真机** | 迁移前先判状态；绑定后不再重复迁移 | session.test.mjs |
| D-3 | 重连时先 `bindSession` 后迁移 → 状态回调**无限递归**（Maximum call stack） | 门禁（fake transport 改为忠实通知） | 先迁移再绑定 | session.test.mjs |

D-2 / D-3 说明了一件事：**DOM stub 覆盖不到挂载时序**（AGENTS.md 已经写过
这条），服务层的「状态回调 + 通知」组合同样只有真机能暴露。

---

## 五、契约缺口（需要协调者决策，本轮未擅自改契约）

冻结契约与 LLD 之间有几处对不上。前端按「契约优先」处理，但这些缺口会
在 Wave 2 真实 WASM 接入时变成硬伤：

1. **没有「寄存器快照」事件变体，也没有「读寄存器」命令。**
   `EventPayload` 只有 `Tick{elapsed_ps, pc}` —— 连 GPR 都带不出来；
   `CommandPayload` 里没有 `ReadRegisters`。
   *本轮做法*：寄存器值以边沿流（`WaveformDelta` + `cpu.gpr.xN` /
   `cpu.pc` 通道）承载 —— 这是契约里唯一契约原生的逐信号载体。
   *建议*：契约升到 `schema_version = 2` 时补 `ReadRegisters` /
   `RegistersUpdated` 变体，M-10 改回直读。

2. **M-05 LLD 与 M-10 LLD 反复提到 `simStateDelta` 事件，冻结契约里没有这个
   变体**（LLD 的 `publish("simStateDelta", …)` 无对应枚举项）。
   本轮用 `WaveformDelta` 顶替，语义上等价但名字对不上，后续排查会误导。

3. **没有「写寄存器」命令。** `WriteMemory` 写的是内存，不是寄存器文件。
   M-10 因此**不提供** `writeRegister` —— 与 M-10 LLD §3.1 的
   `writeReg(name, value)` 端口签名有缺口（该端口在 Rust 侧是 M-10 直接
   调 M-17，不经协议面；前端只能经协议面）。

4. **没有 `negotiate` 命令。** M-05 LLD §3.3 的能力协商在前端**无法实现**
   —— 工作台仍用 M-01 的静态角色能力位。真实内核接入后，能力降级
   （QEMU 只支持 L1）这条链路目前没有协议面。

5. **`u64` 精度。** 契约的 `seq` / `elapsed_ps` / `t_ps` / `t_virt` 都是
   `u64`，serde 序列化成 JSON number，JS 侧是 `Number`。
   `2^53 ps ≈ 104 天`虚拟时间——MVP 不会到，但真实内核长跑会溢出。
   Wave 2 需要定：改 `u53`/字符串编码，或前端用 `BigInt` 解析。

6. **错误码族名。** LLD 用 `E_PROTO-0x`（协议层），冻结契约的
   `ErrorFamily` 里没有 `PROTO`，只有 `TRX`（传输）与 `CTX`（上下文）。
   本轮沿用 LLD 的 `E_PROTO-01/08` 字样，**没有**自造新族。
   建议在契约里补 `Protocol` 族，或统一改用 `E_TRX-0x`。

7. **M-07 LLD 的 `crc` 字段未实现**（P1，契约里也没有对应错误码位）。

---

## 六、验证证据

### 门禁（`tools/check-front.sh`，Node v22.15.0）

| | 基线 | 本轮 |
| --- | --- | --- |
| 用例 | 38 | **109** |
| suite | 7 | **19** |
| 失败 | 0 | **0** |

新增覆盖：契约镜像 ↔ Rust 源码比对（15）、M-07 双适配器（12）、M-05 路由
（14）、M-04 会话（11）、M-10 regmem（8）、mock 内核 + 真实 worker 文件
端到端（11）。

### Rust 门禁（本轮未改 `crates/**`，基线保持）

- `cargo fmt --all -- --check` → 通过
- `cargo clippy --workspace --all-targets -- -D warnings` → 通过
- `cargo test --workspace` → **203 passed / 0 failed**（193 + 5 + 5）

### Chrome 真机复验（AGENTS.md 强制项）

`python3 -m http.server 8080 --directory front` + Chrome 155 headless
（CDP 驱动，真实 ES module + 真实 module Worker + 真实 postMessage）：

| 检查项 | 结果 |
| --- | --- |
| 状态栏 | 「kernel 已连接 — mock worker」（此前是「未连接」） |
| 内核探活 | 页脚「mock 内核已就绪（Nop 往返成功）」 |
| 单步 | PC `0x0` → `0x4`，x1 = `0x0000002a`（42） |
| 单步 ×10 | PC → `0x40`（命中 mock 断点） |
| 继续 | PC = `0x40`，x1/x2/x3 = 42 / 7 / 49 |
| 读内存 | `0x00000100` = `0x00000031`（49，即 `sw x3, 0x100(x0)`） |
| 寄存器/内存视图 | 32 个寄存器格 + 内存单元表 |
| 产品树 | 显示已加载的 mock 项目（MainBoard → RV32IMC → UART0/GPIO0） |
| 控制台 | 无 JS 异常（仅浏览器默认的 `favicon.ico` 404） |

复验脚本已进仓：**`tools/browser-verify.mjs`**（零依赖，Node 22 的全局
`WebSocket` + `fetch` 驱动 Chrome DevTools Protocol，不引入任何 npm 包）。
它**不是**门禁的一部分（`tools/check-front.sh` 不调用它）——它依赖本机
Chrome，只能按需手动跑。

复现步骤（仓库根目录）：

```bash
# 1) 起同源 HTTP 服务（ES module 与 Worker 都要求 HTTP，file:// 会被拦）
python3 -m http.server 8080 --directory front

# 2) 跑真机复验脚本（自己会起 Chrome，跑完自动清理）
node tools/browser-verify.mjs --no-serve

# 3) 看退出码：0 = 通过，1 = 有断言失败 / 控制台错误，127 = 缺前置条件
echo $?
```

不想自己起服务时，直接 `node tools/browser-verify.mjs`——脚本发现端口没人
应答就会自己拉起 `python3 -m http.server`。浏览器路径默认按
`CHROME_PATH` → macOS `Google Chrome.app` → `Chromium.app` → PATH 上的
`google-chrome` / `chromium` 依次尝试；都找不到时打印提示并以 **127**
退出（那是「本机缺前置条件」，不是「前端有问题」）。

脚本验证什么：真实 Chrome 里的**落地页装配**（工作台容器有子节点、当前
视图有标题文本）、**渲染路径**（切到 `#/regmem` 后视图正文非空、32 个
寄存器格渲染出来——这正是 DOM stub 盖不住的「首次事件早于订阅」那类
白屏缺陷）、**服务链**（传输层「已连接」+ 页脚「mock 内核已就绪」）、
**控制台干净**（无 `console.error`、无未捕获异常；浏览器默认的
`favicon.ico` 404 被显式忽略）。

脚本**不**验证什么：它不是 `tools/check-front.sh` 的替代品（契约镜像、
模块行为仍由 Node 门禁覆盖），它不接真实 WASM 内核（前端仍连 dev-only
的 mock worker），也不覆盖多视图交互与仿真结果本身。

> 落地视图 `embedded` 目前**没有** M-02 组件（MVP 占位面板），所以它的
> 正文本来就是空的；脚本因此把「正文非空」的检查放在 `#/regmem` 上。
> 这条已知缺口由 `tools/browser-verify.mjs` 的落地页/渲染路径两段检查
> 显式区分，不会被误判成白屏回归。

---

## 七、遗留与欠账

1. **真实 WASM 接入（Wave 2）** — 本轮**没有**碰 `crates/**`。
   `tools/build-wasm.sh` 现在仍然跑不出可加载模块：`crates/kernel` 缺
   `wasm-bindgen` 依赖、缺 `#[wasm_bindgen]` 导出、缺 cdylib 入口。
   接入时需要：
   - kernel 侧加导出层（`init` + `dispatch(Message) -> Message`），
     并把 `crates/contract` 的 DTO 映射到 wasm-bindgen 可导出的形态；
   - 前端把 `mountTransport` 的 worker URL 从 `./dev/mock-kernel-worker.js`
     换成真实 WASM worker，`front/lib/message.js` **不用改**（契约没变）；
   - 真实内核的 `Tick`/寄存器推送形态要落进冻结契约的既有变体里 ——
     这正是第五节第 1 条的缺口。

2. **M-14 内存映射仍是 demo seed。** 契约里没有「内存映射」事件，产品视图
   的映射表仍靠 `window.riscv_demo.seedBus()`（dev only）。真实内核接入后
   需要一条契约通道（`Project.images` + 装载事件？）。

3. **M-11 反汇编区仍是空的**（`disasm` 恒为空数组）。契约里没有反汇编
   事件变体，同属第五节的契约缺口。

4. **波形视图只显示探针通道**（`probe.*`）。寄存器边沿流不混进去（否则
   波形区会被 32 条寄存器流淹没）。

5. **`front/dev/demo-state.js` 与 mock 项目并存**：demo seed 会把产品树的
   树覆盖回 demo 树。是 dev-only 行为，但两个数据源容易让人误解。

6. **门禁的 DOM stub 仍是 stub**：本轮的 D-2 / D-3 就是它盖不住的
   「挂载时序 / 状态回调时序」类缺陷。真机冒烟脚本已进仓
   （`tools/browser-verify.mjs`，见第六节），可以按需手动跑；它依赖
   本机 Chrome，所以还进不了 `tools/check-front.sh` 的默认门禁。
   后续若要常态化，需要一个带 Chrome 的 CI runner。

7. **落地视图 `embedded` 没有 M-02 组件**（MVP 占位面板，正文为空）：
   首次进入工作台看到的是「嵌入式工作台」标题 + 空面板，要切到
   `#/debug` / `#/regmem` 才有内容。属于 MVP 欠账，不是本轮引入的
   回归——`tools/browser-verify.mjs` 的落地页与渲染路径两段检查把
   两者区分开，不会误报。

---

## 八、提交序列

| commit | 说明 |
| --- | --- |
| `3c3dc9a` | `feat(m07)`: 传输适配器骨架 + Message 信封 JS 形态 |
| `0721bad` | `feat(dev)`: mock kernel worker（纯 JS） |
| `7ae52ff` | `feat(m05)`: 命令路由与事件分发 |
| `728fc8d` | `feat(m04)`: 会话管理器 |
| `9862ece` | `feat(m10)`: 寄存器内存服务 |
| `67982a2` | `feat(front)`: 工作台接线到 mock 内核 |
| `e5c33a7` | `test(front)`: 前端门禁扩展（38 → 109） |

---

## 九、边界声明

- 本轮**只**动了 `front/**` 与本文件（`docs/reviews/FRONTEND_ServiceLayer_Review.md`）。
- `crates/**`、`tools/check-front.sh`、`docs/reviews/CROSS_MODULE_DEBT.md`、
  `docs/progress/PROGRESS.md`、`docs/reviews/FRONTEND_Review.md`、
  `AGENTS.md` / `README.md` / `CONTRIBUTING.md` / `.github/**` **未改**。
- 未推送任何分支（`git push` 需用户明确指示）。
