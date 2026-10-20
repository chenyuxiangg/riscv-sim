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

Spike's built-in HTIF treats `tohost` as a 64-bit word:

```
tohost = (exit_code << 1) | 1      ->  the hart asks to exit with `exit_code`
```

Spike's `syscall_t::handle_syscall` does exactly
`if (cmd.payload() & 1) { htif->exitcode = cmd.payload(); }`, and
`htif_t::exit_code()` is `exitcode >> 1`.  Spike also locates `tohost`
and `fromhost` through the **ELF symbol table** — an image without those
two global symbols is rejected outright ("tohost and fromhost symbols not
in ELF; can't communicate with target").

Our minimal environment writes exactly that encoding, so the verdict is
byte-compatible with the upstream riscv-tests environment.

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

* `isa/rv32ui/*` — in scope (42 cases)
* `isa/rv32um/*` — in scope, needs the **M** extension (8 cases)
* `isa/rv32uc/*` — in scope, needs the **C** extension (1 case)
* `isa/rv32mi/*` — **out**: machine-level, needs CSR + trap delivery
* `isa/rv32si/*` — **out**: supervisor-level, same

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

## Why there is a self-check

A comparison harness that cannot fail is worse than no harness: if both
sides always answered `PASS`, every corpus case would "agree" and the
run would look green while proving nothing.  `--self-check` therefore
runs two mutation controls, both of which must report **`FAIL 3`** on
both sides:

1. `canary/fail_3.S` — self-contained; proves the FAIL verdict encoding
   decodes to the right test number on both sides.
2. a **mutated** copy of the upstream `rv64ui/add.S` whose expected value
   for test 3 is corrupted; proves a *behavioural* ISA difference is
   actually detected, not just a protocol mismatch.

If either canary does not report `FAIL 3` on both sides, the harness
reports "the comparison is not discriminating" and exits non-zero.

## Adding a case

1. Add the case to `corpus.txt` with its `march` and its `status`.
   Use `pending` for anything M-13 does not implement yet — a `pending`
   case that starts agreeing is reported as `NOW-PASSING`, which is the
   signal to flip it to `active`.
2. Run `tools/golden-run.sh --filter <suite/test>`.
3. `rv32mi` / `rv32si` cases need trap delivery; they cannot be added
   before M-16 lands.
