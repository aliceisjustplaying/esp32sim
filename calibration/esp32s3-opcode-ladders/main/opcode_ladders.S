/* ESP32-S3 LX7 opcode ladders. Every named body is in IRAM. */

.section .iram1, "ax"
.literal_position

.macro function_start name
  .align 4
  .global \name
  .type \name, @function
\name:
  entry a1, 32
.endm

.macro function_end name
  retw.n
  .size \name, . - \name
.endm

.macro body_start name
  j .L\name\()_body
  .align 4
  .global \name\()_body_start
\name\()_body_start:
.L\name\()_body:
  .begin no-transform
.endm

.macro body_end name
  .end no-transform
  .global \name\()_body_end
\name\()_body_end:
.endm

/* Condition emitters use a8 and a9. */
.macro emit_beqz target
  beqz a8, \target
.endm
.macro emit_bnez target
  bnez a8, \target
.endm
.macro emit_bltz target
  bltz a8, \target
.endm
.macro emit_bgez target
  bgez a8, \target
.endm
.macro emit_beqi target
  beqi a8, 1, \target
.endm
.macro emit_bnei target
  bnei a8, 1, \target
.endm
.macro emit_beq target
  beq a8, a9, \target
.endm
.macro emit_bne target
  bne a8, a9, \target
.endm
.macro emit_blt target
  blt a8, a9, \target
.endm
.macro emit_bge target
  bge a8, a9, \target
.endm
.macro emit_bltu target
  bltu a8, a9, \target
.endm
.macro emit_bgeu target
  bgeu a8, a9, \target
.endm
.macro emit_blti target
  blti a8, 1, \target
.endm
.macro emit_bgei target
  bgei a8, 1, \target
.endm
.macro emit_bltui target
  bltui a8, 2, \target
.endm
.macro emit_bgeui target
  bgeui a8, 2, \target
.endm
.macro emit_bany target
  bany a8, a9, \target
.endm
.macro emit_bnone target
  bnone a8, a9, \target
.endm
.macro emit_ball target
  ball a8, a9, \target
.endm
.macro emit_bnall target
  bnall a8, a9, \target
.endm
.macro emit_bbc target
  bbc a8, a9, \target
.endm
.macro emit_bbs target
  bbs a8, a9, \target
.endm
.macro emit_bbci target
  bbci a8, 0, \target
.endm
.macro emit_bbsi target
  bbsi a8, 0, \target
.endm
.macro emit_beqz_n target
  beqz.n a8, \target
.endm
.macro emit_bnez_n target
  bnez.n a8, \target
.endm

/* Taken branches skip padding so every static target is 4-byte aligned. */
.macro branch_taken name, emitter, value8, value9, narrow=0
  function_start \name
  _movi a8, \value8
  _movi a9, \value9
  body_start \name
  .rept 256
    \emitter 1f
    .if \narrow
      .byte 0, 0
    .else
      .byte 0
    .endif
1:
  .endr
  body_end \name
  function_end \name
.endm

/* Not-taken branches fall through a nop and a jump to the aligned target. */
.macro branch_not_taken name, emitter, value8, value9, narrow=0
  function_start \name
  _movi a8, \value8
  _movi a9, \value9
  body_start \name
  .rept 256
    \emitter 1f
    .if \narrow
      nop.n
    .else
      nop.n
    .endif
    j 1f
    .if \narrow
      .byte 0
    .endif
1:
  .endr
  body_end \name
  function_end \name
.endm

