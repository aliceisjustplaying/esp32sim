#include "xtensa/coreasm.h"

.section .iram1, "ax"
.align 4

/* Common state occupies 0 through 32, result occupies 36. */
.macro save_level1_state
  rsr a3, windowbase
  s32i a3, a1, 0
  rsr a3, windowstart
  s32i a3, a1, 4
  rsr a3, ps
  s32i a3, a1, 8
  rsr a3, epc1
  s32i a3, a1, 12
  rsr a3, excsave1
  s32i a3, a1, 16
  rsr a3, exccause
  s32i a3, a1, 20
  rsr a3, vecbase
  s32i a3, a1, 24
  rsr a3, sar
  s32i a3, a1, 28
  rsr a3, excsave2
  s32i a3, a1, 32
.endm

.macro restore_level1_state
  l32i a3, a1, 12
  wsr a3, epc1
  l32i a3, a1, 16
  wsr a3, excsave1
  l32i a3, a1, 20
  wsr a3, exccause
  l32i a3, a1, 24
  wsr a3, vecbase
  l32i a3, a1, 28
  wsr a3, sar
  l32i a3, a1, 32
  wsr a3, excsave2
  l32i a3, a1, 8
  wsr a3, ps
  rsync
  l32i a3, a1, 4
  wsr a3, windowstart
  rsync
.endm

.align 4
.global syscall_rfe_pair
.type syscall_rfe_pair, @function
syscall_rfe_pair:
  entry a1, 80
  save_level1_state
  movi a8, h2_syscall_vector_base
  wsr a8, vecbase
  rsync
  rsr a10, ccount
  syscall
  rsr a2, ccount
  sub a2, a2, a10
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size syscall_rfe_pair, . - syscall_rfe_pair

.align 4
.global rfe_alone
.type rfe_alone, @function
rfe_alone:
  entry a1, 80
  save_level1_state
  movi a7, 1f
  wsr a7, epc1
  rsync
  rsr a2, ccount
  rfe
1:
  rsr a3, ccount
  sub a2, a3, a2
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size rfe_alone, . - rfe_alone

.align 4
.global rfi3_alone
.type rfi3_alone, @function
rfi3_alone:
  entry a1, 96
  save_level1_state
  rsr a3, epc3
  s32i a3, a1, 48
  rsr a3, eps3
  s32i a3, a1, 52
  movi a7, 1f
  wsr a7, epc3
  l32i a6, a1, 8
  wsr a6, eps3
  rsync
  rsr a2, ccount
  rfi 3
1:
  rsr a3, ccount
  sub a2, a3, a2
  s32i a2, a1, 36
  l32i a3, a1, 48
  l32i a4, a1, 52
  wsr a3, epc3
  wsr a4, eps3
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size rfi3_alone, . - rfi3_alone

.macro window_return_probe name, instruction
  .align 4
  .global \name
  .type \name, @function
\name:
  entry a1, 80
  save_level1_state
  l32i a3, a1, 0
  l32i a5, a1, 8
  extui a6, a5, 0, 8
  extui a7, a5, 12, 8
  slli a7, a7, 12
  or a6, a6, a7
  slli a3, a3, 8
  or a6, a6, a3
  movi a3, 1f
  wsr a3, epc1
  wsr a6, ps
  rsync
  rsr a2, ccount
  \instruction
1:
  rsr a3, ccount
  sub a2, a3, a2
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size \name, . - \name
.endm

window_return_probe rfwo_alone, rfwo
window_return_probe rfwu_alone, rfwu

.macro make_window_bit output, index
  movi \output, 1
  ssl \index
  sll \output, \output
.endm

.macro require_overflow_shape reject
  rsr a5, windowbase
  addi a11, a5, 1
  extui a11, a11, 0, 4
  addi a12, a5, 2
  extui a12, a12, 0, 4
  addi a13, a5, 3
  extui a13, a13, 0, 4
  addi a14, a5, 4
  extui a14, a14, 0, 4
  make_window_bit a6, a11
  make_window_bit a7, a12
  make_window_bit a8, a13
  make_window_bit a9, a14
  rsr a10, windowstart
  or a11, a6, a7
  or a11, a11, a8
  and a3, a11, a10
  bnez a3, \reject
.endm

