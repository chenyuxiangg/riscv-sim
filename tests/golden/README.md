# Golden comparison harness — Spike vs. our ISS

This directory holds the **candidate** side of the platform's correctness
check against an external oracle (Spike).  The corpus itself is *not*
vendored here: the test cases come from the upstream
[riscv-tests](https://github.com/riscv-software-src/riscv-tests) clone
that `tools/fetch-golden-deps.sh` puts outside the repository.

```
riscv_sim/
├── tests/golden/
│   ├── env/            our minimal, CSR-free test environment
│   │   ├── riscv_test.h
│   │   └── link.ld
│   ├── canary/         mutation controls (must report FAIL 3 on both sides)
│   │   └── fail_3.S
│   ├── corpus.txt      the authoritative list of in-scope cases
│   └── README.md       this file
└── tools/
    ├── fetch-golden-deps.sh   fetches riscv-tests + the cross toolchain
    └── golden-run.sh         builds both sides, compares the verdicts
```

## How a run works

For every case in `corpus.txt` the harness assembles **one** ELF and runs
that same ELF on two sides:

| side      | what runs                                              |
|-----------|--------------------------------------------------------|
| reference | `spike --isa=rv32imc_zicclsm_zifencei -m0x80000000:0x400000` |
| candidate | `riscv-golden` (M-06 loader + M-14 bus + M-13 ISS)     |

Both sides expose the **same memory map** (stack window 4 KiB at
`0x7ffe_f000` + RAM 4 MiB at `0x80000000`) — if the candidate mapped
only the image's own span, a program touching an address outside the
image but inside Spike's memory would fault on one side and run fine on
the other.  Spike's multi-region `-m` parser only accepts **decimal**
numbers, hence the decimal literals in the script.

Both sides must reach the **same verdict**:

| verdict     | meaning                                             |
|-------------|-----------------------------------------------------|
| `PASS`      | the guest asked to exit with code 0                 |
| `FAIL <n>`  | the guest asked to exit with code `n`               |
| `NOVERDICT` | no verdict was produced within the timeout / budget |

A `FAIL` must agree on the **test number** as well — that is what makes
the comparison meaningful rather than a binary "did it crash" check.

```
tools/golden-run.sh --self-check     # mutation canaries only
tools/golden-run.sh --keep-going     # the whole corpus
tools/golden-run.sh --suite rv32ui   # one suite
tools/golden-run.sh --list           # show the corpus
```

## The HTIF verdict protocol

Spike's built-in HTIF treats `tohost` as a 64-bit word, but the
**payload field inside it is only 48 bits wide** — `fesvr/device.h`:

```cpp
uint64_t payload() { return tohost << 16 >> 16; }   // only the low 48 bits
```

Bits 48..63 of `tohost` are therefore **ignored by Spike**.  What
matters is:

```
tohost = (exit_code << 1) | 1      ->  the hart asks to exit with `exit_code`
```

Spike's `syscall_t::handle_syscall` does exactly
`if (cmd.payload() & 1) { htif->exitcode = cmd.payload(); }`, and
`htif_t::exit_code()` is `exitcode >> 1` — the shift happens in 64 bits
and the result is then truncated to an `int`.  **The order matters**:
decoding the full 64-bit word and checking for zero *before* truncating
turns a `PASS` into a `FAIL` and can even produce the impossible verdict
`FAIL 0` (the protocol reserves 0 for "pass").  The candidate runner
therefore masks to 48 bits and truncates to 32 bits *before* the
zero-check.

Spike also locates `tohost` and `fromhost` through the **ELF symbol
table** — an image without those two global symbols is rejected outright
("tohost and fromhost symbols not in ELF; can't communicate with target").
The candidate runner never writes `fromhost`; the corpus does not need
it.

Our minimal environment writes exactly that encoding, so the **verdict
value** is byte-compatible with the upstream riscv-tests env — the two
paths write the same 64-bit word (same low word, same `sw zero` to the
high word, same `tohost` address).  The **execution path** is
different: upstream goes ECALL → trap → trap vector → `sw`, while ours
writes `tohost` directly (our environment never programs `mtvec`).

Two further differences from upstream, both harmless for this corpus:

* **Privilege mode.**  Upstream does `csrwi mstatus, 0` + `mret` and
  therefore runs in **U mode**; ours stays in **M mode**.  The 51
  in-scope cases contain zero CSR or privileged instructions, so nothing
  changes — and both sides run the same ELF, so the comparison stays
  self-consistent either way.
* **`sp`.**  The upstream test macros do `la x2, base`, i.e. they
  **hijack `sp` as a general-purpose data pointer** (`TEST_LD_OP` /
  `TEST_ST_OP`).  The stack itself is therefore never used by the
  corpus; the initial `sp` value is irrelevant to every result.

## Why the environment is CSR-free

Upstream riscv-tests' `env/p/riscv_test.h` is machine-mode oriented:

* `RVTEST_CODE_BEGIN` programs `mtvec`, `mstatus`, `mepc`, `satp`,
  `pmpaddr0`, `pmpcfg0`, `mie`, `medeleg`, `mideleg`, `mnstatus`, reads
  `mhartid`, and ends with `mret`;
* `RVTEST_PASS` / `RVTEST_FAIL` report the verdict by raising an `ECALL`
  (`a7 = 93`) that the trap vector converts into a `sw` to `tohost`.

Assembling that needs the `zicsr` extension, and running it needs full
trap delivery.  M-13 implements the RV32I *user-level* ISA; trap delivery
and the interrupt controller (M-16) land later.

Because the HTIF wire format is much simpler than the route upstream takes
to reach it, the whole CSR + trap machinery can be dropped and `tohost`
written directly — **with the verdict encoding unchanged**.  The full
rationale, including what is dropped and why it is safe, lives in the
header comment of `env/riscv_test.h`.

The consequence for the corpus:

* `isa/rv32ui/*` — in scope (RV32I)
* `isa/rv32um/*` — in scope, needs the **M** extension
* `isa/rv32uc/*` — in scope, needs the **C** extension
* `isa/rv32mi/*` — **out**: machine-level, needs CSR + trap delivery
* `isa/rv32si/*` — **out**: supervisor-level, same

`corpus.txt` is the authoritative list — its `status` column says
whether a case must agree with Spike (`active`) or is a known gap
(`pending`), and every `pending` row must carry the reason and the
ledger entry that tracks it.

### The upstream RV32 suites, and why each is in or out

| upstream suite | cases | in the corpus? | why |
| --- | --- | --- | --- |
| `rv32ui` | 42 | ✅ in (`active`) | the RV32I base — the whole point of this phase |
| `rv32um` | 8 | ✅ in (`pending`) | needs the **M** extension → **D-43** |
| `rv32uc` | 1 | ✅ in (`pending`) | needs the **C** extension → **D-43** |
| `rv32mi` | 16 | ❌ out | machine-level: CSR access, breakpoints, PMP, `mret`, trap-and-continue handlers. Needs trap delivery → **M-16** (phase 5) |
| `rv32si` | 6 | ❌ out | supervisor-level: the same plus S-mode → **M-16** (phase 5) |
| `rv32ua` | 10 | ❌ out | **A** extension (atomics) — not implemented; a natural phase-3 addition next to M/C |
| `rv32uf` / `rv32ud` | 11 / 11 | ❌ out | floating point — out of MVP scope by design (AD §3.3) |
| `rv32up` | 446 | ❌ out | **P** extension — far beyond MVP scope |
| `rv32uz*` (13 suites) | ~80 | ❌ out | Zba / Zbb / Zbc / Zbkb / Zbkx / Zbs / Zfh / Zicond / Zacas / Zalasr … — bit-manipulation and beyond; out of MVP scope |

The excluded machine- and supervisor-level suites contain **155**
CSR / trap instructions between them; the 51 in-scope cases contain
**zero**.

## The Zicclsm detail — read this before touching the ISA string

Spike **traps** on misaligned data accesses by default.  The exception
is `riscv/mmu.cc`:

```c++
bool misaligned_ok = intrapage && is_misaligned_enabled();
...
if (!is_misaligned_enabled())
  throw trap_load_address_misaligned(...);
```

and `is_misaligned_enabled()` is

```c++
int is_misaligned_enabled() { return proc && proc->extension_enabled(EXT_ZICCLSM); }
```

So misaligned accesses are only *performed* when the **Zicclsm**
extension is enabled.  `rv32ui/ma_data` is exactly the case that
exercises this, and it is the one that hangs without Zicclsm (the trap
goes to `mtvec`, which the minimal environment never programs).

Our ISS's default `MisalignedPolicy::Allow` **performs** misaligned
accesses, which matches Spike *with* Zicclsm.  The harness therefore
pins the reference ISA to

```
rv32imc_zicclsm_zifencei
```

* `zicclsm` — so Spike performs misaligned accesses, like our default
  `Allow` policy;
* `zifencei` — so `fence_i` assembles and runs;
* `imc` — the base ISA plus the M and C extensions.

Note that M-13's LLD says the default policy is "模拟器与 Spike 一致允许非对齐".
That statement is only true of a Spike built with Zicclsm enabled — the
stock `--isa=rv32imc` traps.  The ledger entry **D-42** tracks the
wording fix.

## Why there is a self-check — and what it does *not* prove

A comparison harness that cannot fail is worse than no harness: if both
sides always answered `PASS`, every corpus case would "agree" and the
run would look green while proving nothing.  `--self-check` therefore
runs three controls:

| control | expects | proves |
|---------|---------|--------|
| `canary/fail_3.S` (self-contained) | `FAIL 3` on both sides | the FAIL verdict encoding decodes to the right test number on both sides |
| upstream `rv64ui/add.S`, **unmodified** | `PASS` on both sides | the **positive** direction — the harness can recognise a PASS, not just a FAIL |
| upstream `rv64ui/add.S`, **expected value corrupted** | `FAIL 3` on both sides | a *behavioural* ISA difference is actually detected |

If any control does not hold, the harness reports "the comparison is not
discriminating" and exits non-zero.  The controls also run on **every
corpus pass**, not just under `--self-check`.

> ⚠️ **What the controls do NOT prove.**  They exercise the *verdict
> transport path* — `li` / `beqz` / `sll` / `or` / `sw` / `auipc` / `j`.
> An instruction **outside** that path cannot be detected by them, by
> construction: they never execute it.  Measured on this corpus, **8 of
> 10** injected RV32I ALU defects escaped the controls while **10 of 10**
> were caught by the full corpus comparison.  The controls are a
> *protocol handshake self-check*; **the full comparison is what supplies
> the discriminating power**, because Spike is an independent oracle and
> the two sides must agree on the same ELF.
>
> A stronger form — mutating *every* test number in the corpus so each
> assertion has its own failure path, plus fault injection into the
> candidate — is tracked as **D-44**.

A missing verdict is **not** an agreement.  If either side produces
`NOVERDICT` (the reference timed out, or the candidate ran out of
instruction budget) the case is counted as `no-verdict` and **fails the
run** — otherwise a harness whose two sides are both simply broken would
report them as consistent and exit green.

## Adding a case

1. Add the case to `corpus.txt` with its `march` and its `status`.
   Use `pending` for anything M-13 does not implement yet — a `pending`
   case that starts agreeing is reported as `NOW-PASSING`, which is the
   signal to flip it to `active`.
2. Run `tools/golden-run.sh --filter <suite/test>`.
3. `rv32mi` / `rv32si` cases need trap delivery; they cannot be added
   before M-16 lands.
