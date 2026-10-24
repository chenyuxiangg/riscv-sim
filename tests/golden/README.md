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
| reference | `spike --isa=rv32imc_zicsr_zicclsm_zifencei_zicntr -m0x80000000:0x400000` |
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

## The two environments

There are **two** golden environments, selected per case by the `env`
column of `corpus.txt`:

| variant | directory | what it provides | used by |
| --- | --- | --- | --- |
| *(default)* | `env/` | **CSR-free**: `tohost` is written directly, no trap vector, no `mtvec` programming, no `mret` | the 51 RV32I/M/C cases |
| `csr` | `env/csr/` | **CSR-capable**: trap vector, `mtvec` / `mie` / `mstatus` programming, `mepc` + `mret` epilogue, and the verdict carried by the `ECALL` | the machine-level corpus |

The default environment is CSR-free because the HTIF wire format is much
simpler than the route upstream takes to reach it: the whole CSR + trap
machinery can be dropped and `tohost` written directly — **with the verdict
encoding unchanged**.  The full rationale, including what is dropped and
why it is safe, lives in the header comment of `env/riscv_test.h`.

The `csr` environment exists because the machine-level corpus *is* about
CSRs, traps and `mret` — the CSR-free environment has neither a trap vector
nor a way to program `mtvec`, so those tests could not even be assembled
meaningfully.  Its header comment explains the two sides' shared trap
protocol and the three places where the platform's absences (no PMP, no
RNMI, no paging, no delegation) make the upstream writes inert.

⚠️ **The `no PMP` clause of that sentence is stale as of v0.37.**  The
platform *does* implement PMP now ⇒ ⇒ the upstream `INIT_PMP` write is no
longer ignored ⇒ ⇒ the deviation is registered in the section
*The `csr` environment's `INIT_PMP` comment is stale* below — the env file
itself is a verbatim mirror of upstream and **cannot** be edited.

⚠️ **`env/csr/riscv_test.h` is verified against upstream, not eyeballed.**
The corpus build of all 16 `rv32mi` cases produces a **byte-identical
`.text.init`** with our variant and with upstream's own `env/p/riscv_test.h`
— i.e. the reimplementation executes the same instruction stream as the
reference environment, so the golden comparison compares the same program on
both sides.

`env/encoding.h` holds the handful of architectural constants the CSR
variant and the upstream test bodies reference.  They are upstream's values
verbatim: they are encodings the specification fixes, not platform choices,
and a "cleaned up" constant would silently diverge from the reference side.

⚠️ **That claim is now enforced, not just asserted.**  The harness
compares every constant in `env/encoding.h` against upstream's
`env/encoding.h` (whitespace- and case-insensitive) **before it builds
anything**, and a mismatch is a hard error:

```
golden-run: encoding.h: 'PMP_A' = '0x0c', upstream says '0x18' — the corpus must run upstream's program
```

⚠️ **Why a hard error and not a warning**: the harness compiles **one**
image and hands it to **both** sides, so a drifted constant does not show up
as a side-vs-side mismatch — it silently replaces the upstream test program
with a slightly different one, and the corpus keeps comparing two different
programs while looking green.  (The first version of `env/encoding.h` did
carry such a drift: `PMP_A` was `0x0c` where upstream says `0x18`.)  The
byte-identity check above remains the **second** line of defence — it proves
the two *environments* agree; the constant check proves neither drifted
from upstream.

### The upstream RV32 suites, and why each is in or out

