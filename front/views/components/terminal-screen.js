// front/views/components/terminal-screen.js —— 串口终端的**屏幕仿真**
// （STAGE-41 · v0.56 批 · 裁决 A ~ F）。
//
// ## 为什么需要它（用户反馈：「往串口界面输入 cyx，输出却是好几行红字」）
//
// guest 的 shell（osz 的 `SHELL_CLEAN_LINE` = `"\033[2K\r"`）是**行编辑器**：
// **每敲一个键就把整行重画一遍** —— 清行 ⇒ 回列首 ⇒ 重打提示符与缓冲区。
// ⇒ ⇒ guest 吐出来的字节流里**同一个提示符出现了 N 次**（N = 敲了几个键）。
//
// ⇒ ⇒ **真实串口终端**只显示**一行**（后一次重画**覆盖**前一次）⇒
// ⇒ ⇒ 而本平台的前端此前把字节流**原样**塞进 `<pre>` ⇒ ⇒
// ⇒ ⇒ **`\r` 被当成换行渲染** ⇒ ⇒ **`ESC[2K` 的 `[2K` 变成可见字面量**
// ⇒ ⇒ 用户看到的是「提示符 + 半截命令」**堆成好几行**的乱码。
//
// ⚠️ **字节流本身是对的**：回显**来自 guest**（平台不做本地回显，见终端视图的
//   文案）⇒ ⇒ 与 Spike 的逐字节对拍也是拿这串字节 ⇒ ⇒ **改字节流才是错的**；
//   ⇒ ⇒ **要改的是「怎么把字节流画成屏幕」** —— 也就是本模块。
//
// ## 本模块是什么
//
// 一个**极小的终端仿真器**：把 guest 的字节流解释成「屏幕上的字符」。
// 它**只认**真正会出现在串口输出里的那一小撮序列，其余**一律吞掉**
// （⛔ 绝不能把「不认识」的东西当普通字符打印出来 —— 那正是乱码的来源）。
//
// ⚠️ **纯函数、不碰 DOM**：⇒ ⇒ Node 侧零 DOM 就能测「屏幕上到底是什么」
//   ⇒ ⇒ 与 `wave-chart.js` 的纯函数口径同形。

/** ESC（转义序列的引导字节）。 */
const ESC = 0x1b;
/** BEL（响铃；OSC 的终止符之一）。 */
const BEL = 0x07;
/** BS（退格）。 */
const BS = 0x08;
/** HT（制表符）。 */
const HT = 0x09;
/** LF（换行）。 */
const LF = 0x0a;
/** CR（回车 ⇒ 列首）。 */
const CR = 0x0d;
/** DEL（无用，吞掉）。 */
const DEL = 0x7f;

/**
 * 制表符的**列宽**（HT 走到下一个 8 的倍数）。
 *
 * ⚠️ **只写一处**：本模块内部与任何测试都引用这一个常量 ⇒ ⇒ 不会出现
 *   「实现是 8、文档写 4」那种漂移。
 */
export const TAB_WIDTH = 8;

/**
 * 建一个终端屏幕。
 *
 * ⚠️ **返回的是「可增量喂字节」的对象**（不是一次性函数）⇒ ⇒ 长会话里
 *   每次只处理**新到的那些字节** ⇒ ⇒ 不用每来一个字节就重扫整条历史。
 *   （一次性版本见 [`renderTerminalScreen`]。）
 *
 * @returns {{write: (text: string) => void, reset: () => void, text: () => string, lineCount: () => number}}
 */
