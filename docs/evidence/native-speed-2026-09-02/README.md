# Native product boot speed, 2026-09-02

This historical blocked result was resolved after the fast-mode small SPI2 DMA
completion fix. The five-run READY-qualified rerun is recorded in
[`../native-speed-2026-09-03/`](../native-speed-2026-09-03/README.md).

The Task 4 native release fast-mode check was blocked at this commit. The
authorized TinyDraw product image emitted no `TINYDRAW_VECTOR_V2_READY` marker
in five bounded ROM boots, including a final 16,000,000,000-cycle run covering
66.667 emulated seconds and 162.15 wall seconds.

The exact image is TinyDraw `fc6d9347549730a0e57aa926f8f6935e12636844`,
ESP-IDF v6.1, from the committed Tier B archive reference. Its physical serial
receipt reaches READY. All four tested image hashes match
`../timing/tier-b-2026-09-01/archive-reference.json`.

Every emulator run reached `co5300_spi: LCD panel create success` and then made
no further product-log progress. The five command processes exited 0 only
because each reached its requested `MaxInsns` ceiling.

| Run | Cycle ceiling | Emulated seconds | Wall seconds | Retired instructions | MIPS | READY |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 1,000,000,000 | 4.167 | 8.66 | 968,962,542 | 111.889 | no |
| 2 | 2,000,000,000 | 8.333 | 17.53 | 1,974,192,724 | 112.618 | no |
| 3 | 4,000,000,000 | 16.667 | 35.75 | 3,984,651,827 | 111.459 | no |
| 4 | 8,000,000,000 | 33.333 | 70.43 | 8,005,571,295 | 113.667 | no |
| 5 | 16,000,000,000 | 66.667 | 162.15 | 16,047,408,970 | 98.966 | no |

The median bounded pre-READY rate was 111.889 MIPS. The maximum was 113.667
MIPS, 23.681 percent of the 480 MIPS budget. These are pre-READY observations,
not completed product-boot throughput measurements. There is no JIT claim.

`result.json` records the machine, commit, binary and image hashes, command,
per-core retired counts, wall times, marker result, and acceptance blocker.
