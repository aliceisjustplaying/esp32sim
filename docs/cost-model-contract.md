# Cost model contract proposal

Status: draft for discussion on issue #6. This document defines the contract before changing execution behavior.

## Scope

The hook change belongs in one PR. It supplies complete execution facts, advances cores and devices from accepted model cycles, and stops on an unpriced event. The silicon model, AMOLED board, probe firmware, and boot correlation test remain separate work.

With no model attached, `Machine::run` keeps its existing block and JIT path byte-for-byte. The model check occurs once when selecting the run path, not inside the no-model block loop.

## Proposed facts

The public types remain next to `CostModel` in `emu-core` because they describe what any core exposes to a machine. This is an illustrative shape; the named operation enums and read-only snapshot interface are part of the review:

```rust
pub enum MemoryAccessKind { Fetch, Read, Write }

pub struct MemoryAccess {
    pub kind: MemoryAccessKind,
    pub address: u32,
    pub width: u8,
    pub value: u32,
    pub fault: Option<Fault>,
}

pub enum ControlEvent {
    Cache { operation: CacheOperation, address: u32 },
    Tlb { operation: TlbOperation, address: u32 },
}

pub enum ExecutionOutcome {
    Retired,
    TrapBeforeInstruction(Trap),
    TrapDuringInstruction(Trap),
}

pub struct ExecutionFacts<'a> {
    pub core: usize,
    pub pc: u32,
    pub next_pc: u32,
    pub bytes: Option<[u8; 4]>,
    pub length: u8,
    pub outcome: ExecutionOutcome,
    pub accesses: &'a [MemoryAccess],
    pub control: &'a [ControlEvent],
}

pub enum LifecycleKind { Attach, ChipReset, CoreReset(usize) }

pub struct LifecycleFacts<'a> {
    pub kind: LifecycleKind,
    pub chip: &'static str,
    pub cores: usize,
    pub cpu_hz: u64,
    // SoC-owned cache, MMU, clock, flash, and PSRAM configuration snapshot.
    // Its concrete read-only shape is one of the decisions below.
    pub configuration: &'a dyn ConfigurationSnapshot,
}

pub trait CostModel {
    fn lifecycle(&mut self, facts: &LifecycleFacts<'_>) -> Result<(), String>;
    fn cycles(&mut self, facts: &ExecutionFacts) -> Result<u32, String>;
}
```

The machine rejects a returned cost of zero. Every model error names the unsupported cost or configuration.

`bytes` contains the four-byte fetch window and `length` identifies the decoded instruction length. When `bytes` is `None`, `length` is zero. This includes an interrupt accepted before execution.

Accesses are in CPU program order. They include one conceptual instruction fetch plus every ordinary load, store, MMIO access, and MMU-table access made by that execution event. Reads and fetches carry the returned value. Writes carry the attempted value. Faults retain the attempted address, width, kind, and value.

MMU-table and clock/configuration writes that use the bus reach the model as ordinary typed accesses. Cache-control and core TLB operations that do not touch the bus are explicit control events carrying the effective address and operation. Instruction bytes alone are insufficient because the address may come from a register. Attach, chip reset, and secondary-core reset or release use explicit lifecycle events.

The lifecycle snapshot must cover cache geometry and enable state, MMU state, clocks, flash mode and frequency, PSRAM mode and frequency, and other configuration required to select receipted costs. This also covers a model attached after prior execution, after memory resizing, or after host-applied configuration. Attaching only to a pristine machine is an acceptable initial limitation if it is explicit. Synthetic `boot_app` cannot be supported without either the snapshot or that limitation.

## Capturing accesses

The modeled path single-steps through a private `RecordingBus` in `esp-soc::Machine`. It delegates data-access `Bus` calls and records only calls made by the CPU. Device and DMA activity outside `Core::step` is not mislabeled as CPU traffic.

The richer core outcome must return the actual decoded bytes, length, trap timing, and explicit cache/TLB control events. A bus recorder cannot produce one conceptual fetch reliably because an LX7 decode-cache hit makes no `Bus::fetch` call, and a separate machine prefetch can invent or duplicate an access. The facts contain one conceptual fetch for every fetched instruction event, but none for a pre-instruction interrupt. The existing `Misc::mmio_log` remains observer-owned and unchanged; it omits ordinary memory and MMU-table accesses and may be drained by `regstat`.

The present `Core::step` result does not return decoded bytes and does not say whether a trap happened before or during an instruction. A complete implementation must enrich the core outcome enough to populate `ExecutionOutcome`. Inferring retirement from PC or instruction counters is not a contract.

The core contract also needs a timing-only `advance_cycles` operation. The current `idle_advance` cannot serve this purpose generically because the RV32 implementation increments its instruction count.

## Shared time