branch_taken opcode_beqz_taken, emit_beqz, 0, 0
branch_not_taken opcode_beqz_not_taken, emit_beqz, 1, 0
branch_taken opcode_bnez_taken, emit_bnez, 1, 0
branch_not_taken opcode_bnez_not_taken, emit_bnez, 0, 0
branch_taken opcode_bltz_taken, emit_bltz, -1, 0
branch_not_taken opcode_bltz_not_taken, emit_bltz, 0, 0
branch_taken opcode_bgez_taken, emit_bgez, 0, 0
branch_not_taken opcode_bgez_not_taken, emit_bgez, -1, 0
branch_taken opcode_beqi_taken, emit_beqi, 1, 0
branch_not_taken opcode_beqi_not_taken, emit_beqi, 0, 0
branch_taken opcode_bnei_taken, emit_bnei, 0, 0
branch_not_taken opcode_bnei_not_taken, emit_bnei, 1, 0
branch_taken opcode_beq_taken, emit_beq, 1, 1
branch_not_taken opcode_beq_not_taken, emit_beq, 0, 1
branch_taken opcode_bne_taken, emit_bne, 0, 1
branch_not_taken opcode_bne_not_taken, emit_bne, 1, 1
branch_taken opcode_blt_taken, emit_blt, 0, 1
branch_not_taken opcode_blt_not_taken, emit_blt, 1, 0
branch_taken opcode_bge_taken, emit_bge, 1, 0
branch_not_taken opcode_bge_not_taken, emit_bge, 0, 1
branch_taken opcode_bltu_taken, emit_bltu, 0, 1
branch_not_taken opcode_bltu_not_taken, emit_bltu, 1, 0
branch_taken opcode_bgeu_taken, emit_bgeu, 1, 0
branch_not_taken opcode_bgeu_not_taken, emit_bgeu, 0, 1
branch_taken opcode_blti_taken, emit_blti, 0, 0
branch_not_taken opcode_blti_not_taken, emit_blti, 1, 0
branch_taken opcode_bgei_taken, emit_bgei, 1, 0
branch_not_taken opcode_bgei_not_taken, emit_bgei, 0, 0
branch_taken opcode_bltui_taken, emit_bltui, 1, 0
branch_not_taken opcode_bltui_not_taken, emit_bltui, 2, 0
branch_taken opcode_bgeui_taken, emit_bgeui, 2, 0
branch_not_taken opcode_bgeui_not_taken, emit_bgeui, 1, 0
branch_taken opcode_bany_taken, emit_bany, 1, 1
branch_not_taken opcode_bany_not_taken, emit_bany, 0, 1
branch_taken opcode_bnone_taken, emit_bnone, 0, 1
branch_not_taken opcode_bnone_not_taken, emit_bnone, 1, 1
branch_taken opcode_ball_taken, emit_ball, 1, 1
branch_not_taken opcode_ball_not_taken, emit_ball, 0, 1
branch_taken opcode_bnall_taken, emit_bnall, 0, 1
branch_not_taken opcode_bnall_not_taken, emit_bnall, 1, 1
branch_taken opcode_bbc_taken, emit_bbc, 0, 0
branch_not_taken opcode_bbc_not_taken, emit_bbc, 1, 0
branch_taken opcode_bbs_taken, emit_bbs, 1, 0
branch_not_taken opcode_bbs_not_taken, emit_bbs, 0, 0
branch_taken opcode_bbci_taken, emit_bbci, 0, 0
branch_not_taken opcode_bbci_not_taken, emit_bbci, 1, 0
branch_taken opcode_bbsi_taken, emit_bbsi, 1, 0
branch_not_taken opcode_bbsi_not_taken, emit_bbsi, 0, 0
branch_taken opcode_beqz_n_taken, emit_beqz_n, 0, 0, 1
branch_not_taken opcode_beqz_n_not_taken, emit_beqz_n, 1, 0, 1
branch_taken opcode_bnez_n_taken, emit_bnez_n, 1, 0, 1
branch_not_taken opcode_bnez_n_not_taken, emit_bnez_n, 0, 0, 1

/* Unconditional jump ladders. */
function_start opcode_j
body_start opcode_j
.rept 256
  j 1f
  .byte 0
1:
.endr
body_end opcode_j
function_end opcode_j

function_start opcode_jx
  j .Lopcode_jx_body
  .align 4
  .global opcode_jx_body_start
opcode_jx_body_start:
.Lopcode_jx_body:
.rept 256
  movi a8, 2f
  jx a8
  .align 4
2:
.endr
  .global opcode_jx_body_end
opcode_jx_body_end:
function_end opcode_jx

/* Call ladders execute 256 call and return pairs without window overflow. */
function_start opcode_call0_ret
  _or a12, a0, a0
body_start opcode_call0_ret
.rept 256
  call0 opcode_call0_leaf
.endr
body_end opcode_call0_ret
  _or a0, a12, a12
function_end opcode_call0_ret

.align 4
.type opcode_call0_leaf, @function
opcode_call0_leaf:
  ret
.size opcode_call0_leaf, . - opcode_call0_leaf

function_start opcode_callx0_ret
  _or a12, a0, a0
  l32r a8, opcode_callx0_leaf_literal
