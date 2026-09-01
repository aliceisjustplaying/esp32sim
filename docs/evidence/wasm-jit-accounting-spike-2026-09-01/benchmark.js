"use strict";

const PIXELS = 2048;
const EMULATED_INSTRUCTIONS_PER_ITERATION = 8 * PIXELS + 3;
const SAMPLE_SECONDS = 1.5;
const MODES = [
  "off", "on", "on", "off", "off", "on", "on",
  "off", "off", "on", "on", "off", "off", "on"
];

function expectSwapped(exports) {
  const destination = new Uint8Array(exports.memory.buffer, exports.jit_dest(), PIXELS * 2);
  for (let index = 0; index < PIXELS * 2; index += 2) {
    const low = (index * 31 + 7) & 0xff;
    const high = ((index + 1) * 31 + 7) & 0xff;
    if (destination[index] !== high || destination[index + 1] !== low) {
      throw new Error(`destination byte ${index} was not swapped`);
    }
  }
}

function cycleLedger(exports) {
  return (exports.jit_cycles_hi() >>> 0) * 4294967296 + (exports.jit_cycles_lo() >>> 0);
}

function cacheMisses(exports) {
  return (exports.jit_misses_hi() >>> 0) * 4294967296 + (exports.jit_misses_lo() >>> 0);
}

function outputFnv1a32(exports) {
  const destination = new Uint8Array(exports.memory.buffer, exports.jit_dest(), PIXELS * 2);
  let hash = 0x811c9dc5;
  for (const byte of destination) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

async function load(path) {
  const response = await fetch(path, { cache: "no-store" });
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return instance.exports;
}

function calibrate(exports) {
  if (exports.jit_setup(PIXELS) !== 0) throw new Error("jit_setup failed during calibration");
  exports.jit_run(5000);
  const probeIterations = 4000;
  const start = performance.now();
  exports.jit_run(probeIterations);
  const elapsedMs = performance.now() - start;
  if (!(elapsedMs > 0)) throw new Error("Chrome timer did not advance");
  return {
    probeIterations,
    probeElapsedMilliseconds: elapsedMs,
    selectedIterations: Math.max(
      1,
      Math.min(0x7fffffff, Math.round(probeIterations * SAMPLE_SECONDS * 1000 / elapsedMs))
    )
  };
}

function sample(exports, mode, iterations, orderIndex) {
  if (exports.jit_setup(PIXELS) !== 0) throw new Error(`${mode}: jit_setup failed`);
  const start = performance.now();
  const checksum = exports.jit_run(iterations) >>> 0;
  const elapsedMilliseconds = performance.now() - start;
  expectSwapped(exports);
  const emulatedInstructions = iterations * EMULATED_INSTRUCTIONS_PER_ITERATION;
  return {
    orderIndex,
    iterations,
    emulatedInstructions,
    elapsedMilliseconds,
    mips: emulatedInstructions / elapsedMilliseconds / 1000,
    checksum,
    outputFnv1a32: outputFnv1a32(exports),
    cycleLedger: cycleLedger(exports),
    cacheMisses: cacheMisses(exports)
  };
}

async function post(path, body) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body)
  });
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
}

async function main() {
  const modules = { off: await load("/accounting-off.wasm"), on: await load("/accounting-on.wasm") };
  const calibration = { off: calibrate(modules.off), on: calibrate(modules.on) };
  const raw = { calibration, accountingOff: [], accountingOn: [] };
  for (let orderIndex = 0; orderIndex < MODES.length; orderIndex++) {
    const mode = MODES[orderIndex];
    const result = sample(modules[mode], mode, calibration[mode].selectedIterations, orderIndex);
    raw[mode === "on" ? "accountingOn" : "accountingOff"].push(result);
  }
  await post("/result", {
    browser: { userAgent: navigator.userAgent, language: navigator.language },
    parameters: {
      pixelsPerIteration: PIXELS,
      emulatedInstructionsPerIteration: EMULATED_INSTRUCTIONS_PER_ITERATION,
      targetSecondsPerSample: SAMPLE_SECONDS,
      sampleOrder: MODES
    },
    raw
  });
  document.body.textContent = "measurement complete";
}

main().catch(async (error) => {
  document.body.textContent = String(error.stack || error);
  try {
    await post("/error", { error: String(error.stack || error) });
  } catch (_) {
    // The runner reports its own timeout if the result server is unavailable.
  }
});
