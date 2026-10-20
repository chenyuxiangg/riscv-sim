/* Linker script for the golden corpus.
 *
 * Layout mirrors upstream riscv-tests' `env/p/link.ld`:
 *
 *   0x80000000  .text.init   reset vector + test body
 *   (page)      .tohost      HTIF handshake words
 *   (page)      .text        (unused by the current corpus)
 *   (page)      .data        test data
 *               .bss
 *
 * The `.tohost` section is page-aligned away from `.text.init` so that
 * the HTIF words never share a page with executable code, and so that
 * `tohost` has a stable, predictable address for every test in the
 * corpus.
 */

OUTPUT_ARCH( "riscv" )
ENTRY(_start)

SECTIONS
{
  . = 0x80000000;
  .text.init : { *(.text.init) }
  . = ALIGN(0x1000);
  .tohost : { *(.tohost) }
  . = ALIGN(0x1000);
  .text : { *(.text) }
  . = ALIGN(0x1000);
  .data : { *(.data) }
  .bss : { *(.bss) }
  _end = .;
}
