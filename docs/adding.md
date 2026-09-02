# Adding things

Four recipes. Each is one implementation plus one line in a table; nothing in the scheduler,
the bus, or the front-ends changes.

## A peripheral

1. Write the model in `esp-periph/src/<name>.rs` (shared IP) or in the chip crate's `periph.rs`
   (chip-only). It owns its registers and implements `esp_periph::Device`:

   ```rust
   impl Device for Ledc {
       fn read(&mut self, off: u32) -> u32 { ... }
       fn write(&mut self, off: u32, v: u32) -> WriteEffect { ...; WriteEffect::NONE }
       fn irq_sources(&self) -> u64 { self.irq() as u64 }              // bit i = i-th source asserted
       fn clock(&self) -> Option<ClockDomain> { Some(ClockDomain::Apb) }   // if it keeps time
       fn tick(&mut self, ticks: u64) { ... }
       fn has_deadline(&self) -> bool { true }                          // if it can say when it fires next
       fn next_deadline(&self) -> Option<u64> { ... }                  // in its clock's ticks
       fn debug(&mut self, on: bool) { self.log = on; }                 // `--debug ledc`
   }
   ```
   Unhandled offsets go to a `RegRam` (read back what was written) — that is what an unmodelled
   register does today, so start from `--log-periph` and model only what the firmware polls.

2. Add a field to the chip's `Peripherals` and one line to its `device_set!` table:

   ```rust
   0x19 "LEDC" (ledc) => [SRC_LEDC];
   ```
   block number, name (also the `--debug` area), field, and the chip's source numbers in the
   order `irq_sources` numbers them. `@ lo..=hi` limits an entry to part of a block, `delta` shifts
   the offset the device sees, `alias` mounts a further block of the same device.

3. If it moves data through memory (a DMA engine, a frame source), the pump goes in the chip's
   `bus.rs` next to the I2S/LCD/camera ones; the device exposes what the pump needs.

4. If a reboot must keep some of its state (a strap, a JEDEC id, captured audio), copy it across
   in the chip's `SocBus::reboot`.

Check: `cargo test --release --workspace -- --include-ignored` unchanged, `--log-periph` no longer
lists the registers you modelled.

## A board

1. `impl esp_soc::BoardModel` in `esp32s3/src/board.rs` (or a new module). The SoC hands it pin
   edges (`gpio_changes`), RMT symbol streams (`rmt_frame`), SPI bytes (`spi_tx`), LCD frames
   (`lcd_frame`), and asks for camera frames; it offers the UI a display, LEDs, a camera preview,
   and the scripts named pins and an encoder. Its I2C devices (`impl esp_periph::i2c::I2cDevice`)
   are returned by `i2c_devices` as (bus, address, device).
2. One line in `make_board`.
3. A run script under `examples/` and a row in `docs/boards.md`.

The web page keys its layout on the board name (`web/index.html`); a board with a display and
nothing else shows as a bare module with a screen.

## A CPU and a chip

1. A core crate implementing `emu_core::Core` over `emu_core::Bus` (see `riscv-rv32/src/core.rs`
   for the minimal one: `step`, `set_irq`, `idle_advance`, the trace/dump surface). A fast path
   overrides `run` and stops at `bus.block_break()` so the machine can re-derive interrupt lines
   at the same instruction the slow path would.
2. A chip crate with: the memory map (`impl Bus for SocBus`), the `Peripherals` set (`device_set!`
   plus `impl DeviceSet`: block names, `pre_access` for the registers that depend on another
   device), the interrupt controller, and `soc.rs`: `impl Soc` (cores, `irqs` per core, secondary
   core control) and `impl SocBus` (console streams, reset, app boot, board, audio, interrupt
   routing, flash size, strap, reset cause, report).
3. `pub type Machine = esp_soc::Machine<C6>;` and a `machine()` constructor; a `setup_c6` in
   `cli/src/lib.rs` and a `--chip` arm; the same board name switch in `wasm/src/lib.rs`.

Everything else — scheduler, device time, console, scripts, stubs, observers, the web UI,
reboot, image loading — is `esp_soc::Machine` and needs no change.

## An observer

`impl esp_soc::Observer<S>` in `esp-soc/src/observers/`: say what you want (`Wants::BLOCK` runs at
full speed; `INSN` single-steps; `NO_IDLE_SKIP` changes timing — only ask for it if you count
instructions), implement the hooks, and produce a `report`. Register it with
`Machine::add_observer` from a CLI flag in `cli/src/lib.rs` and, if the page should have it, a
name in `MachineApi::observer` in `wasm/src/lib.rs`. `block_profile.rs` is the template.
