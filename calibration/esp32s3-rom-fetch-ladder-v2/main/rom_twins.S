// Generated from rom-cells.json: byte-identical IRAM twins of ten mask-ROM
// bodies, each placed at the same address modulo 128 as its ROM original.
// objdump prints an encoding most-significant byte first; memory order is the
// reverse, so each encoding is emitted reversed. Do not edit by hand.

 .section .iram1.rom_twins, "ax"

 .balign 256
 .skip 36
 .global twin_p_none
 .type twin_p_none, @function
twin_p_none:  // ROM 0x400559a4, 5 bytes
  .byte 0x36, 0x21, 0x00  // 002136 entry a1, 16
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_p_none, . - twin_p_none

 .balign 256
 .skip 20
 .global twin_abs
 .type twin_abs, @function
twin_abs:  // ROM 0x4002e314, 8 bytes
  .byte 0x36, 0x41, 0x00  // 004136 entry a1, 32
  .byte 0x20, 0x21, 0x60  // 602120 abs a2, a2
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_abs, . - twin_abs

 .balign 256
 .skip 0
 .global twin_mulsi3
 .type twin_mulsi3, @function
twin_mulsi3:  // ROM 0x40056080, 8 bytes
  .byte 0x36, 0x21, 0x00  // 002136 entry a1, 16
  .byte 0x30, 0x22, 0x82  // 822230 mull a2, a2, a3
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_mulsi3, . - twin_mulsi3

 .balign 256
 .skip 40
 .global twin_clzsi2
 .type twin_clzsi2, @function
twin_clzsi2:  // ROM 0x400560a8, 8 bytes
  .byte 0x36, 0x21, 0x00  // 002136 entry a1, 16
  .byte 0x20, 0xf2, 0x40  // 40f220 nsau a2, a2
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_clzsi2, . - twin_clzsi2

 .balign 256
 .skip 112
 .global twin_temp_to_power
 .type twin_temp_to_power, @function
twin_temp_to_power:  // ROM 0x40055c70, 11 bytes
  .byte 0x36, 0x41, 0x00  // 004136 entry a1, 32
  .byte 0x30, 0x22, 0xc0  // c02230 sub a2, a2, a3
  .byte 0x20, 0x22, 0x74  // 742220 extui a2, a2, 2, 8
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_temp_to_power, . - twin_temp_to_power

 .balign 256
 .skip 100
 .global twin_roundup2
 .type twin_roundup2, @function
twin_roundup2:  // ROM 0x40056064, 15 bytes
  .byte 0x36, 0x41, 0x00  // 004136 entry a1, 32
  .byte 0x0b, 0x22  // 220b addi.n a2, a2, -1
  .byte 0x3a, 0x22  // 223a add.n a2, a2, a3
  .byte 0x30, 0x30, 0x60  // 603030 neg a3, a3
  .byte 0x30, 0x22, 0x10  // 102230 and a2, a2, a3
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_roundup2, . - twin_roundup2

 .balign 256
 .skip 48
 .global twin_ctzsi2
 .type twin_ctzsi2, @function
twin_ctzsi2:  // ROM 0x400560b0, 20 bytes
  .byte 0x36, 0x21, 0x00  // 002136 entry a1, 16
  .byte 0x20, 0x30, 0x60  // 603020 neg a3, a2
  .byte 0x20, 0x33, 0x10  // 103320 and a3, a3, a2
  .byte 0x20, 0xf3, 0x40  // 40f320 nsau a2, a3
  .byte 0x20, 0x20, 0x60  // 602020 neg a2, a2
  .byte 0x22, 0xc2, 0x1f  // 1fc222 addi a2, a2, 31
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_ctzsi2, . - twin_ctzsi2

 .balign 256
 .skip 68
 .global twin_ffssi2
 .type twin_ffssi2, @function
twin_ffssi2:  // ROM 0x400560c4, 20 bytes
  .byte 0x36, 0x21, 0x00  // 002136 entry a1, 16
  .byte 0x20, 0x30, 0x60  // 603020 neg a3, a2
  .byte 0x20, 0x33, 0x10  // 103320 and a3, a3, a2
  .byte 0x20, 0xf3, 0x40  // 40f320 nsau a2, a3
  .byte 0x20, 0x20, 0x60  // 602020 neg a2, a2
  .byte 0x22, 0xc2, 0x20  // 20c222 addi a2, a2, 32
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_ffssi2, . - twin_ffssi2

 .balign 256
 .skip 108
 .global twin_clzdi2
 .type twin_clzdi2, @function
twin_clzdi2:  // ROM 0x4005636c, 20 bytes
  .byte 0x36, 0x41, 0x00  // 004136 entry a1, 32
  .byte 0x30, 0x23, 0x93  // 932330 movnez a2, a3, a3
  .byte 0x0c, 0x09  // 090c movi.n a9, 0
  .byte 0x2c, 0x08  // 082c movi.n a8, 32
  .byte 0x30, 0x89, 0x93  // 938930 movnez a8, a9, a3
  .byte 0x20, 0xf2, 0x40  // 40f220 nsau a2, a2
  .byte 0x2a, 0x28  // 282a add.n a2, a8, a2
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_clzdi2, . - twin_clzdi2

 .balign 256
 .skip 0
 .global twin_ctzdi2
 .type twin_ctzdi2, @function
twin_ctzdi2:  // ROM 0x40056380, 30 bytes
  .byte 0x36, 0x41, 0x00  // 004136 entry a1, 32
  .byte 0x20, 0x32, 0x93  // 933220 movnez a3, a2, a2
  .byte 0x0c, 0x09  // 090c movi.n a9, 0
  .byte 0x2c, 0x08  // 082c movi.n a8, 32
  .byte 0x20, 0x89, 0x93  // 938920 movnez a8, a9, a2
  .byte 0x30, 0x20, 0x60  // 602030 neg a2, a3
  .byte 0x30, 0x22, 0x10  // 102230 and a2, a2, a3
  .byte 0x20, 0xf2, 0x40  // 40f220 nsau a2, a2
  .byte 0x20, 0x28, 0xc0  // c02820 sub a2, a8, a2
  .byte 0x22, 0xc2, 0x1f  // 1fc222 addi a2, a2, 31
  .byte 0x1d, 0xf0  // f01d retw.n
 .size twin_ctzdi2, . - twin_ctzdi2

