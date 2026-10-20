// A deliberately-failing golden case.
//
// It is the harness's own mutation control: both the reference (Spike) and
// the candidate (our ISS) must report **FAIL 3** on this image.  If either
// side reported PASS — or if the two disagreed — the comparison would be
// vacuous (e.g. both sides always answering PASS), and the whole corpus
// run would be worthless.
//
// The file is deliberately self-contained: it does not include any
// riscv-tests source, so it keeps working even if the upstream tree
// changes shape.
//
//   TESTNUM = 3  ->  RVTEST_FAIL encodes tohost = (3 << 1) | 1 = 7
//                 ->  Spike's exit_code() = 7 >> 1 = 3
//
#include "riscv_test.h"

RVTEST_RV32U
RVTEST_CODE_BEGIN

  li TESTNUM, 3;
  RVTEST_FAIL

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