| upstream suite | cases | in the corpus? | why |
| --- | --- | --- | --- |
| `rv32ui` | 42 | ✅ in (`active`) | the RV32I base — the whole point of this phase |
| `rv32um` | 8 | ✅ in (`active`) | needs the **M** extension — landed in v0.16 (**D-43** closed) |
| `rv32uc` | 1 | ✅ in (`active`) | needs the **C** extension — landed in v0.16 (**D-43** closed) |
| `rv32mi` | 16 | ✅ 13 in (`active`, `csr` env) | machine-level: CSR access, breakpoints, `mret`, trap-and-continue handlers — trap delivery landed in v0.19, so 13 of the 16 are now comparable |
| `rv32si` | 6 | ❌ out | supervisor-level: the same **plus S-mode** — the platform is M-mode only (**D-94**) |
| `rv32ua` | 10 | ❌ out | **A** extension (atomics) — not implemented; a natural phase-3 addition next to M/C |
| `rv32uf` / `rv32ud` | 11 / 11 | ❌ out | floating point — out of MVP scope by design (AD §3.3) |
| `rv32up` | 446 | ❌ out | **P** extension — far beyond MVP scope |
| `rv32uz*` (13 suites) | ~80 | ❌ out | Zba / Zbb / Zbc / Zbkb / Zbkx / Zbs / Zfh / Zicond / Zacas / Zalasr … — bit-manipulation and beyond; out of MVP scope |

## Cases that are *not* in the corpus

Three `rv32mi` cases and all six `rv32si` cases are **not** rows in
`corpus.txt`, and that is deliberate:

* the harness's `pending` rule tolerates **only** "the candidate cannot
  execute it at all" (`ERROR` / `NOVERDICT`).  Two of the three `rv32mi`
  cases *do* execute — they merely diverge, or agree only vacuously,
  because the platform lacks a feature the case tests.  Listing them would
  force the rule to be loosened into a hole ("any divergence is fine"),
  which is exactly what the rule was written to prevent;
* a divergence nobody watches is still a divergence — so each one is
  recorded **here**, with the measured verdict, and tracked by the ledger.

| case | Spike | ours | why it diverges |
| --- | --- | --- | --- |
| `rv32mi/illegal` | `PASS` | `NOVERDICT` | needs S-mode semantics: `csrwi mip, MIP_SSIP` is ignored on this platform, so the case never finishes → **D-94** |
| `rv32mi/pmpaddr` | `PASS` | `PASS` | agrees — but **vacuously**: the case auto-detects the granularity `G` from the read-back of `pmpaddr0`, and the reference side stores `pmpaddr` **verbatim** (writing all ones reads back `0xffffffff` — measured) ⇒ ⇒ the lowest set bit is bit 0 ⇒ ⇒ `G` = 0 ⇒ ⇒ the case takes its **own** `beqz a7, pass` early exit ⇒ ⇒ **the main test never runs on the reference side** ⇒ ⇒ the agreement proves nothing ⇒ ⇒ it stays out of the corpus → **D-94** |
| `rv32mi/zicntr` | `PASS` | `PASS` | the reference side now implements Zicntr — the default ISA carries `zicntr` ⇒ it no longer traps ⇒ but the agreement is **hollow**: the case only checks that reading `cycle` / `instret` / `cycleh` / `instreth` in four ways does **not** trap ⇒ a CSR read on this platform never traps ⇒ ⇒ the case **cannot fail here** ⇒ ⇒ it stays out of the corpus → **D-94** |
| `rv32si/csr` | `PASS` | `FAIL 22` | S-mode: the case runs in S-mode, we stay in M-mode → **D-94** |
| `rv32si/ma_fetch` | `PASS` | `PASS` | agrees — but for a **vacuous** reason (neither side reaches the S-mode path the case is about) ⇒ it is *not* promoted to `active` until S-mode exists |
| `rv32si/sbreak` | `PASS` | `FAIL 669` | S-mode → **D-94** |
| `rv32si/scall` | `PASS` | `FAIL 669` | S-mode → **D-94** |
| `rv32si/wfi` | `PASS` | `FAIL 668` | S-mode → **D-94** |
| `rv32si/dirty` | — | — | does not even assemble: it needs `SATP_MODE` / `SATP_MODE_SV32`, which only matter for paging → **D-94** |

⚠️ **`rv32si/ma_fetch` is the interesting one**: the two sides agree, so a
naive "promote it to `active`" would put a **vacuous** agreement into the
corpus — a green row that proves nothing.  It stays out until the S-mode
path it actually tests exists.

