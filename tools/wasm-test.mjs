#!/usr/bin/env node
// Run the real WebAssembly module the way the page does — instantiate web/wasm/esp32sim.wasm,
// drive the C ABI (docs/wasm.md) through a firmware manifest, drain the outbox — and check that
// the firmware boots to its console output with no panic. The native ABI tests (wasm/tests/abi.rs)
// compile the same crate for the host, so they can never see a wasm-only abort; this can.
//
//   tools/wasm-test.mjs [manifest ...]          default: hello c3-hello
//   ESP32SIM_ROM_DIR=dir                        mask ROM ELFs not found next to the manifests
//   ESP32SIM_NO_WASM_JIT=1                      interpreter-only manifest runs
//
// A manifest (web/wasm/fw/<name>.json) names the board, sizes, stubs and files exactly as the
// page reads them. Each run boots and executes `seconds` (3) of emulated time in 2 M-cycle
// slices, then expects a `board` message, the expected console line, and a clean run.
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fwDir = join(root, 'web', 'wasm', 'fw');
const names = process.argv.slice(2).length ? process.argv.slice(2) : ['hello', 'c3-hello'];
const EXPECT = { console: 'Hello world!', seconds: 3 };

function romPath(file) {
  for (const d of [fwDir, process.env.ESP32SIM_ROM_DIR].filter(Boolean)) { const p = join(d, file); if (existsSync(p)) return p; }
  const base = join(homedir(), '.espressif', 'tools', 'esp-rom-elfs');
  if (existsSync(base)) for (const rel of readdirSync(base).sort().reverse()) { const p = join(base, rel, file); if (existsSync(p)) return p; }
  throw new Error(`${file}: not in ${fwDir}, ESP32SIM_ROM_DIR or ~/.espressif/tools/esp-rom-elfs`);
}

const wasmBytes = readFileSync(join(root, 'web', 'wasm', 'esp32sim.wasm'));
const enc = new TextEncoder(), dec = new TextDecoder();
let failures = 0;

function dispatchJit(w, emu, mem, cache, disabled, cycles) {
  if (!w.esp32sim_jit_prepare) return false;
  const id = w.esp32sim_jit_prepare(emu, cycles, Date.now());
  if (id === 0) return false;
  if (disabled.has(id)) { w.esp32sim_jit_abort(emu); return false; }
  try {
    let instance = cache.get(id);
    if (!instance) {
      const p = w.esp32sim_jit_module_ptr(emu), len = w.esp32sim_jit_module_len(emu);
      const module = new WebAssembly.Module(mem().slice(p, p + len));
      instance = new WebAssembly.Instance(module, { env: { memory: w.memory } });
      cache.set(id, instance);
    }
    instance.exports.run();
    if (w.esp32sim_jit_commit(emu) === 1) return true;
  } catch (_) {
    w.esp32sim_jit_abort(emu);
  }
  disabled.add(id);
  return false;
}

function jitStats(w, emu, mem) {
  if (!w.esp32sim_jit_stats_ptr) return null;
  const p = w.esp32sim_jit_stats_ptr(emu), len = w.esp32sim_jit_stats_len(emu);
  return JSON.parse(dec.decode(mem().subarray(p, p + len)));
}

async function testJitHandoff() {
  let w;
  const { instance } = await WebAssembly.instantiate(wasmBytes, { env: { host_log() {} } });
  w = instance.exports;
  const mem = () => new Uint8Array(w.memory.buffer);
  const withBytes = (bytes, f) => { const p = w.esp32sim_alloc(bytes.length); mem().set(bytes, p); try { return f(p, bytes.length); } finally { w.esp32sim_free(p, bytes.length); } };
  const emu = withBytes(enc.encode('none'), (p, n) => w.esp32sim_new(p, n, 1, 0));
  const entry = 0x40370000;
  const program = new Uint8Array(64 * 2 + 3);
  for (let i = 0; i < 64; i++) program.set([0x0c, 0x03], i * 2); // movi.n a3,0
  program.set([0x06, 0xff, 0xff], 64 * 2);                       // j .
  const app = new Uint8Array(24 + 8 + program.length), view = new DataView(app.buffer);
  app[0] = 0xe9; app[1] = 1; view.setUint32(4, entry, true);
  view.setUint32(24, entry, true); view.setUint32(28, program.length, true);
  app.set(program, 32);
  if (withBytes(app, (p, n) => w.esp32sim_load(emu, 3, p, n)) !== 0 || w.esp32sim_boot(emu, 1) !== 0) throw new Error('JIT fixture boot failed');
  if (!dispatchJit(w, emu, mem, new Map(), new Set(), 64)) throw new Error('JIT handoff did not commit');
  if (w.esp32sim_cycles(emu) !== 64 || w.esp32sim_insns(emu) !== 64) throw new Error('JIT handoff accounting mismatch');
  const stats = jitStats(w, emu, mem);
  if (stats.attempts !== 1 || stats.prepared !== 1 || stats.committed !== 1) throw new Error(`JIT statistics mismatch: ${JSON.stringify(stats)}`);
  w.esp32sim_delete(emu);
  console.log('ok   wasm JIT handoff: shared-memory block committed at a scheduler boundary');
}

