// esp32sim in a Web Worker: owns the wasm instance, paces it to wall time, and relays the UI
// protocol (docs/web-ui.md) to the page as postMessage — text as strings, binary as ArrayBuffers.
let CPU_HZ = 240e6;   // replaced from the module once an emulator exists: the C3 runs at 160 MHz
let wasm = null, emu = 0, running = false, t0 = 0, resyncs = 0, lastStat = { wall: 0, insns: 0 };
const jitModules = new Map();
const jitDisabled = new Set();
const enc = new TextEncoder(), dec = new TextDecoder();
const mem = () => new Uint8Array(wasm.memory.buffer);
function put(bytes) { const p = wasm.esp32sim_alloc(bytes.length); mem().set(bytes, p); return p; }
function withBytes(bytes, f) { const p = put(bytes); try { return f(p, bytes.length); } finally { wasm.esp32sim_free(p, bytes.length); } }
const imports = { env: { host_log: (p, n) => postMessage({ log: dec.decode(mem().subarray(p, p + n)) }) } };

function drain() {
  const n = wasm.esp32sim_out_take(emu);
  for (let i = 0; i < n; i++) {
    const kind = wasm.esp32sim_out_kind(emu, i), p = wasm.esp32sim_out_ptr(emu, i), len = wasm.esp32sim_out_len(emu, i);
    if (kind === 1) postMessage({ text: dec.decode(mem().subarray(p, p + len)) });
    else { const buf = new ArrayBuffer(len); new Uint8Array(buf).set(mem().subarray(p, p + len)); postMessage({ bin: buf }, [buf]); }
  }
}

function dispatchJit(cycles) {
  if (!wasm.esp32sim_jit_prepare) return false;
  const id = wasm.esp32sim_jit_prepare(emu, Math.max(1, Math.min(cycles, 0xffffffff)), Date.now());
  if (id === 0) return false;
  if (jitDisabled.has(id)) { wasm.esp32sim_jit_abort(emu); return false; }
  try {
    let instance = jitModules.get(id);
    if (!instance) {
      const p = wasm.esp32sim_jit_module_ptr(emu), len = wasm.esp32sim_jit_module_len(emu);
      const module = new WebAssembly.Module(mem().slice(p, p + len));
      instance = new WebAssembly.Instance(module, { env: { memory: wasm.memory } });
      jitModules.set(id, instance);
    }
    instance.exports.run();
    if (wasm.esp32sim_jit_commit(emu) === 1) return true;
    jitDisabled.add(id);
    return false;
  } catch (error) {
    wasm.esp32sim_jit_abort(emu);
    jitDisabled.add(id);
    postMessage({ log: '[jit] falling back: ' + (error && error.message || error) });
    return false;
  }
}

function loop() {
  if (!running) return;
  const now = performance.now();
  let cur = wasm.esp32sim_cycles(emu);
  let target = (now - t0) / 1000 * CPU_HZ;
  if (target - cur > CPU_HZ * 0.5) { t0 = now - cur / CPU_HZ * 1000; target = cur + CPU_HZ * 0.02; resyncs++; }   // hopelessly behind: skip, don't burst
  while (cur < target) {
    const remaining = Math.min(target - cur, 2_000_000);
    const usedJit = dispatchJit(remaining);
    const rc = usedJit ? 0 : wasm.esp32sim_run(emu, remaining, Date.now());
    cur = wasm.esp32sim_cycles(emu);
    drain();
    if (rc !== 0) { running = false; postMessage({ stopped: rc }); return; }
    if (performance.now() - now > 25) break;                       // let messages flow, come back
  }
  const aheadMs = cur / CPU_HZ * 1000 - (performance.now() - t0);
  const wall = performance.now();
  if (wall - lastStat.wall > 1000) {
    const insns = wasm.esp32sim_insns(emu);
    postMessage({ pace: { behind: Math.max(0, -aheadMs / 1000), resyncs, mips: Math.max(0, (insns - lastStat.insns)) / (wall - lastStat.wall) / 1000 } });
    lastStat = { wall, insns };
  }
  setTimeout(loop, Math.max(0, Math.min(20, aheadMs)));
}

onmessage = async (ev) => {
  const m = ev.data;
  try {
    if (m.op === 'init') { const r = await WebAssembly.instantiate(m.wasm, imports); wasm = r.instance.exports; jitModules.clear(); jitDisabled.clear(); postMessage({ ready: true }); }
    else if (m.op === 'create') {
      jitModules.clear(); jitDisabled.clear();
      emu = withBytes(enc.encode(m.board), (p, n) => wasm.esp32sim_new(p, n, m.flash_mb | 0, m.psram_mb | 0));
      if (emu !== 0 && wasm.esp32sim_cpu_hz) CPU_HZ = wasm.esp32sim_cpu_hz(emu);
      postMessage({ created: emu !== 0 });
    }
    else if (m.op === 'load') { const rc = withBytes(new Uint8Array(m.data), (p, n) => m.at !== undefined ? wasm.esp32sim_load_at(emu, m.at >>> 0, p, n) : wasm.esp32sim_load(emu, m.kind, p, n)); postMessage({ loaded: m.at !== undefined ? 'at' + m.at : m.kind, ok: rc === 0 }); }
    else if (m.op === 'stub') { withBytes(enc.encode(m.name), (p, n) => wasm.esp32sim_stub(emu, p, n, m.value >>> 0)); }
    else if (m.op === 'wifi') { withBytes(enc.encode(m.spec), (p, n) => wasm.esp32sim_wifi(emu, p, n)); }
    else if (m.op === 'start') { const rc = wasm.esp32sim_boot(emu, m.appDirect ? 1 : 0); if (rc === 0) { running = true; t0 = performance.now(); loop(); } postMessage({ started: rc === 0 }); }
    else if (m.op === 'stop') { running = false; }
    else if (m.op === 'text') { withBytes(enc.encode(m.data), (p, n) => wasm.esp32sim_in_text(emu, p, n)); }
    else if (m.op === 'bin') { withBytes(new Uint8Array(m.data), (p, n) => wasm.esp32sim_in_bin(emu, p, n)); }
  } catch (err) { postMessage({ log: '[worker] ' + (err && err.stack || err) }); running = false; }
};
