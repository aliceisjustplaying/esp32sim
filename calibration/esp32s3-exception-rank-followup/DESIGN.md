# H2 exception and ROM-fetch design

H1 identifies only syscall entry among the entry classes. Its exact window
receipt is `35 = E_wo8 + 9 + rfwo + E_wu8 + 9 + rfwu`, so it constrains
`E_wo8 + E_wu8 + rfwo + rfwu = 17`. Equal entry prices are not assumed.

H2 has eight unknown columns: `rfe`, `rfi3`, syscall entry,
WindowOverflow8 entry, WindowUnderflow8 entry, `rfwo`, `rfwu`, and mask-ROM
fetch delay `F`. After the verified known terms are removed, the eight rows
in `design-proof.json` form an exact rank-eight identity-plus-syscall matrix.
Removing any column reduces the rank to seven.

The ELF verifier reconstructs those rows from the executable. It requires the
CCOUNT instruction adjacent to each return or trigger, matched no-exception
controls for the faulting `entry` and `retw.n`, exact handler bodies,
all return paths to restore the captured special-register state, and the
sample loop to reject cache or state deltas. The build runs the paper proof,
then exact ELF verification runs the same proof on the reconstructed rows.
Level 1 has EPC1 and PS state, including PS.EXCM and PS.OWB, but no EPS1.
The level-three return separately snapshots and restores EPC3 and EPS3.

Both underflow cells refuse unless WINDOWSTART has B set and B-1, B+1, and
B+2 clear. After `call8`, the trigger target AND-clears its `(WINDOWBASE - 2)`
bit, which is the caller's B bit, immediately before `retw.n`. Its a2 holds the
start timestamp, while the WindowUnderflow8 vector writes the matched endpoint
to a3 as its first instruction. The outer cell restores EXCSAVE2 after copying
the endpoint there.

Both overflow cells refuse unless WINDOWSTART B+1 through B+3 are clear. The
entry cell sets B+2 and B+4, which forces the call8 target's `entry` to the
WindowOverflow8 vector at VECBASE+0x80. The matched control leaves those bits
clear and executes the same target. These differences measure the typed
trigger-to-first-vector-CCOUNT residuals, not an unqualified common entry
latency.

H2 derives candidates from its new rows. It does not use H1 to solve them.
The pinned H1 receipt at commit
`c6c0d5af528f0988004b7f77427a9259d9d2db3a`, summary SHA-256
`511dd814024a7385dc2185f9f155819802c8e81e913568307c311262b541a613`,
and probe source commit `75778a4cfef4332b09b7e0595d36fde188d0c118`
is the unused validation set. H2 must reproduce raw totals 6 for `rfe`, 5 for
`rfi 3`, 18 for syscall plus `rfe`, and 15 for the ROM target. The four H2
window candidates must also sum to 17. Candidates remain unadopted until a
committed H2 hardware receipt passes all five validation checks.

The ROM and IRAM targets are the same aligned five bytes,
`entry a1, 16; retw.n`, and use one measurement wrapper. The wrapper clears
PS.WOE during both timed calls, snapshots and restores PS, WINDOWBASE, and
WINDOWSTART, and rejects every state delta. No window exception can enter the
matched interval. With cache-counter deltas also required to be zero,
`ROM raw - IRAM raw = 2F` has no unmatched wrapper or window term.