export function createTerminalScreen() {
    /** @type {string[][]} 每行是「一格一个字符」的数组（列 = 下标）。 */
    let lines = [[]];
    /** @type {number} 光标所在行。 */
    let row = 0;
    /** @type {number} 光标所在列。 */
    let col = 0;

    /**
     * 取当前行；**行不够就补空行**（光标往下走过头时）。
     *
     * @returns {string[]}
     */
    function currentLine() {
        while (lines.length <= row) lines.push([]);
        return lines[row];
    }

    /**
     * 在光标处写一个**可见字符**（覆盖该格）。
     *
     * ⚠️ **覆盖语义**是终端的核心：重画时新内容比旧内容短 ⇒ ⇒ 旧内容的
     *   **尾巴**必须由 guest 自己的清行序列（`ESC[2K`）负责擦掉 ⇒ ⇒
     *   本函数**不**越界擦 ⇒ ⇒ 与真实终端一致。
     *
     * @param {string} ch
     */
    function putChar(ch) {
        const line = currentLine();
        while (line.length < col) line.push(" ");
        line[col] = ch;
        col += 1;
    }

    /**
     * **擦除行内**（CSI K）。
     *
     * @param {number} mode 0 = 光标到行尾 · 1 = 行首到光标 · 2 = 整行
     */
    function eraseInLine(mode) {
        const line = currentLine();
        if (mode === 2) {
            // 整行 ⇒ 整行的格子全空。⚠️ **光标不动** ⇒ ⇒ 后续在光标处写
            // 会**补空格**再写 ⇒ ⇒ 与真实终端一致（不会把内容顶到左边）。
            line.length = 0;
            return;
        }
        if (mode === 1) {
            for (let i = 0; i <= col && i < line.length; i += 1) line[i] = " ";
            return;
        }
        // 0：光标到行尾 ⇒ ⇒ 光标**之后**的格子（含光标格）清掉。
        line.length = Math.min(line.length, col);
    }

    /**
     * **擦除显示**（CSI J）。
     *
     * ⚠️ 本模块**没有回滚缓冲**（屏幕 = 一条线性缓冲）⇒ ⇒ 「擦到屏幕末尾」
     *   就是「把光标之后的行**丢掉**」⇒ ⇒ 与线性模型自洽。
     *
     * @param {number} mode 0 = 光标到末尾 · 1 = 开头到光标 · 2 = 整个屏幕
     */
    function eraseInDisplay(mode) {
        if (mode === 2) {
            for (let i = 0; i < lines.length; i += 1) lines[i] = [];
            lines.length = row + 1;
            return;
        }
        if (mode === 1) {
            for (let i = 0; i < row && i < lines.length; i += 1) lines[i] = [];
            const line = currentLine();
            for (let i = 0; i <= col && i < line.length; i += 1) line[i] = " ";
            return;
        }
        // 0：光标到屏幕末尾。
        const line = currentLine();
        line.length = Math.min(line.length, col);
        lines.length = row + 1;
    }

    /**
     * 解析并执行一条 **CSI** 序列（`ESC [ ... final`）。
     *
     * @param {string} final 结束字节（`@` ~ `~`）
     * @param {string} params 参数串（可能带 `?` 之类的私有前缀）
     */
    function applyCsi(final, params) {
        // ⚠️ **私有前缀**（`?` `>` `<` `=`）只对个别序列有意义 ⇒ ⇒ 本模块
        //   **不实现**它们 ⇒ ⇒ 解析数字时**跳过** ⇒ ⇒ 免得把 `?25` 当成 `25`。
        const numeric = params.replace(/^[^0-9;]*/, "");
        const nums = numeric.split(";").map((p) => (p === "" ? 0 : Number(p) || 0));
        const p0 = nums.length > 0 ? nums[0] : 0;
        const p1 = nums.length > 1 ? nums[1] : 0;

        switch (final) {
            case "K": // EL —— 擦除行内
                eraseInLine(p0 === 1 ? 1 : p0 === 2 ? 2 : 0);
                return;
            case "J": // ED —— 擦除显示
                eraseInDisplay(p0 === 1 ? 1 : p0 === 2 ? 2 : 0);
                return;
            case "A": // CUU —— 光标上移
                row = Math.max(0, row - (p0 || 1));
                return;
            case "B": // CUD —— 光标下移
                row += p0 || 1;
                currentLine();
                return;
            case "C": // CUF —— 光标右移
                col += p0 || 1;
                return;
            case "D": // CUB —— 光标左移
                col = Math.max(0, col - (p0 || 1));
                return;
            case "G": // CHA —— 光标移到列
                col = Math.max(0, (p0 || 1) - 1);
                return;
            case "d": // VPA —— 光标移到行
                row = Math.max(0, (p0 || 1) - 1);
                currentLine();
                return;
            case "H": // CUP —— 光标定位
            case "f":
                row = Math.max(0, (p0 || 1) - 1);
                col = Math.max(0, (p1 || 1) - 1);
                currentLine();
                return;
            case "m": // SGR —— 颜色/属性：本模块**不画颜色** ⇒ ⇒ 吞掉
            default:
                // ⚠️ **不认识的一律吞掉** ⇒ ⇒ 绝不能让它变成屏幕上的字面量。
                return;
        }
    }

    /**
     * 吞掉一条 **OSC** 序列（`ESC ] ... BEL` 或 `ESC ] ... ESC \`）。
     *
     * @param {string} text 整条流
     * @param {number} start OSC 内容的第一位
     * @returns {number} 被消费到的最后一位下标
     */
    function consumeOsc(text, start) {
        let j = start;
        while (j < text.length) {
            const code = text.charCodeAt(j);
            if (code === BEL) return j;
            if (code === ESC && text.charCodeAt(j + 1) === 0x5c) return j + 1;
            j += 1;
        }
        return text.length - 1;
    }

    /**
     * 吞掉一条 **CSI** 序列（`ESC [ ... final`）。
     *
     * @param {string} text 整条流
     * @param {number} start 参数的第一位
     * @returns {number} 被消费到的最后一位下标
     */
    function consumeCsi(text, start) {
        let j = start;
        let params = "";
        while (j < text.length) {
            const code = text.charCodeAt(j);
            // 参数字节（`0-9 : ; < = > ?`）。
            if (code >= 0x30 && code <= 0x3f) {
                params += text[j];
                j += 1;
                continue;
            }
            // 中间字节（` ` ~ `/`）。
            if (code >= 0x20 && code <= 0x2f) {
                j += 1;
                continue;
            }
            break; // 结束字节
        }
        if (j >= text.length) return text.length - 1; // 没写完 ⇒ ⇒ 吞掉剩下的
        applyCsi(text[j], params);
        return j;
    }

    /**
     * 吞掉一条 **ESC 序列**（非 CSI / 非 OSC 的那些）。
     *
     * @param {string} text 整条流
     * @param {number} i ESC 的下标
     * @returns {number} 被消费到的最后一位下标
     */
    function consumeEscape(text, i) {
        const next = text.charCodeAt(i + 1);
        if (Number.isNaN(next)) return i; // 只有一个 ESC ⇒ ⇒ 吞掉
        if (next === 0x5b) return consumeCsi(text, i + 2); // ESC [
        if (next === 0x5d) return consumeOsc(text, i + 2); // ESC ]
        // ⚠️ **字符串型序列**（DCS `ESC P` / SOS `ESC X` / PM `ESC ^` /
        //   APC `ESC _`）⇒ ⇒ 它们有**载荷**⇒ ⇒ 一直吞到 ST（`ESC \`）
        //   或 BEL ⇒ ⇒ 载荷一个字都不许漏到屏幕上。
        if (next === 0x50 || next === 0x58 || next === 0x5e || next === 0x5f) {
            return consumeOsc(text, i + 2);
        }
        // 其余：`ESC` + 中间字节 + 结束字节 ⇒ ⇒ 整条吞掉。
        let j = i + 1;
        while (j < text.length) {
            const code = text.charCodeAt(j);
            if (code >= 0x20 && code <= 0x2f) {
                j += 1;
                continue;
            }
            break;
        }
        if (j < text.length) j += 1; // 结束字节
        return j - 1;
    }

    return {
        /**
         * 喂**新到的字节**（按码位一对一，⛔ 不做 UTF-8 解码 —— 字节流才是真相）。
         *
         * @param {string} text
         */
        write(text) {
            const src = String(text ?? "");
            for (let i = 0; i < src.length; i += 1) {
                const code = src.charCodeAt(i);
                if (code === ESC) {
                    i = consumeEscape(src, i);
                    continue;
                }
                if (code === LF) {
                    // ⚠️ **LF 顺带 CR**（裁决 B）：真实串口上 host 端的 tty 默认
                    //   **ONLCR** ⇒ ⇒ guest 那个光秃秃的 `\n` 到屏幕上是
                    //   **回车 + 换行** ⇒ ⇒ 否则 guest 打印的下一行会**缩进**
                    //   「上一行的长度」⇒ ⇒ 没有任何真实终端是那个样子。
                    row += 1;
                    col = 0;
                    currentLine();
                    continue;
                }
                if (code === CR) {
                    col = 0;
                    continue;
                }
                if (code === BS) {
                    col = Math.max(0, col - 1);
                    continue;
                }
                if (code === HT) {
                    col = (Math.floor(col / TAB_WIDTH) + 1) * TAB_WIDTH;
                    continue;
                }
                if (code < 0x20 || code === DEL) {
                    // 其它控制字符（NUL / BEL / SOH …）⇒ ⇒ **吞掉**。
                    continue;
                }
                putChar(src[i]);
            }
        },

        /** 清空整块屏幕（换工程 / 重置会话时用）。 */
        reset() {
            lines = [[]];
            row = 0;
            col = 0;
        },

        /**
         * 屏幕上的**文本**（行用 `\n` 分隔）。
         *
         * ⚠️ **行尾空格被去掉**（裁决 C）：屏幕上的空格格是**空白** ⇒ ⇒
         *   文本里留着它只会让「这行到底有没有东西」变得看不清。
         *
         * @returns {string}
         */
        text() {
            return lines
                .map((line) => line.join("").replace(/[ \t]+$/, ""))
                .join("\n");
        },

        /** 屏幕上的**行数**（给「显示成几行」这类判据用）。 */
        lineCount() {
            return lines.length;
        },
    };
}

/**
 * 把**整条**字节流渲染成屏幕文本（一次性）。
 *
 * ⚠️ **这是「从零重算」的那条路** ⇒ ⇒ 它与「增量喂字节」那条路
 *   **必须给出同一个答案**（判据 T-11 钉住这一点）⇒ ⇒ 否则会出现
 *   「重渲染一次界面就变了」的诡异现象。
 *
 * @param {unknown} raw 原始字节流（字符串）
 * @returns {string}
 */
export function renderTerminalScreen(raw) {
    const screen = createTerminalScreen();
    screen.write(typeof raw === "string" ? raw : String(raw ?? ""));
    return screen.text();
}
