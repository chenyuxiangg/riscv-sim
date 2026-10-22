#!/usr/bin/env node
// tools/elf-symtab.mjs —— 零依赖的 **ELF32** `.symtab` 解析器。
//
// # 为什么要有它
//
// 真机段（`tools/browser-verify.mjs` 的 E-7）要在 osz 的**调度器符号**上
// 下断点 ⇒ 必须**按符号名**取地址。
//
// ⚠️ **不许写死地址**：写死就成了「恒绿」的断言——换个 osz 构建就判错，
// 而且它证明不了「平台真的按名字找对了符号」。
//
// ⚠️ **不许依赖 `nm`**：系统上的 `nm` 不保证存在、不保证支持 RISC-V
// （macOS 的 `/usr/bin/nm` 是 LLVM 的，**实测**能读 RISC-V ELF，但那是
// 「碰巧」，不是保证）⇒ 自己解析 ELF，**零依赖**。
//
// # 解析的东西
//
// 只解析**要用的**那几样：
//
//   1. **ELF 头**（`Elf32_Ehdr`，52 字节）⇒ `e_entry`（入口地址）、
//      `e_shoff` / `e_shentsize` / `e_shnum` / `e_shstrndx`（节区表）；
//   2. **节区头表**（`Elf32_Shdr`，每项 40 字节）⇒ 找出 `.symtab`
//      （`SHT_SYMTAB` = 2）与它指向的**字符串表**（`sh_link` ⇒ `.strtab`）；
//   3. **符号表**（`Elf32_Sym`，每项 16 字节）⇒ 名字（`st_name` 指向
//      `.strtab` 的偏移）、地址（`st_value`）、大小、类型与绑定。
//
// # 不解析的东西
//
// **不**解析重定位、**不**解析 DWARF、**不**校验校验和 ⇒ 这个工具只
// 回答一个问题：「**这个符号名**在哪个地址？」。
//
// # 用法
//
// ```js
// import { loadElf, findFirstSymbol } from "./elf-symtab.mjs";
// const elf = loadElf("/tmp/osz-artifacts/osz.elf");
// const hit = findFirstSymbol(elf, ["first_schedule", "schedule"]);
// console.log(hit.name, "0x" + hit.value.toString(16));
// ```

import { readFileSync } from "node:fs";

/** `EI_CLASS` —— 1 = 32 位。 */
const ELFCLASS32 = 1;
/** `EI_DATA` —— 1 = 小端（LSB）。 */
const ELFDATA2LSB = 1;

/** `SHT_SYMTAB` —— 符号表。 */
const SHT_SYMTAB = 2;

/** `STT_FUNC` —— 函数符号（`st_info` 的低 4 位）。 */
const STT_FUNC = 2;

/** `SHN_UNDEF` —— 未定义符号（没有地址）。 */
const SHN_UNDEF = 0;

/** `Elf32_Ehdr` 的大小。 */
const EHDR_SIZE = 52;
/** `Elf32_Shdr` 的大小。 */
const SHDR_SIZE = 40;
/** `Elf32_Sym` 的大小。 */
const SYM_SIZE = 16;

/**
 * 读一个 **u16**（小端）。
 *
 * @param {Buffer} buf
 * @param {number} off
 * @returns {number}
 */
function u16(buf, off) {
    return buf.readUInt16LE(off);
}

/**
 * 读一个 **u32**（小端）。
 *
 * @param {Buffer} buf
 * @param {number} off
 * @returns {number}
 */
function u32(buf, off) {
    return buf.readUInt32LE(off);
}

/**
 * 读一个 **C 字符串**（到 `\0` 为止）。
 *
 * @param {Buffer} buf
 * @param {number} off
 * @returns {string}
 */
function cstr(buf, off) {
    if (off < 0 || off >= buf.length) return "";
    const end = buf.indexOf(0, off);
    return buf.toString("utf8", off, end === -1 ? buf.length : end);
}

/**
 * 解析一份 **ELF32** 镜像 ⇒ 入口地址 + 节区表 + 符号表。
 *
 * @param {Buffer|Uint8Array} bytes — ELF 文件的字节
 * @returns {{class: number, entry: number, sections: object[], symbols: object[]}}
 */
