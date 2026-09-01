.option rvc
.text
.global mandatory_start
.type mandatory_start, @function
mandatory_start:
    addi a0, zero, 42
    lw a1, 0(sp)
    sw a1, 4(sp)
    add a2, a0, a1
    mul a3, a2, a0
    beq a3, zero, mandatory_done
    csrr a4, mstatus
    nop
mandatory_done:
    ret
.size mandatory_start, .-mandatory_start
