#include "xtensa/coreasm.h"

.section .iram1, "ax"
.align 4

.macro recursion name, call, pass
  .global \name
  .type \name, @function
\name:
  entry a1, 64
  beqz a2, 1f
  addi \pass, a2, -1
  \call \name
1:
  retw.n
  .size \name, . - \name
.endm

recursion call4_window_pair, call4, a6
.align 4
recursion call8_window_pair, call8, a10
.align 4
recursion call12_window_pair, call12, a14

.align 4
.global syscall_rfe_pair
.type syscall_rfe_pair, @function
syscall_rfe_pair:
  entry a1, 32
  rsr a8, vecbase
  movi a9, h1_vector_base
  wsr a9, vecbase
  rsync
  rsr a10, ccount
  syscall
  rsr a2, ccount
  sub a2, a2, a10
  wsr a8, vecbase
  rsync
  retw.n
  .size syscall_rfe_pair, . - syscall_rfe_pair

.align 4
.global rfe_alone
.type rfe_alone, @function
rfe_alone:
  entry a1, 32
  movi a7, 1f
  wsr a7, epc1
  rsr a2, ccount
  rfe
1:
  rsr a3, ccount
  sub a2, a3, a2
  retw.n
  .size rfe_alone, . - rfe_alone

.align 4
.global rfi3_alone
.type rfi3_alone, @function
rfi3_alone:
  entry a1, 32
  movi a7, 1f
  wsr a7, epc3
  rsr a6, ps
  wsr a6, eps3
  rsr a2, ccount
  rfi 3
1:
  rsr a3, ccount
  sub a2, a3, a2
  retw.n
  .size rfi3_alone, . - rfi3_alone

/* A private 1 KiB vector table used only while interrupts are masked. */
.p2align 10
.global h1_vector_base
.type h1_vector_base, @function
h1_vector_base:
  .space 0x300
  j exception_level1_handler
  .space 0x3d
  j exception_level1_handler
  .space 0x7d
  .size h1_vector_base, . - h1_vector_base

.global exception_level1_handler
.type exception_level1_handler, @function
exception_level1_handler:
  rsr a2, epc1
  _addi a2, a2, 3
  wsr a2, epc1
  rsync
  rfe
  .size exception_level1_handler, . - exception_level1_handler