.macro require_underflow_shape reject
  rsr a5, windowbase
  addi a11, a5, -1
  extui a11, a11, 0, 4
  addi a12, a5, 1
  extui a12, a12, 0, 4
  addi a13, a5, 2
  extui a13, a13, 0, 4
  make_window_bit a6, a5
  make_window_bit a7, a11
  make_window_bit a8, a12
  make_window_bit a9, a13
  rsr a10, windowstart
  and a3, a6, a10
  beqz a3, \reject
  or a11, a7, a8
  or a11, a11, a9
  and a3, a11, a10
  bnez a3, \reject
.endm

.align 4
.global window_overflow8_entry
.type window_overflow8_entry, @function
window_overflow8_entry:
  entry a1, 80
  save_level1_state
  movi a5, h2_window_vector_base
  wsr a5, vecbase
  rsync
  require_overflow_shape 2f
  or a11, a7, a9
  or a10, a10, a11
  wsr a10, windowstart
  rsync
  rsr a2, ccount
  call8 h2_overflow_target
  rsr a3, excsave2
  sub a2, a3, a2
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
2:
  movi a2, 0
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size window_overflow8_entry, . - window_overflow8_entry

.align 4
.type h2_overflow_target, @function
h2_overflow_target:
  entry a1, 16
  rsr a2, ccount
  retw.n
  .size h2_overflow_target, . - h2_overflow_target

.align 4
.global window_overflow8_control
.type window_overflow8_control, @function
window_overflow8_control:
  entry a1, 80
  save_level1_state
  require_overflow_shape 3f
  rsr a2, ccount
  call8 h2_overflow_target
  sub a2, a10, a2
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
3:
  movi a2, 0
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size window_overflow8_control, . - window_overflow8_control

.align 4
.global window_underflow8_entry
.type window_underflow8_entry, @function
window_underflow8_entry:
  entry a1, 80
  save_level1_state
  movi a5, h2_window_vector_base
  wsr a5, vecbase
  rsync
  require_underflow_shape 5f
  movi a10, 1
  call8 h2_underflow_target
  rsr a3, excsave2
  sub a2, a3, a10
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
5:
  movi a2, 0
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size window_underflow8_entry, . - window_underflow8_entry

.align 4
.global window_underflow8_control
.type window_underflow8_control, @function
window_underflow8_control:
  entry a1, 80
  save_level1_state
  require_underflow_shape 6f
  movi a10, 0
  call8 h2_underflow_target
  rsr a3, ccount
  sub a2, a3, a10
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
6:
  movi a2, 0
  s32i a2, a1, 36
  restore_level1_state
  l32i a2, a1, 36
  retw.n
  .size window_underflow8_control, . - window_underflow8_control

.p2align 2
.type h2_underflow_target, @function
h2_underflow_target:
  entry a1, 32
  beqz a2, 4f
  rsr a2, windowbase
  addi a2, a2, -2
  extui a2, a2, 0, 4
  movi a3, 1
  ssl a2
  sll a3, a3
  rsr a2, windowstart
  movi a4, -1
  xor a3, a3, a4
  and a2, a2, a3
  wsr a2, windowstart
  rsync
4:
  rsr a2, ccount
  retw.n
  .size h2_underflow_target, . - h2_underflow_target

/* Private level-one vector table used only while interrupts are masked. */
.p2align 10
.global h2_syscall_vector_base
h2_syscall_vector_base:
  .space 0x300
  j h2_syscall_handler
  .space 0x3d
  j h2_syscall_handler
  .space 0xbd

.align 4
.global h2_syscall_handler
.type h2_syscall_handler, @function
h2_syscall_handler:
  rsr a2, epc1
  _addi a2, a2, 3
  wsr a2, epc1
  rsync
  rfe
  .size h2_syscall_handler, . - h2_syscall_handler

/* First-vector-instruction timestamps are copied to EXCSAVE2. */
.p2align 10
.global h2_window_vector_base
h2_window_vector_base:
  .space 0x80
.global h2_window_overflow8_vector
h2_window_overflow8_vector:
  rsr a2, ccount
  wsr a2, excsave2
  rsync
  rfwo
  .space 0xc0 - (. - h2_window_vector_base)
.global h2_window_underflow8_vector
h2_window_underflow8_vector:
  rsr a3, ccount
  wsr a3, excsave2
  rsync
  rfwu
  .space 0x400 - (. - h2_window_vector_base)
