#!/usr/bin/env node
// Run the real WebAssembly module the way the page does — instantiate web/wasm/esp32sim.wasm,
// drive the C ABI (docs/wasm.md) through a firmware manifest, drain the outbox — and check that
// the firmware boots to its console output with no panic. The native ABI tests (wasm/tests/abi.rs)
// compile the same crate for the host, so they can never see a wasm-only abort; this can.
//
//   tools/wasm-test.mjs [manifest ...]          default: hello c3-hello
//   ESP32SIM_ROM_DIR=dir                        mask ROM ELFs not found next to the manifests
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
    const rc = w.esp32sim_run(emu, 2_000_000, Date.now());
    if (rc !== 0) throw new Error(`esp32sim_run stopped with ${rc} at ${(w.esp32sim_cycles(emu) / hz).toFixed(3)} s: ${logs.slice(-3).join(' | ')}`);
    drain();
  }
  drain();
  const panics = logs.filter(l => l.includes('panic'));
  const problems = [];
  if (panics.length) problems.push(`panicked: ${panics[0]}`);
  if (!board) problems.push('no board message');
  if (!text.includes(m.expect || EXPECT.console)) problems.push(`console never showed ${JSON.stringify(m.expect || EXPECT.console)}; got ${text.length} bytes`);
  const insns = w.esp32sim_insns(emu);
  w.esp32sim_delete(emu);
  const wall = (Date.now() - t0) / 1000;
  if (problems.length) { failures++; console.error(`FAIL ${name}: ${problems.join('; ')}\n  logs: ${logs.slice(0, 5).join('\n        ')}\n  console tail: ${text.slice(-400)}`); }
  else console.log(`ok   ${name}: board ${board}, ${(insns / 1e6).toFixed(1)} M insns in ${wall.toFixed(1)} s wall (${(insns / 1e6 / wall).toFixed(1)} Minsn/s), ${text.split('\n').length} console lines, ${frames} binary frames`);
}

for (const n of names) { try { await runManifest(n); } catch (e) { failures++; console.error(`FAIL ${n}: ${e.message}`); } }
process.exit(failures ? 1 : 0);