export function parseElf(bytes) {
    const buf = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes);

    // -- ELF 头 ------------------------------------------------------------
    if (buf.length < EHDR_SIZE) {
        throw new Error(`ELF 太短（${buf.length} 字节），读不出 ELF 头`);
    }
    if (!(buf[0] === 0x7f && buf[1] === 0x45 && buf[2] === 0x4c && buf[3] === 0x46)) {
        throw new Error("不是 ELF 文件（魔数不对）");
    }
    const elfClass = buf[4];
    if (elfClass !== ELFCLASS32) {
        throw new Error(
            `只支持 ELF32（EI_CLASS = 1），这份镜像是 EI_CLASS = ${elfClass}`
            + "（64 位镜像不在本工具的射程内）",
        );
    }
    if (buf[5] !== ELFDATA2LSB) {
        throw new Error(`只支持小端 ELF（EI_DATA = 1），这份镜像是 EI_DATA = ${buf[5]}`);
    }
    const entry = u32(buf, 24);
    const shoff = u32(buf, 32);
    const shentsize = u16(buf, 46);
    const shnum = u16(buf, 48);
    const shstrndx = u16(buf, 50);

    // -- 节区头表 ----------------------------------------------------------
    /** @type {object[]} */
    const sections = [];
    for (let i = 0; i < shnum; i += 1) {
        const base = shoff + i * shentsize;
        if (base + SHDR_SIZE > buf.length) {
            throw new Error(`节区头表越界（节区 ${i}）`);
        }
        sections.push({
            index: i,
            nameOffset: u32(buf, base),
            type: u32(buf, base + 4),
            addr: u32(buf, base + 12),
            offset: u32(buf, base + 16),
            size: u32(buf, base + 20),
            link: u32(buf, base + 24),
            info: u32(buf, base + 28),
            entsize: u32(buf, base + 36),
        });
    }

    // -- 节区名（`.shstrtab`）------------------------------------------------
    const shstr = sections[shstrndx];
    const nameOf = (section) => {
        if (!shstr) return "";
        const at = shstr.offset + section.nameOffset;
        if (at >= buf.length) return "";
        return cstr(buf, at);
    };
    for (const section of sections) section.name = nameOf(section);

    // -- 符号表（`.symtab`）-------------------------------------------------
    /** @type {object[]} */
    const symbols = [];
    const symtab = sections.find((s) => s.type === SHT_SYMTAB);
    if (symtab) {
        const strtab = sections[symtab.link];
        const count = Math.floor(symtab.size / SYM_SIZE);
        for (let i = 0; i < count; i += 1) {
            const base = symtab.offset + i * SYM_SIZE;
            if (base + SYM_SIZE > buf.length) break;
            const nameOffset = u32(buf, base);
            const info = buf[base + 12];
            symbols.push({
                name: strtab ? cstr(buf, strtab.offset + nameOffset) : "",
                value: u32(buf, base + 4),
                size: u32(buf, base + 8),
                type: info & 0x0f,
                bind: info >> 4,
                shndx: u16(buf, base + 14),
            });
        }
    }

    return { class: elfClass, entry, sections, symbols };
}

/**
 * 从**文件**读一份 ELF32 镜像并解析。
 *
 * @param {string} path
 * @returns {{class: number, entry: number, sections: object[], symbols: object[]}}
 */
export function loadElf(path) {
    return parseElf(readFileSync(path));
}

/**
 * 按**名字**找一个符号。
 *
 * ⚠️ **只认「有定义」的符号**（`shndx != SHN_UNDEF`）——未定义符号没有
 * 地址（它的 `st_value` 是 0）⇒ 拿它下断点会断在地址 0。
 *
 * @param {{symbols: object[]}} elf
 * @param {string} name
 * @returns {object|null}
 */
export function findSymbolByName(elf, name) {
    const hit = (elf?.symbols ?? []).find(
        (s) => s.name === name && s.shndx !== SHN_UNDEF,
    );
    return hit ?? null;
}

/**
 * 在**候选名字**里按顺序找**第一个**存在的符号。
 *
 * ⚠️ **不要把某一个名字写死成唯一答案**（那会变成「恒绿」的断言）：
 * 候选是「这一类角色」的若干可能名字（调度器可能是 `first_schedule` /
 * `schedule` / …）⇒ **按名字去符号表里查**，查到第一个就用它。
 *
 * ⚠️ **只优先函数符号**（`STT_FUNC`）：调度器是个函数；若同名的是个
 * 对象符号（`STT_OBJECT`），地址虽然也对，但优先函数更贴近意图。
 *
 * @param {{symbols: object[]}} elf
 * @param {string[]} names — 候选名字（**按优先级**排）
 * @returns {{name: string, value: number, symbol: object}|null}
 */
export function findFirstSymbol(elf, names) {
    for (const name of names ?? []) {
        const candidates = (elf?.symbols ?? []).filter(
            (s) => s.name === name && s.shndx !== SHN_UNDEF,
        );
        if (candidates.length === 0) continue;
        const fn = candidates.find((s) => s.type === STT_FUNC);
        const symbol = fn ?? candidates[0];
        return { name: symbol.name, value: symbol.value, symbol };
    }
    return null;
}

// ---------------------------------------------------------------------------
// 命令行自测（`node tools/elf-symtab.mjs <elf> [符号名…]`）
// ---------------------------------------------------------------------------

// ⚠️ **只有被直接当脚本跑时**才走这条（`import` 时不做任何事）。
if (process.argv[1] && process.argv[1].endsWith("elf-symtab.mjs")) {
    const [, , elfPath, ...names] = process.argv;
    if (!elfPath) {
        console.error("用法：node tools/elf-symtab.mjs <elf> [符号名…]");
        process.exit(2);
    }
    try {
        const elf = loadElf(elfPath);
        console.log(`entry = 0x${elf.entry.toString(16)}`);
        console.log(`节区 ${elf.sections.length} 个 · 符号 ${elf.symbols.length} 个`);
        if (names.length > 0) {
            const hit = findFirstSymbol(elf, names);
            if (!hit) {
                console.error(`这些候选一个都没找到：${names.join(" / ")}`);
                process.exit(1);
            }
            console.log(`${hit.name} = 0x${hit.value.toString(16)}`);
        } else {
            for (const s of elf.symbols.slice(0, 20)) {
                console.log(`0x${s.value.toString(16)} ${s.name}`);
            }
        }
    } catch (err) {
        console.error(`error: ${err?.message ?? err}`);
        process.exit(1);
    }
}