Modeled execution uses `ready_at[core]`, the shared-cycle timestamp when each core may start its next instruction, plus the bus's single device-time horizon.

1. Select an active core at the smallest `ready_at`, with core index as the deterministic tie break.
2. Advance devices to that start timestamp and expose crossed deadlines and interrupt changes.
3. Execute one event, collect facts, and ask the model for its cost.
4. Require a cost of at least one, use the timing-only core operation to advance its cycle counter by the accepted difference from baseline, and set its next `ready_at` to `start + cost`.
5. Advance the device horizon only to the smallest active frontier, then select again.

A held core is excluded. A newly released or reset core starts at the current device horizon. Whenever a waiting core could be woken before the next runnable-core frontier, modeled execution needs the exact next device deadline. The scheduler advances to the earlier of that deadline and the next runnable frontier, refreshes interrupts, and selects again. A waiting core must not freeze time or be skipped past its wakeup. This requires a modeled-mode deadline API from `SocBus`; the existing idle chunk remains unchanged only on the no-model path.

The existing 64-instruction round algorithm is not used with a model. Running all of core 0 before core 1 either exposes core 1 to core 0's future or delays device events until the end of the round.

## Atomicity and refusal

An after-execution hook cannot roll back registers, RAM, MMIO, board output, or trap state. A direct implementation therefore applies architectural and bus effects at the event's start timestamp, while the accepted cost determines when that core may start its next event.

This convention does not defer shared RAM or MMIO effects until the instruction's completion frontier. For example, a core-0 MMIO write started at cycle 0 and priced at 10 can be observed by core 1 before cycle 10. If completion-time visibility is required, execution needs a planning or transactional phase that can defer shared effects. The current APIs cannot provide that. Maintainer acceptance of start-time effects is therefore a prerequisite, not an implementation detail.

Same-timestamp events use core-index order. This is deterministic and must be treated as part of the contract.

Refusal stops immediately after the event's effects:

```rust
Stop::CostModel { core: usize, pc: u32, reason: String }
```

`pc` is the event's starting instruction PC. The core's current PC and all architectural, memory, MMIO, and trap effects remain committed. No model-added cycles, later core event, or later device-time advance is committed. The reason is preserved exactly.

Attach-time refusal should make `set_cost_model` return its error. A chip-reset or core-reset refusal needs a separate pending stop without an instruction PC, for example `Stop::CostModelLifecycle { kind, reason }`, delivered before another execution event. The final variant shape is a requested decision.

Existing stop precedence must also be explicit. The proposal is that an emulator-level terminal stop (`Unimplemented`, `Simcall`, or unhandled `Ebreak`) wins and is not replaced by a model refusal. Architectural exceptions and accepted interrupts reach the model as priced execution outcomes. A model refusal on one of those outcomes retains the trap effects and returns `Stop::CostModel`.

General rollback is not implementable with the current mutable `SocBus` and `BoardModel` APIs.

## Contract tests

The implementation PR must add executable tests for:

- a fetch followed by an ordinary SRAM load and store, preserving address, width, value, fault, and order;
- MMIO and MMU-table writes, including the written configuration value;
- attach, chip-reset, and secondary-core reset or release events, including the configuration snapshot or the pristine-attachment refusal;
- attach-time and reset-time refusal, with no later execution;
- an interrupt accepted before instruction fetch, with no invented instruction access;
- a trapping instruction, preserving its bytes, accesses, starting PC, and trap outcome;
- refusal after a register, RAM, and MMIO write, proving the side effect remains, the starting PC and reason are retained, and no later event or device tick runs;
- a zero-cycle model result, which is rejected;
- two released cores with costs 10 and 1, proving core 1 receives horizons 0 through 9 before core 0 runs again at 10;
- a timer or scripted input at cycle 5, invisible through cycle 4 and visible starting at cycle 5;
- a core released after time advanced, whose first event starts at the current horizon;
- unchanged no-model observer output and goldens;
- defined modeled observer behavior, including whether block observers are unavailable on the single-step path;
- precedence between model refusal and every existing terminal stop;
- byte-identical no-model goldens and benchmark results within noise.

## Decisions requested

1. Is the eager-effect, next-ready-time atomicity rule acceptable for modeled execution?
2. Is start-time visibility of shared RAM and MMIO effects acceptable, or is a planning or transactional execution phase required?
3. May `Core::step` gain a richer outcome carrying decoded bytes, length, trap timing, and cache/TLB control events without moving the model into the core?
4. What read-only configuration snapshot should `Soc` provide at attach and reset?
5. Should reset refusal use a separate stop variant, and is the proposed terminal-stop precedence correct?
6. What exact next-deadline API should `SocBus` expose for modeled execution when any waiting core could wake before the next runnable frontier?
7. Which observer callbacks should modeled single-step execution provide, especially for block observers?
