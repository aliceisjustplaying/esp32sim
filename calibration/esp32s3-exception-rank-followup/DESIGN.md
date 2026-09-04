# H2 exception and ROM-fetch design

H1 identifies only syscall entry among the entry classes. Its exact window
receipt is `35 = E_wo8 + 9 + rfwo + E_wu8 + 9 + rfwu`, so it constrains
`E_wo8 + E_wu8 + rfwo + rfwu = 17`. Equal entry prices are not assumed.

H2 has seven exception unknown columns: `rfe`, `rfi3`, syscall entry,
WindowOverflow8 entry, WindowUnderflow8 entry, `rfwo`, and `rfwu`. After the
verified known terms are removed, the seven rows in `design-proof.json` form
an exact rank-seven matrix with determinant one. Subtracting the `rfe` row
from the syscall row exposes syscall entry. Removing any column reduces the
rank to six.

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
start timestamp, while the WindowUnderflow8 vector copies its first-instruction
a3 timestamp to EXCSAVE2. The outer cell reads that endpoint after `rfwu`, then
restores EXCSAVE2.

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
is the unused validation set. Before exception adoption, the measured engine
must reproduce raw totals 6 for `rfe`, 5 for `rfi 3`, and 18 for syscall plus
`rfe`, and the four H2 window candidates must sum to 17. The H2 receipt must
contain two clean IDF 6.1 boots, 100 accepted samples for every exception
cell, zero refusals, exact constant direct totals and matched differences
across boots, committed archive hashes, and verified ELF row reconstruction.

Mask-ROM fetch remains an exact-tier refusal. A matched ROM and IRAM pair with
the minimal WOE=1 safe-window predicate refused in the emulator before target
execution. An assumption-free price therefore needs a dedicated wrapper that
controls and restores WINDOWSTART. That machinery is outside this minimal
exception adoption slice, and no ROM row is queued.
