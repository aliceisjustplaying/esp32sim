.section .iram1,"ax",@progbits
.literal_position

.macro read_block count
  .align 4
  .global register_read_\count
  .type register_read_\count,@function
register_read_\count:
  entry a1, 16
  .begin no-transform
  rsr.ccount a9
  .rept \count
  _l32i a8, a2, 0
  .endr
  rsr.ccount a2
  sub a2, a2, a9
  .end no-transform
  retw.n
  .size register_read_\count, .-register_read_\count
.endm

.macro write_block count
  .align 4
  .global register_write_\count
  .type register_write_\count,@function
register_write_\count:
  entry a1, 16
  .begin no-transform
  rsr.ccount a9
  .rept \count
  _s32i a3, a2, 0
  .endr
  rsr.ccount a2
  sub a2, a2, a9
  .end no-transform
  retw.n
  .size register_write_\count, .-register_write_\count
.endm

read_block 1
read_block 2
read_block 4
read_block 8
read_block 16
read_block 256

write_block 1
write_block 2
write_block 4
write_block 8
write_block 16
write_block 256
