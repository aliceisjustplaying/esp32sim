import { readFileSync, writeFileSync, createWriteStream } from 'node:fs';
import { spawn } from 'node:child_process';

const assets = JSON.parse(readFileSync('/Users/alice/src/a/esp32sim/work/perf-pie-confirm/candidate/assets.json'));
const prefix = 'docs/evidence/fast-display/tinydraw-timed-full';
const args = ['--board', 'waveshare-amoled18-v2', '--spi2-timing', '--measured-te',
  '--flash-mb', '16', '--psram-mb', '8', '--boot', 'rom', '--console', 'uart0', '--max-seconds', '75', '--no-dump'];
for (const key of ['rom', 'bootloader', 'ptable', 'app', 'elf']) args.push(`--${key}`, assets[key]);
const start = performance.now();
const log = createWriteStream(`${prefix}.log`);
const child = spawn('target/release/esp32sim', args);
let stdout = '', wallLimit = false;
child.stdout.on('data', chunk => { stdout += chunk; log.write(chunk); });
child.stderr.on('data', chunk => log.write(chunk));
const timer = setTimeout(() => { wallLimit = true; child.kill('SIGTERM'); }, 150000);
child.on('close', (code, signal) => {
  clearTimeout(timer);
  log.end();
  const records = stdout.split(/\r?\n/).filter(line => line.startsWith('TINYDRAW_')).map(line => {
    const [marker, ...tokens] = line.trim().split(/\s+/);
    return { marker, ...Object.fromEntries(tokens.filter(x => x.includes('=')).map(x => {
      const i = x.indexOf('='); const value = x.slice(i + 1);
      return [x.slice(0, i), /^-?\d+$/.test(value) ? Number(value) : value];
    })) };
  });
  const verdict = records.find(r => r.marker === 'TINYDRAW_GATE1_AUTOMATED_DONE');
  const result = { args, code, signal, wallLimit, wallSeconds: (performance.now() - start) / 1000,
    verdict, firmwareTimingRecords: records.filter(r => Object.keys(r).some(k => k.endsWith('_us'))) };
  writeFileSync(`${prefix}.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ ...result, args: undefined, firmwareTimingRecords: undefined }));
});
