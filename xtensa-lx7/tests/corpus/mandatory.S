.text
.global mandatory_start
.type mandatory_start, @function
mandatory_start:
    entry a1, 32
    movi a2, 42
    addi a3, a2, -1
    l32i a4, a1, 0
    s32i a4, a1, 4
    add a5, a3, a4
    xor a6, a5, a2
    beqz a6, mandatory_done
    nop
mandatory_done:
    retw.n
.size mandatory_start, .-mandatory_start
