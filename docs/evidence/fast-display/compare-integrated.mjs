import { readFileSync, writeFileSync } from 'node:fs';
const run = process.argv[2] ?? '/Users/alice/src/a/esp32sim/work/fast-run/integrated-inline-full-v2/run';
const hardwarePath = '/Users/alice/src/a/esp32sim/work/fast-hardware/docs/evidence/fast-hardware-gate1-2026-09-07/summary-1.json';
const hardware = JSON.parse(readFileSync(hardwarePath));
const browser = JSON.parse(readFileSync(`${run}/result.json`));
const serial = JSON.parse(readFileSync(`${run}/events.json`)).filter(r => r.type === 'serial').map(r => r.data).join('');
const parse = line => {
  const [marker, ...tokens] = line.trim().split(/\s+/);
  return { marker, ...Object.fromEntries(tokens.filter(x => x.includes('=')).map(x => {
    const i = x.indexOf('='); const value = x.slice(i + 1);
    return [x.slice(0, i), /^-?\d+$/.test(value) ? Number(value) : value];
  })) };
};
const identity = r => [r.marker, ...['corpus', 'zoom', 'trace', 'kind', ...(r.marker === 'TINYDRAW_GATE1_HARD' ? ['operations', 'samples'] : [])].filter(k => k in r).map(k => `${k}=${r[k]}`)].join(' ');
const sims = new Map(serial.split(/\r?\n/).filter(l => l.startsWith('TINYDRAW_')).map(parse).map(r => [identity(r), r]));
const selected = {
  TINYDRAW_GATE1_PACED_COLD: ['compute_us', 'present_us', 'wall_us'],
  TINYDRAW_GATE1_PANEL_STAGE_AB: ['linear_pie_us', 'linear_scalar_us', 'ring_pie_us', 'ring_scalar_us'],
  TINYDRAW_LIVE_PRESENT: ['compose_us', 'transfer_wait_us', 'tear_wait_us'],
  TINYDRAW_GATE1_HARD: ['total_us', 'presentation_us'],
  TINYDRAW_GATE1_WORKLOAD: ['load_us'],
  TINYDRAW_GATE1_EXPORT: ['elapsed_us'],
};
const rows = [];
for (const h of hardware.firmwareTimingRecords) {
  if (!(h.marker in selected)) continue;
  // Other LIVE_PRESENT identities repeat; do not pair several hardware calls
  // with the final simulator call just because kind and zoom match.
  if (h.marker === 'TINYDRAW_LIVE_PRESENT' && h.kind !== 'startup') continue;
  const s = sims.get(identity(h));
  if (!s) continue;
  const workloadDifferences = Object.fromEntries(['operations', 'samples', 'events', 'consumed', 'steps', 'tiles', 'rendered', 'pushes', 'encoded', 'svg_bytes', 'png_bytes'].filter(k => k in h && h[k] !== s[k]).map(k => [k, { hardware: h[k], simulator: s[k] }]));
  for (const field of selected[h.marker]) {
    if (typeof h[field] !== 'number' || !h[field] || typeof s[field] !== 'number') continue;
    rows.push({ record: identity(h), field, hardwareUs: h[field], integratedUs: s[field], ratio: s[field] / h[field], workloadDifferences });
  }
}
const median = a => { a.sort((a,b) => a-b); return a.length ? a[Math.floor(a.length/2)] : null; };
const summary = Object.fromEntries(['compute_us', 'present_us', 'wall_us'].map(field => {
  const ratios = rows.filter(r => r.record.startsWith('TINYDRAW_GATE1_PACED_COLD') && r.field === field).map(r => r.ratio);
  return [field, { count: ratios.length, median: median(ratios), min: Math.min(...ratios), max: Math.max(...ratios) }];
}));
const result = { run, hardwarePath, browserStatus: browser.result.status, browserVerdict: browser.result.verdict,
  caveat: 'Firmware timer ratios, not host throughput. One hardware run restores a drawing unlike erased simulator flash. Matching selected workload fields does not establish complete state equality.', summary, rows };
writeFileSync('docs/evidence/fast-display/integrated-hardware-comparison.json', JSON.stringify(result, null, 2));
console.log(JSON.stringify(summary, null, 2));
console.log(JSON.stringify(rows.filter(r => !r.record.startsWith('TINYDRAW_GATE1_PACED_COLD')), null, 2));
