import assert from 'node:assert/strict';
import { createPacing } from '../web/wasm/pacing.mjs';

const p = createPacing();
assert.equal(p.turnMs(0), 25);
p.input(100);
assert.equal(p.turnMs(100), 8);
assert.equal(p.turnMs(349), 8);
assert.equal(p.turnMs(350), 25);
p.input(300);
assert.equal(p.turnMs(549), 8);
assert.equal(p.turnMs(550), 25);
assert.equal(p.sliceCycles(2_000_000, 25), 64_000);
assert.equal(p.sliceCycles(100, 8), 100, 'never extend the guest target');
p.observe(64_000, 16);
assert.equal(p.sliceCycles(2_000_000, 8), 16_000, 'adapt immediately to expensive work');
assert.equal(p.sliceCycles(2_000_000, 1), 4_000, 'respect remaining host turn');
p.observe(64_000, 1);
assert.equal(p.sliceCycles(2_000_000, 8), 20_000, 'grow gradually');
p.observe(0, 1);
p.observe(1, 0);
assert.equal(p.sliceCycles(2_000_000, 8), 20_000);
for (let i = 0; i < 100; i++) p.observe(2_000_000, 1);
assert.equal(p.sliceCycles(3_000_000, 25), 2_000_000, 'retain maximum guest slice');
console.log('worker pacing tests passed');

// Exercise the actual worker loop with a deterministic WASM stand-in. Each run advances
// exactly its requested cycles and consumes host time; scheduling must not invent guest time.
const { readFile } = await import('node:fs/promises');
const { runInNewContext } = await import('node:vm');
let wall = 0, cycles = 0;
const pending = [], runs = [];
const wasm = {
  memory: new WebAssembly.Memory({ initial: 1 }),
  esp32sim_alloc: () => 0, esp32sim_free() {}, esp32sim_new: () => 1,
  esp32sim_set_jit() {}, esp32sim_boot: () => 0,
  esp32sim_cpu_hz: () => 240e6, esp32sim_cycles: () => cycles,
  esp32sim_insns: () => cycles, esp32sim_out_take: () => 0,
  esp32sim_in_text() {},
  esp32sim_run(_emu, amount) { runs.push(amount); cycles += amount; wall += amount / 16_000; return 0; },
};
const source = (await readFile(new URL('../web/wasm/worker.js', import.meta.url), 'utf8'))
  .replace(/^import .*;\n/gm, '');
const context = {
  createPacing, createJitHost: () => ({ imports: {} }), TextEncoder, TextDecoder,
  performance: { now: () => wall }, Date, postMessage() {},
  WebAssembly: { instantiate: async () => ({ instance: { exports: wasm } }) },
  setTimeout: (callback) => pending.push(callback),
};
runInNewContext(source, context);
await context.onmessage({ data: { op: 'init' } });
await context.onmessage({ data: { op: 'create', board: 'test' } });
await context.onmessage({ data: { op: 'start' } });
wall = 100;
await context.onmessage({ data: { op: 'text', data: '{"t":"touch"}' } });
const start = wall;
pending.shift()();
assert.equal(wall - start, 8, 'interaction turn yields after eight ms');
assert.equal(cycles, runs.reduce((a, b) => a + b, 0), 'only WASM advances guest cycles');
assert.ok(runs.every(n => n > 0 && n <= 2_000_000));
console.log('worker integration test passed');
wall = 400;
const idleStart = wall;
pending.shift()();
assert.equal(wall - idleStart, 25, 'restore throughput turn after interaction expires');
await context.onmessage({ data: { op: 'stop' } });
const stoppedCycles = cycles;
pending.shift()();
assert.equal(cycles, stoppedCycles, 'a pending callback cannot run a stopped emulator');
