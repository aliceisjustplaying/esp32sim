.text
.global integer_case
.type integer_case, @function
integer_case:
    movi a2, 42
    movi a3, -17
    add a4, a2, a3
    sub a5, a2, a3
    and a6, a4, a5
    or a7, a4, a5
    xor a8, a6, a7
    neg a9, a8
    abs a10, a3
    addx2 a11, a2, a3
    subx4 a12, a2, a3
    mull a13, a4, a5
    muluh a14, a4, a5
    mulsh a15, a3, a5
    nop
.size integer_case, .-integer_case

.global shift_case
.type shift_case, @function
shift_case:
    movi a2, 0x135
    movi a3, -31
    ssai 7
    sll a4, a2
    srl a5, a2
    sra a6, a3
    src a7, a2, a3
    slli a8, a2, 3
    srli a9, a2, 5
    srai a10, a3, 4
    extui a11, a2, 3, 9
    sext a12, a3, 7
    nsau a13, a2
    nop
.size shift_case, .-shift_case

.global memory_case
.type memory_case, @function
memory_case:
    l8ui a2, a1, 0
    l16ui a3, a1, 2
    l16si a4, a1, 4
    l32i a5, a1, 8
    s8i a5, a1, 12
    s16i a4, a1, 14
    s32i a3, a1, 16
    l32i.n a6, a1, 16
    s32i.n a6, a1, 20
    add a7, a2, a3
    xor a8, a6, a7
    nop
.size memory_case, .-memory_case

.global compare_case
.type compare_case, @function
compare_case:
    min a2, a3, a4
    max a5, a3, a4
    minu a6, a3, a4
    maxu a7, a3, a4
    moveqz a8, a3, a5
    movnez a9, a4, a5
    movltz a10, a3, a4
    movgez a11, a4, a3
    nop
.size compare_case, .-compare_case
