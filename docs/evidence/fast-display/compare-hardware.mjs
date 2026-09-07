import { readFileSync, writeFileSync } from 'node:fs';
const hardware = JSON.parse(readFileSync('/Users/alice/src/a/esp32sim/work/fast-hardware/docs/evidence/fast-hardware-gate1-2026-09-07/summary-1.json'));
const sim = JSON.parse(readFileSync('docs/evidence/fast-display/tinydraw-timed-full.json'));
const identity = r => [r.marker, ...['corpus', 'zoom', 'trace', 'kind'].filter(k => k in r).map(k => `${k}=${r[k]}`)].join(' ');
const sims = new Map(sim.firmwareTimingRecords.map(r => [identity(r), r]));
const rows = [];
for (const h of hardware.firmwareTimingRecords) {
  if (!['TINYDRAW_GATE1_PANEL_STAGE_AB', 'TINYDRAW_GATE1_PACED_COLD', 'TINYDRAW_LIVE_PRESENT'].includes(h.marker)) continue;
  const s = sims.get(identity(h));
  if (!s) continue;
  const workloadDifferences = Object.fromEntries(['operations', 'samples', 'events', 'consumed', 'steps', 'tiles', 'rendered', 'pushes'].filter(k => k in h && h[k] !== s[k]).map(k => [k, { hardware: h[k], simulator: s[k] }]));
  for (const field of ['compute_us', 'present_us', 'wall_us', 'linear_pie_us', 'linear_scalar_us', 'ring_pie_us', 'ring_scalar_us', 'transfer_wait_us', 'tear_wait_us']) {
    if (typeof h[field] !== 'number' || !h[field] || typeof s[field] !== 'number') continue;
    rows.push({ record: identity(h), field, hardwareUs: h[field], timedDisplayUs: s[field], timedDisplayOverHardware: s[field] / h[field], workloadDifferences });
  }
}
const median = a => { a.sort((a,b) => a-b); return a.length ? a[Math.floor(a.length/2)] : null; };
const summary = Object.fromEntries(['compute_us', 'present_us'].map(field => [field, median(rows.filter(r => r.record.startsWith('TINYDRAW_GATE1_PACED_COLD') && r.field === field).map(r => r.timedDisplayOverHardware))]));
const result = { caveat: 'Firmware timer ratios, not host throughput. One hardware run restores a drawing unlike erased simulator flash. Matching workload fields are not proof of complete state equality.', summary, rows };
writeFileSync('docs/evidence/fast-display/hardware-comparison.json', JSON.stringify(result, null, 2));
console.log(JSON.stringify(summary));