body_start opcode_callx0_ret
.rept 256
  callx0 a8
.endr
body_end opcode_callx0_ret
  _or a0, a12, a12
function_end opcode_callx0_ret
.literal opcode_callx0_leaf_literal, opcode_callx0_leaf
.align 4
.type opcode_callx0_leaf, @function
opcode_callx0_leaf:
  ret
.size opcode_callx0_leaf, . - opcode_callx0_leaf

function_start opcode_call8_retw
body_start opcode_call8_retw
.rept 256
  call8 opcode_call8_leaf
.endr
body_end opcode_call8_retw
function_end opcode_call8_retw

.align 4
.type opcode_call8_leaf, @function
opcode_call8_leaf:
  entry a1, 16
  retw.n
.size opcode_call8_leaf, . - opcode_call8_leaf

function_start opcode_callx8_retw
  l32r a4, opcode_callx8_leaf_literal
body_start opcode_callx8_retw
.rept 256
  callx8 a4
.endr
body_end opcode_callx8_retw
function_end opcode_callx8_retw
.literal opcode_callx8_leaf_literal, opcode_callx8_leaf
.align 4
.type opcode_callx8_leaf, @function
opcode_callx8_leaf:
  entry a1, 16
  retw.n
.size opcode_callx8_leaf, . - opcode_callx8_leaf

/* Loop setup ladders use a one-instruction body per setup. */
.macro loop_ladder name, instruction
  function_start \name
  _movi a8, 1
  j .L\name\()_body
  .align 4
  .byte 0
  .global \name\()_body_start
\name\()_body_start:
.L\name\()_body:
  .begin no-transform
  .rept 256
    \instruction a8, 1f
    nop.n
1:
    j 2f
2:
  .endr
  body_end \name
  function_end \name
.endm

loop_ladder opcode_loop, loop
loop_ladder opcode_loopnez, loopnez
loop_ladder opcode_loopgtz, loopgtz

/* Straight-line 256-op ladders with independent source operands. */
.macro simple_ladder name, instruction:vararg
  function_start \name
  _movi a9, 291
  _movi a10, 7
  body_start \name
  .rept 256
    \instruction
  .endr
  body_end \name
  function_end \name
.endm

simple_ladder opcode_issue_nop_baseline, nop.n
simple_ladder opcode_mull, mull a8, a9, a10
simple_ladder opcode_mulsh, mulsh a8, a9, a10
simple_ladder opcode_muluh, muluh a8, a9, a10
simple_ladder opcode_quos, quos a8, a9, a10
simple_ladder opcode_quou, quou a8, a9, a10
simple_ladder opcode_rems, rems a8, a9, a10
simple_ladder opcode_remu, remu a8, a9, a10
simple_ladder opcode_nsa, nsa a8, a9
simple_ladder opcode_nsau, nsau a8, a9
simple_ladder opcode_sext, sext a8, a9, 7

.literal opcode_l32r_value, 0x12345678
simple_ladder opcode_l32r, l32r a8, opcode_l32r_value

function_start opcode_s32c1i
  _movi a8, 7
  wsr.scompare1 a8
  rsync
body_start opcode_s32c1i
.rept 256
  s32c1i a8, a2, 0
.endr
body_end opcode_s32c1i
function_end opcode_s32c1i

simple_ladder opcode_memw, memw
simple_ladder opcode_extw, extw
simple_ladder opcode_rsr, rsr.sar a8
simple_ladder opcode_wsr, wsr.sar a9
simple_ladder opcode_xsr, xsr.sar a8
simple_ladder opcode_rsync, rsync
simple_ladder opcode_isync, isync
simple_ladder opcode_movsp, movsp a8, a1
simple_ladder opcode_min, min a8, a9, a10
simple_ladder opcode_max, max a8, a9, a10
simple_ladder opcode_minu, minu a8, a9, a10
simple_ladder opcode_maxu, maxu a8, a9, a10

function_start load_use_distance_1
body_start load_use_distance_1
.rept 256
  l32i a8, a2, 0
  add a9, a8, a10
.endr
body_end load_use_distance_1
function_end load_use_distance_1

function_start load_use_distance_2
body_start load_use_distance_2
.rept 256
  l32i a8, a2, 0
  or a11, a10, a10
  add a9, a8, a10
.endr
body_end load_use_distance_2
function_end load_use_distance_2