⚠️ The `zicntr` row is no longer a **platform-vs-reference semantic
difference**: the reference side now implements Zicntr, so it no longer
raises an illegal-instruction exception on `cycle`.  What is left is a
**hollow agreement**: the case asserts only that the four reads do not
trap, and a CSR read on this platform never traps — an implemented CSR
returns its value, an unimplemented one reads 0 without trapping (the
rule STAGE-6 adjudication D chose) ⇒ ⇒ the case cannot fail here ⇒ ⇒ it
stays out of the corpus rather than becoming a green row that proves
nothing.

## `active` rows that agree **vacuously** — read this before citing coverage

⚠️ **An `active` row means "both sides reach the same verdict".  It does
NOT mean "both sides exercised the same thing".**  Two of the 13 `rv32mi`
rows are **hollow agreements**: the verdicts match, but the two sides ran
**different programs** (or the case asserts something the platform cannot
fail).  They stay in the corpus — the agreement is real and worth keeping
green — but they must **never** be cited as coverage of the behaviour they
name.  The project's own rule (STAGE-6 adjudication I): **a hollow
agreement is more dangerous than a divergence**, because a divergence is
visible.

| case | why the agreement is hollow | how that was established |
| --- | --- | --- |
| `rv32mi/csr` | the case branches on `misa[20]` (the **U** bit). The reference side runs with its default `--priv=MSU` ⇒ `misa = 0x40141104` ⇒ **U is set** ⇒ it runs the U-mode section (`csrwi mcounteren`, `sret` into user mode, "reading `sstatus` in user mode must trap"). This platform is **M-mode only** ⇒ `misa = 0x40001104` ⇒ **U is clear** ⇒ the case jumps straight to `finish` and passes early ⇒ **the whole user-mode / privilege-transition section never runs here** | instrumented copy of the upstream `rv64si/csr.S`: the reference side reaches the U-mode marker, we do not. Trap entries: reference side 5, ours 1. Ledger **D-96** |
| `rv32mi/ma_fetch` | the case probes "can the `C` bit be switched off?" with `csrci misa, 1 << ('c'-'a')`. The reference side's `misa` is **WARL-writable** (the write really clears `C`); ours is **fully read-only** ⇒ the **same probe takes opposite branches**: the reference side continues into the IALIGN assertions, we skip to `pass` ⇒ **that half of the case is not covered here** | instrumented copy of the upstream `rv64si/ma_fetch.S`: the reference side hits the IALIGN-section marker, we do not. Ledger **D-95** |

⚠️ **The two differences above all come from the same root**: the
platform implements **fewer** features than the reference side
(U-mode, S-mode) and reports that honestly in `misa` instead of
pretending.  ⇒ A hollow agreement here is the **expected** consequence of
an M-mode-only platform — the fix is **labelling**, not faking a match.

