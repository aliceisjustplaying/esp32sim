# Timed two-core experiment: correct fixture, below real time

The experiment matched the measured interpreter in 84 short reference comparisons per browser process and caught four deliberately broken variants. Across three fresh Chrome processes, all 15 one-billion-instruction speed samples were below real time: median **0.98298×**, range **0.97587–0.99337×**. These long samples check derived final-state expectations; they are not independent billion-instruction reference comparisons. [Speed summary](speed-summary.json) · [Process 1](speed-1.json) · [Process 2](speed-2.json) · [Process 3](speed-3.json)

This is a small two-core internal-RAM fixture with a synthetic device event. It does not establish full-product accuracy or performance. The feature-gated prototype remains an experiment; no production scheduler or timing price was changed. See the [specification and reproduction commands](../../../wasm-jit/examples/timed_pair/README.md).

Profiling the unchanged module located native execution inside its compiled function: 7,198 of 7,271 native ticks were within that function's address range. Chrome did not print instruction disassembly or source-position mappings, so the samples cannot rank scheduling, dispatch or state-update costs. DevTools separately assigned most samples to a wrapper, which does not establish wrapper overhead. Instrumented elapsed times are not new speed evidence. No optimization resulted. [Profile summary](profile-summary.json) · [DevTools run](profile-1.json) · [Native-only run](profile-native.json)

The host was an Apple M1 Pro running Chrome 152.0.7977.77 and V8 15.2.124.19. The generated module is 5,888 bytes with SHA-256 `5a0d2be5b4e176c8790e6e62723003b0af7976b3e5a6f24c5d3a10a8079e8240`; per-run results pin the sources and environment. [Speed summary](speed-summary.json) · [Input manifest](MANIFEST.json)

The [raw evidence capsule](https://github.com/aliceisjustplaying/esp32sim/releases/download/timed-two-core-evidence-2026-09-06/timed-two-core-evidence.tar.gz) contains the original speed results, generated fixture modules, source snapshots and profiles under their original worktree paths. Browser user-data directories are excluded. Capsule size: 3,893,251 bytes. SHA-256: `7b07ea5756ff163c397ade4f4b2e9d2e8755eb308f1e6835b08900bff7ea1a0c`. The [manifest](MANIFEST.json) records each included file's hash and the source revision. The compact copies here retain the result JSON unchanged.

After generating the fixture using the specification, reproduce profiling from the worktree root:

```sh
node docs/evidence/timed-two-core-2026-09-06/profile.mjs work/timed-pair-profile/run-1
node docs/evidence/timed-two-core-2026-09-06/profile.mjs work/timed-pair-profile/run-native --native-only
cp docs/evidence/timed-two-core-2026-09-06/summarize.mjs work/timed-pair-profile/summarize.mjs
node work/timed-pair-profile/summarize.mjs
```

The scripts use the installed macOS Chrome path and Node's built-in WebSocket. The first-run script is preserved separately because it predates the native-only option. [Profiling script](profile.mjs) · [Original script](profile-first-run.mjs) · [Summary script](summarize.mjs)
