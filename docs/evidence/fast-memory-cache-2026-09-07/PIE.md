# Selective PIE issue-cost probes

Adding a cycle only to 128-bit staging memory instructions moved hot linear
PIE staging from 10,609 to 12,887 guest microseconds. Adding two cycles only
to fused `EE.SRC.Q.LD` moved it to 12,785. Hardware recorded 12,640. Both
experiments kept linear scalar staging at 67,398 and passed the staging
checksums and guards. Neither closed the streaming gap: ring PIE remained
about 138 ms versus the recorded 165.815 ms hardware run.

See `receipts/pie-cost-comparison.json` and its referenced raw run receipts.
The comparison uses the initial lockstep approximate scheduler, cache fill
120 and writeback 96. These are sensitivity hypotheses, not measured opcode
prices. Tier B has no isolated PIE issue/throughput cell.

The actual linear cases all enter the unaligned assembly path. Their aligned
destination prefixes leave 22, 21, 15 and 7 vector blocks per iteration.
Across 2,048 iterations, the assembly therefore executes 540,672 selected
128-bit memory operations and 258,048 fused SRC-load operations. The two
hypotheses predict 2,252.8 and 2,150.4 additional microseconds at 240 MHz,
close to the observed increases of 2,278 and 2,176. Sources:

- TinyDraw `vector_v2/include/tinydraw/vector_v2/panel_staging.h:34`
- TinyDraw `esp32/main/vector_v2/panel_staging_esp32s3.S:39`
- TinyDraw `esp32/main/vector_v2/vector_v2_gate_harness_kernels.cpp:73`

The manual unaligned assembly loop also contains taken branches. The uniform
CPI=1 model does not price their pipeline cost, so attributing the entire gap
to PIE memory operations would be premature. The expected ring increase from
these selective costs is only several milliseconds; memory overlap still
needs a separate model.

Reproduction after building WASM with `--features cache-inline` and copying
the artifact to `web/wasm/esp32sim.wasm`:

```
PIE_TIMING=1 node tools/approximate-jit-smoke.mjs ASSETS.json 1 64 3 3
PIE_TIMING=2 node tools/approximate-jit-smoke.mjs ASSETS.json 1 64 3 3
cargo test -p xtensa-lx7 selective_pie_costs
```

The targeted test passed: selected successful instructions charge the chosen
penalty, ordinary SRC remains unchanged and a coprocessor trap adds no charge.
The prototype uses the existing batch-penalty drain without adding scheduler
state. Mode 1 deliberately routes compiled PIE memory operations through the
existing helper. Its host performance is not a proposed optimized design.