⚠️ **PMP used to be on that list** (v0.36 and earlier).  As of **v0.37**
the platform implements PMP ⇒ ⇒ the remaining gap is **U-mode and S-mode**
only ⇒ ⇒ `rv32mi/pmpaddr` now agrees for a *different* reason (it is
**hollow on the reference side**, not hollow on ours — see its row above).


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
rv32imc_zicsr_zicclsm_zifencei_zicntr
```

* `zicclsm` — so Spike performs misaligned accesses, like our default
  `Allow` policy;
* `zifencei` — so `fence_i` assembles and runs;
* `imc` — the base ISA plus the M and C extensions;
* `zicsr` — so the reference side has the CSR instructions the counter cases consist of;
* `zicntr` — so the reference side has the counter CSRs, shadows included.

⚠ **Why `zicsr` is in the string — and why it is *not* the load-bearing
one.**  Measured on this machine's Spike: the base `rv32imc` already
**implies** Zicsr ⇒ ⇒ `csrr` / `csrw` execute fine even under the old
string `rv32imc_zicclsm_zifencei` ⇒ ⇒ the counter cases *do* run to
completion there (under the old string the reference side already reports
`PASS` for `rv32mi/instret_overflow`).  ⇒ ⇒ `zicsr` is in the string to
**name** the dependency explicitly instead of relying on the implication —
it is **not** what makes the counter cases work.

⚠ **Why `zicntr` is in the string — this one *is* load-bearing.**  Zicntr
brings the **shadow** counters — `cycle` / `instret` / `cycleh` /
`instreth` — and those are **standard** CSRs ⇒ ⇒ Spike gates them by
extension ⇒ ⇒ without `zicntr` reading `cycle` raises an
illegal-instruction exception ⇒ ⇒ the `rv32mi/zicntr` case traps on the
reference side instead of running ⇒ ⇒ that case cannot be compared at
all.  Measured: with the previous string the reference side reported
`FAIL 2` for it, with this one it reports `PASS`.

⚠ **Mind the asymmetry, because it is easy to get backwards.**  Spike
gates **standard** CSRs by extension but does **not** gate **custom**
ones ⇒ ⇒ `mcycle` / `minstret` / `mcountinhibit` (custom addresses) were
already readable **and writable** on the reference side under the old
string — measured: writing `0xFFFFFFFF` into `minstret` lands and the
counter wraps ⇒ ⇒ it is the **shadows** alone that need `zicntr`.

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

## The `local` suite — this repository's own cases

Most corpus rows come from the upstream riscv-tests tree.  A few do
not have to: some behaviours are **ours** to get right and upstream
simply has no case for them.  Those live in the `local` suite:

| row | source | what it pins |
| --- | --- | --- |
| `local/unmapped_store` | `tests/golden/local/unmapped_store.S` | a **store** to an unmapped address traps with `mcause = 7` (store access fault) |
| `local/unmapped_load` | `tests/golden/local/unmapped_load.S` | a **load** from an unmapped address traps with `mcause = 5` (load access fault) |
| `local/unmapped_fetch` | `tests/golden/local/unmapped_fetch.S` | an **instruction fetch** from an unmapped address traps with `mcause = 1` (instruction access fault), `mtval` = `mepc` = the fetch address |
| `local/compressed_trap` | `tests/golden/local/compressed_trap.S` | a **compressed** store to an unmapped address traps, and the trap context carries the **exact** faulting PC (bit 1 preserved) |
| `local/pmp_deny` | `tests/golden/local/pmp_deny.S` | **PMP denies a store to a mapped address**: with `mstatus.MPRV = 1`, entry 0 (NAPOT, 4 KiB at `0x80040000`, **no permissions**) matches ⇒ the store traps with `mcause = 7` and `mtval = 0x80040000` |

The corpus is **69** cases now (it was 68 before `local/pmp_deny`).

### `pmp_deny` — PMP must deny a store to an address that **is** mapped

This is the one behaviour the upstream tree cannot supply: **PMP has no
upstream case that exercises a *denial*.**  `rv32mi/pmpaddr` writes a
`pmpaddr` register and reads it back — it never performs an access that
PMP could refuse ⇒ ⇒ a platform that *stores* the PMP registers but
ignores them on the access path would still agree with it ⇒ ⇒ that case
is hollow (see the *Cases that are not in the corpus* section above)
⇒ ⇒ the corpus needs a case of our own.

`pmp_deny.S` is that case, and it pins four things:

1. **What is denied.**  With `mstatus.MPRV = 1` (M-mode loads and stores
   are subject to PMP only while `MPRV` is set), PMP entry 0 (NAPOT,
   4 KiB at `0x80040000`, **no** `R`/`W`/`X`) matches the access ⇒ ⇒ the
   store is denied ⇒ ⇒ the guest's trap handler must see `mcause == 7`
   (store access fault), `mtval == 0x80040000` and `mepc` == the address
   of that `sw`.
2. **Why the denied address has to be mapped.**  `0x80040000` is
   **inside** the RAM window **both** sides map (`0x8000_0000` + 4 MiB)
   ⇒ ⇒ the store can fail **only** because of PMP, not because the
   address is unmapped ⇒ ⇒ that is exactly what separates this case
   from `unmapped_store` (which faults on `0x90000000`, unmapped on both
   sides).  A PMP denial and an unmapped access carry the **same** cause
   numbers, so the **address** — not the cause — is what tells them
   apart.
3. **The control group.**  Entry 1 (NAPOT, 1 MiB at `0x8000_0000`,
   `R`+`W`+`X`) permits the rest of the low RAM ⇒ ⇒ the guest stores to
   `0x80002000` — inside entry 1, outside entry 0 — and reads the value
   back, and both must succeed ⇒ ⇒ without that control, an
   implementation that denies *every* access would be green too.
4. **The criterion is the guest's own check** (see below): the guest
   reports PASS only when `mcause`, `mtval` and `mepc` are all exactly
   right ⇒ ⇒ a platform without PMP lets the store succeed, falls
   through to `fail` and reports **FAIL 3** ⇒ ⇒ the row has
   discriminating power.

⚠️ **The failure modes carry distinct numbers**, so a red row says *which*
invariant broke — a single "FAIL 3" for everything would hide the
difference between "PMP refused nothing" and "PMP refused the wrong
address":

| verdict | meaning |
| --- | --- |
| `PASS` | the reference side's exact behaviour (all three fields match) |
| `FAIL 3` | the denied store was **not** denied (PMP did not refuse) |
| `FAIL 4` | the **control group** broke (PMP refused an access entry 1 permits, or the read-back did not match) |
| `FAIL 5` | the trap happened but `mcause` / `mtval` / `mepc` did **not** match (e.g. PMP refused the *wrong* address) |

⚠️ **Every verdict path clears `mstatus.MPRV` first.**  The verdict travels
through an `ecall`, and the environment's trap vector turns it into the
`tohost` write — which is a **store** ⇒ ⇒ with `MPRV` still set, that
write is itself subject to PMP ⇒ ⇒ if no entry covers the `tohost`
address, the case could report nothing and would **hang** instead of
failing ⇒ ⇒ the failure path must be able to report, too.

⚠️ **The packing pitfall, because it is easy to get wrong.**  RV32's
`pmpcfg{i}` register holds **four** 8-bit entries (byte 0 = entry 0,
byte 1 = entry 1) ⇒ ⇒ entry 1 has to be packed into **byte 1 of
`pmpcfg0`**, so the whole register is `pmpcfg0 = 0x1f18` (entry 0 =
`0x18`, entry 1 = `0x1f`).  Writing `pmpcfg1` instead configures entries
**4..7** ⇒ ⇒ entry 1 would silently never exist ⇒ ⇒ the control group
would then fail for the wrong reason.  (The `pmpcfg` byte layout itself
— `L` = bit 7, `A` = bits 4:3, `X` = bit 2, `W` = bit 1, `R` = bit 0 —
matches upstream's `env/encoding.h`, so that file needs no change.)

The harness resolves a row's source in this order:

1. `$RISCV_TESTS_DIR/isa/<suite>/<test>.S` — the upstream tree (every
   suite **except** `local`);
2. `tests/golden/local/<test>.S` — this repository, for the `local`
   suite only.

The fallback can never shadow an upstream case: it is reachable **only**
for the `local` suite, and the upstream tree has no `isa/local/`.

### Why the verdict is the guest's own check

⚠️ **The harness compares the tohost exit code** — it cannot see
`mcause` or `mtval`.  So the *guest* has to check them: the trap
handler reads `mcause`, `mtval` and `mepc` and writes `PASS` **only**
when all three are exactly the expected values (`7` / `5`, the faulting
address, the faulting instruction's address).  A wrong number on either
side turns into `FAIL 3` on that side — a **mismatch**, not a quiet
agreement.

⚠️ This matters because the weaker criterion — "both sides stopped
erroring" — is **green for two sides that both abort**, which is
exactly the state the platform was in before ledger **D-99** was
closed (the bus error was propagated verbatim and the guest's handler
never ran).

All five `local` cases are **self-contained** (they include no
riscv-tests source, like `canary/fail_3.S`) and they use the `csr`
environment, because they need a trap vector, `mtvec`, CSR reads and
`mret`-free trap handling.

## The `csr` environment's `INIT_PMP` comment is stale — and cannot be fixed

⚠️ **`tests/golden/env/` is a verbatim mirror of upstream riscv-tests.**
`tools/golden-run.sh`'s `check_env_constants` compares it byte for byte
against the upstream tree ⇒ ⇒ **not one character of it may be edited**
⇒ ⇒ a comment in it that has become wrong **cannot be corrected in
place** ⇒ ⇒ the deviation has to be registered **here**, in this file.

The comment in question sits next to `INIT_PMP` in
`tests/golden/env/csr/riscv_test.h`:

```
// A permissive PMP entry.  ⚠️ **Ignored on this platform** (no PMP) — but
// the write stays, so that both sides execute the same instruction stream.
```

It claims the platform has no PMP ⇒ ⇒ the write is ignored ⇒ ⇒ every
access is permitted.  After v0.37 that is **half** wrong:

* `INIT_PMP` writes `pmpaddr0 = 0x7fffffff` and `pmpcfg0 = 0x1f`
  (`PMP_NAPOT | PMP_R | PMP_W | PMP_X`);
* the platform **does** implement PMP now ⇒ ⇒ that write **takes
  effect** — it is no longer ignored;
* but the entry it configures is a NAPOT entry whose `pmpaddr` ends in
  **31** ones ⇒ ⇒ its range is `[0, 2^34)` ⇒ ⇒ it covers the **whole
  32-bit space** ⇒ ⇒ with `R`+`W`+`X` it still permits **everything**.

⇒ ⇒ **The behaviour is unchanged**: the reference side has always run
under exactly one "permit all" PMP entry, and so does ours.  ⇒ ⇒ **The
verdicts of the 68 pre-existing corpus cases do not change** — this
batch has zero effect on them (STAGE-21 fact F19).  ⇒ ⇒ With
`local/pmp_deny` the corpus itself grows from **68** to **69** cases.

⚠️ **Related, and not fixable in the env either**: the `rv32mi/pmpaddr`
row in the *Cases that are not in the corpus* section above carries a
**pre-v0.37 measurement** — back then the platform had no PMP at all, so
`pmpaddr0` read back 0, the case's `beqz a7, fail` fired, and the
verdict was `FAIL 1`.  ⇒ ⇒ That row has been **re-measured** since the
kernel side landed: both sides now say `PASS`.  Its *disposition* is
fixed either way (STAGE-21 adjudication O): it stays **out** of the
corpus, because the agreement is **hollow on the reference side** — the
reference side stores `pmpaddr` verbatim, so the case detects `G` = 0
and takes its own "trivially passes" early exit without ever running its
main test (STAGE-21 fact F21).

## Adding a case

1. Add the case to `corpus.txt` with its `march`, its `status` and its
   `env` variant:
   ```
   rv32mi scall rv32i_zicsr_zifencei active csr
   rv32ui add  rv32i_zifencei        active
   ```
   Use `pending` for anything M-13 does not implement yet — a `pending`
   case that starts agreeing is reported as `NOW-PASSING`, which is the
   signal to flip it to `active`.
   ⚠️ An **unknown** `env` value is a hard error, not a reason: a reason
   that accidentally lands in the env column would otherwise select the
   wrong environment silently.
2. Run `tools/golden-run.sh --filter <suite/test>`.
3. Machine-level cases need the `csr` environment; the supervisor-level
   ones need S-mode, which the platform does not have (**D-94**).
4. A case that upstream riscv-tests does not have goes into the
   `local` suite: put the source in `tests/golden/local/<test>.S` and
   write the row as `local <test> <march> active <env>`.

