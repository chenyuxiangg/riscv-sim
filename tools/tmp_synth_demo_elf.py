#!/usr/bin/env python3
"""Synthesize a minimal RV32IMC ELF that writes tohost=1 (PASS) and self-loops.

Layout:
  [  0 ..  52) ELF header
  [ 52 ..  84) program header (PT_LOAD, file off = 84)
  [ 84 .. 100) code (4 words)
  [100 ..  ...) section headers + .symtab/.strtab/.shstrtab (minimal, with `tohost`)
"""
import struct, sys

ENTRY = 0x8000_0000
TOHOST = 0x8000_1000
out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/demo-pass.elf"

code = [
    0x00100093,  # addi x1, x0, 1   -> x1 = 1
    0x80001137,  # lui  x2, 0x80001 -> x2 = 0x80001000
    0x00112023,  # sw   x1, 0(x2)   -> tohost = 1 (exit bit set, code 0 => PASS)
    0x0000006F,  # j .              -> spin
]
code_bytes = b"".join(struct.pack("<I", w) for w in code)

code_off = 84

# ---------- ELF header ----------
eh = bytearray(52)
eh[0:4] = b"\x7fELF"
eh[4] = 1            # ELFCLASS32
eh[5] = 1            # ELFDATA2LSB
eh[6] = 1            # EV_CURRENT
struct.pack_into("<H", eh, 16, 2)     # e_type = ET_EXEC
struct.pack_into("<H", eh, 18, 243)   # e_machine = EM_RISCV
struct.pack_into("<I", eh, 20, 1)     # e_version
struct.pack_into("<I", eh, 24, ENTRY) # e_entry
phoff = 52
struct.pack_into("<I", eh, 28, phoff) # e_phoff
shoff = code_off + len(code_bytes)
struct.pack_into("<I", eh, 32, shoff) # e_shoff
struct.pack_into("<H", eh, 40, 52)    # e_ehsize
struct.pack_into("<H", eh, 42, 32)    # e_phentsize
struct.pack_into("<H", eh, 44, 1)     # e_phnum
struct.pack_into("<H", eh, 46, 40)    # e_shentsize
struct.pack_into("<H", eh, 48, 4)     # e_shnum
struct.pack_into("<H", eh, 50, 3)     # e_shstrndx

# ---------- program header ----------
ph = struct.pack(
    "<IIIIIIII",
    1,            # p_type = PT_LOAD
    code_off,     # p_offset
    ENTRY,        # p_vaddr
    ENTRY,        # p_paddr
    len(code_bytes),  # p_filesz
    len(code_bytes),  # p_memsz
    5,            # p_flags = R|X
    4,            # p_align
)

# ---------- section headers ----------
# Section 0: NULL
sh_null = bytes(40)

# Section 1: .symtab (2 entries: null + tohost)
symtab_off = shoff + 4 * 40
symtab = bytes(16)  # null symbol
symtab += struct.pack("<IIIBBH", 1, TOHOST, 8, 0, 0, 1)  # st_name, st_value, st_size, st_info, st_other, st_shndx
symtab += bytes(16)  # pad to 2 entries (16+16 = 32 bytes)
symtab = symtab[:32]

sh_symtab = struct.pack(
    "<IIIIIIIIII",
    1,            # sh_name -> ".symtab"
    2,            # sh_type = SHT_SYMTAB
    0,            # sh_flags
    0,            # sh_addr
    symtab_off,   # sh_offset
    len(symtab),  # sh_size
    2,            # sh_link -> .strtab
    1,            # sh_info
    4,            # sh_addralign
    16,           # sh_entsize
)

# Section 2: .strtab
strtab = b"\x00tohost\x00"
strtab_off = symtab_off + len(symtab)
sh_strtab = struct.pack(
    "<IIIIIIIIII",
    9,            # sh_name -> ".strtab"
    3,            # sh_type = SHT_STRTAB
    0, 0,         # flags, addr
    strtab_off,
    len(strtab),
    0, 0, 1, 0,
)

# Section 3: .shstrtab
shstrtab = b"\x00.symtab\x00.strtab\x00.shstrtab\x00"
shstrtab_off = strtab_off + len(strtab)
sh_shstrtab = struct.pack(
    "<IIIIIIIIII",
    17,           # sh_name -> ".shstrtab"
    3, 0, 0,
    shstrtab_off,
    len(shstrtab),
    0, 0, 1, 0,
)

blob = bytes(eh) + ph + code_bytes + sh_null + sh_symtab + sh_strtab + sh_shstrtab + symtab + strtab + shstrtab

with open(out_path, "wb") as f:
    f.write(blob)
print(f"wrote {out_path} ({len(blob)} bytes)")
