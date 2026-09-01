import { readFile } from "node:fs/promises";

const PIXELS = 2048;
const expectedCycles = 8 * PIXELS + 3;

async function instantiate(path) {
  const bytes = await readFile(path);
  return (await WebAssembly.instantiate(bytes, {})).instance.exports;
}

function outputHash(exports) {
  const destination = new Uint8Array(exports.memory.buffer, exports.jit_dest(), PIXELS * 2);
  let hash = 0x811c9dc5;
  for (const byte of destination) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

const [off, on] = await Promise.all([instantiate(process.argv[2]), instantiate(process.argv[3])]);
for (const exports of [off, on]) {
  if (exports.jit_setup(PIXELS) !== 0) throw new Error("jit_setup failed");
}
const offChecksum = off.jit_run(1) >>> 0;
const onChecksum = on.jit_run(1) >>> 0;
const cycles = (on.jit_cycles_hi() >>> 0) * 4294967296 + (on.jit_cycles_lo() >>> 0);
const misses = (on.jit_misses_hi() >>> 0) * 4294967296 + (on.jit_misses_lo() >>> 0);
if (offChecksum !== onChecksum || offChecksum === 0) throw new Error("architectural checksums differ");
const offHash = outputHash(off);
const onHash = outputHash(on);
if (offHash !== onHash) throw new Error("full architectural output hashes differ");
if (off.jit_cycles_lo() !== 0 || off.jit_cycles_hi() !== 0) throw new Error("accounting-off ledger changed");
if (cycles !== expectedCycles) throw new Error(`accounting-on cycles ${cycles}, expected ${expectedCycles}`);
if (misses !== 128) throw new Error(`accounting-on misses ${misses}, expected 128`);
console.log(JSON.stringify({ checksum: onChecksum, outputFnv1a32: onHash, cycleLedger: cycles, cacheMisses: misses }));
