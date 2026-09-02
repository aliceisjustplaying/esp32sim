/* Exact 32-bit CPU copy loop for one 32,768-byte display staging slot. */

    .section .iram1,"ax",@progbits
    .literal_position
    .align 4
    .global dma_sram_copy_32k
    .type dma_sram_copy_32k,@function
dma_sram_copy_32k:
    entry   a1, 16
    movi    a4, 1
    slli    a4, a4, 13
    rsr.ccount a6
.Lcopy_word:
    l32i    a5, a3, 0
    s32i    a5, a2, 0
    addi    a2, a2, 4
    addi    a3, a3, 4
    addi    a4, a4, -1
    bnez    a4, .Lcopy_word
    rsr.ccount a2
    sub     a2, a2, a6
    retw.n
    .size dma_sram_copy_32k, .-dma_sram_copy_32k