async function runManifest(name) {
  const m = JSON.parse(readFileSync(join(fwDir, `${name}.json`), 'utf8'));
  const logs = [];
  const { instance } = await WebAssembly.instantiate(wasmBytes, { env: { host_log: (p, n) => logs.push(dec.decode(mem().subarray(p, p + n))) } });
  const w = instance.exports;
  const mem = () => new Uint8Array(w.memory.buffer);
  const withBytes = (bytes, f) => { const p = w.esp32sim_alloc(bytes.length); mem().set(bytes, p); try { return f(p, bytes.length); } finally { w.esp32sim_free(p, bytes.length); } };
  const file = (rel) => readFileSync(rel.endsWith('_rom.elf') && !rel.includes('/') ? romPath(rel) : join(fwDir, rel));

  const emu = withBytes(enc.encode(m.board), (p, n) => w.esp32sim_new(p, n, m.flash_mb | 0, m.psram_mb | 0));
  if (!emu) throw new Error(`esp32sim_new(${m.board}) returned null: ${logs.join(' | ')}`);
  const kinds = { rom: 0, bootloader: 1, ptable: 2, app: 3, flash: 5, script: 6, picture: 7 };
  for (const [k, v] of Object.entries(m.files || {})) {
    for (const rel of [].concat(v)) {
      const rc = withBytes(new Uint8Array(file(rel)), (p, n) => w.esp32sim_load(emu, k === 'elf' ? 4 : kinds[k], p, n));
      if (rc !== 0) throw new Error(`load ${k} ${rel} failed: ${logs.join(' | ')}`);
    }
  }
  for (const [off, rel] of Object.entries(m.flash_at || {})) withBytes(new Uint8Array(file(rel)), (p, n) => w.esp32sim_load_at(emu, Number(off) >>> 0, p, n));
  for (const s of m.stubs || []) { const [sym, val] = s.split('='); const name = (m.symbols || {})[sym] || sym;   // as the page: a symbols map resolves a stub without the ELF
    withBytes(enc.encode(name), (p, n) => w.esp32sim_stub(emu, p, n, Number(val ?? 0) >>> 0)); }
  if (m.wifi) withBytes(enc.encode(m.wifi), (p, n) => w.esp32sim_wifi(emu, p, n));
  if (w.esp32sim_boot(emu, 0) !== 0) throw new Error(`boot failed: ${logs.join(' | ')}`);

  const hz = w.esp32sim_cpu_hz(emu);
  const jitCache = new Map(), jitDisabled = new Set();
  let board = null, text = '', frames = 0;
  const drain = () => {
    const n = w.esp32sim_out_take(emu);
    for (let i = 0; i < n; i++) {
      const kind = w.esp32sim_out_kind(emu, i), p = w.esp32sim_out_ptr(emu, i), len = w.esp32sim_out_len(emu, i);
      if (kind !== 1) { frames++; continue; }
      const msg = JSON.parse(dec.decode(mem().subarray(p, p + len)));
      if (msg.t === 'board') board = msg.name;
      if (msg.t === 'serial') text += msg.data;
    }
  };
  const target = hz * (m.seconds || EXPECT.seconds);
  const t0 = Date.now();
  while (w.esp32sim_cycles(emu) < target) {
    const usedJit = !process.env.ESP32SIM_NO_WASM_JIT && dispatchJit(w, emu, mem, jitCache, jitDisabled, 2_000_000);
    const rc = usedJit ? 0 : w.esp32sim_run(emu, 2_000_000, Date.now());
    if (rc !== 0) throw new Error(`esp32sim_run stopped with ${rc} at ${(w.esp32sim_cycles(emu) / hz).toFixed(3)} s: ${logs.slice(-3).join(' | ')}`);
    drain();
  }
  drain();
  const panics = logs.filter(l => l.includes('panic'));
  const problems = [];
  if (panics.length) problems.push(`panicked: ${panics[0]}`);
  if (!board) problems.push('no board message');
  const expected = m.expect || (!process.env.ESP32SIM_WASM_JIT_STATS && EXPECT.console);
  if (expected && !text.includes(expected)) problems.push(`console never showed ${JSON.stringify(expected)}; got ${text.length} bytes`);
  const insns = w.esp32sim_insns(emu);
  const stats = jitStats(w, emu, mem);
  w.esp32sim_delete(emu);
  const wall = (Date.now() - t0) / 1000;
  if (problems.length) { failures++; console.error(`FAIL ${name}: ${problems.join('; ')}\n  logs: ${logs.slice(0, 5).join('\n        ')}\n  console tail: ${text.slice(-400)}`); }
  else {
    console.log(`ok   ${name}: board ${board}, ${(insns / 1e6).toFixed(1)} M insns in ${wall.toFixed(1)} s wall (${(insns / 1e6 / wall).toFixed(1)} Minsn/s), ${text.split('\n').length} console lines, ${frames} binary frames`);
  }
  if (process.env.ESP32SIM_WASM_JIT_STATS) console.log(`jit  ${name}: ${JSON.stringify(stats)}`);
}

try { await testJitHandoff(); } catch (e) { failures++; console.error(`FAIL wasm JIT handoff: ${e.message}`); }
for (const n of names) { try { await runManifest(n); } catch (e) { failures++; console.error(`FAIL ${n}: ${e.message}`); } }
process.exit(failures ? 1 : 0);
