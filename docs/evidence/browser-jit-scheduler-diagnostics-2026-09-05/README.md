# Browser JIT scheduler eligibility with complete diagnostics

Allowing idle peers and scripts exposes known unsupported instructions in 2 Atech
attempts and 55 panel attempts, compared with 0 and 5 before that scheduler change.
No firmware quantum commits yet. Core 0 waiting remains the dominant limit at these
sampling instants; the complete masks retain its overlap with active-peer refusals.

All 15 machine tests passed, including comparisons with the ordinary scheduler for
idle-peer timing, timer wakeup, script stop, console draining and peer reset during a
quantum. Those tests exercise external execution independently of the firmware receipt.
The `Script` and `OtherCoreIdle` bits remain reserved so diagnostic bit numbers stay stable.

The statistics JSON retains every ticket counter, including zero values. Each preparation
has exactly one current outcome:

```text
prepared = committed + commitRejected + aborted + declined + superseded + pending
```

`aborted` counts module compilation/execution failures; `declined` counts a driver skipping
a prepared module (for example, a blacklisted ID). `superseded` counts a ticket replaced by
another preparation, and `pending` is 0 or 1. Calls without a ticket increment the separate
`commitWithoutTicket`, `abortWithoutTicket`, or `declineWithoutTicket` counter.

`rejections.schedulerMasks` counts complete concurrent refusal masks, with decimal keys.
The named per-reason totals are derived from those masks; they overlap and must not be
summed as attempts. `schedulerReasonBits` in the receipt maps each name to its bit.
`rejections.decode` distinguishes illegal/undecodable words from known unsupported opcodes.

The actual-WASM synthetic handoff test checks successful and rejected commits, aborts,
blacklisted-module declines, replacement of pending tickets, calls without tickets,
invalid decoding, concurrent refusals, and an actionable error for an older statistics ABI.
All five firmware runs passed their console assertions. [Raw output](run.log) and the
[full counters, source commit, WASM hash and input hashes](result.json) are retained.

Reproduce after `tools/wasm-build.sh`:

```sh
ESP32SIM_WASM_JIT_STATS=1 node tools/wasm-test.mjs hello atech atech-sid panel panel-sid
```

These runs measure eligibility at Node driver boundaries, not browser speed or the fraction
of workload instructions compiled. Each firmware run made 360 attempts and prepared zero
quanta, so this firmware evidence does not exercise successful external execution.
