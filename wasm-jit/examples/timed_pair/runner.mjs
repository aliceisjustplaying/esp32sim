// Same verification and measurement code under Node and Chrome.
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);
function check(value, message) { if (!value) throw Error(message); }
const layouts = {pcs: [0x40381000, 0x40382000], sram: 4096, sramBase: 0x3fc89000};
function reset(memory, test) {
  new Uint8Array(memory.buffer).fill(0, 0, 4160);
  const v = new DataView(memory.buffer);
  for (let c = 0; c < 2; c++) {
    const b = c * 128;
    v.setUint32(b + 8, layouts.sramBase, true);
    v.setUint32(b + 12, 7 + c, true);
    v.setUint32(b + 16, (0xdeadbeef + c) >>> 0, true);
    v.setUint32(b + 64, layouts.pcs[c], true);
  }
  v.setBigUint64(272, BigInt(test.deadline), true);
  v.setUint32(4096, test.data, true);
  v.setUint32(4100, (test.data + 11) >>> 0, true);
}
function snapshot(memory, traced) {
  const v = new DataView(memory.buffer), u32 = o => v.getUint32(o, true), u64 = o => Number(v.getBigUint64(o, true));
  const trace = [];
  if (traced) for (let i = 0; i < u32(280); i++) {
    const b = 8192 + i * 96;
    trace.push([u64(b), u32(b+8), u32(b+12), u32(b+16), u32(b+20),
      ...Array.from({length:16}, (_,r)=>u32(b+24+r*4)), u32(b+88)]);
  }
  return {registers:[0,128].map(b=>Array.from({length:16},(_,r)=>u32(b+r*4))),
    pcs:[u32(64),u32(192)], ccounts:[u32(80),u32(208)], insns:[u32(84),u32(212)],
    ready:[u64(72),u64(200)], sram:Array.from(new Uint8Array(memory.buffer,4096,64)),
    now:u64(256), flag:u32(264), events:u32(268), stopped:u32(284), trace,
    edges:u32(264)?[u64(288)]:[]};
}
async function instance(load, name) {
  const bytes = await load(name+'.wasm');
  const memory = new WebAssembly.Memory({initial:8});
  const {instance} = await WebAssembly.instantiate(bytes, {env:{memory}});
  return {memory, run:instance.exports.run, bytes:bytes.length};
}
export async function verify(load) {
  const cases = JSON.parse(new TextDecoder().decode(await load('cases.json')));
  const modules = {};
  for (const name of ['timed','refusal','reverse-ties','late-deadline','omit-load-use','refusal-reverse-ties']) modules[name] = await instance(load,name);
  let comparisons = 0;
  for (const test of cases) {
    const n = test.count || 20;
    for (const chunks of [[n], Array(n).fill(1), n===20?[3,1,7,2,7]:[137,1,862]]) {
      for (const tracing of [true,false]) {
        const m = modules[test.refusal?'refusal':'timed']; reset(m.memory,test);
        for (const chunk of chunks) m.run(chunk, Number(tracing));
        if (test.refusal) m.run(1,Number(tracing)); // sticky refusal must not execute twice
        const actual=snapshot(m.memory,tracing), expected={...test.expected,trace:tracing?test.expected.trace:[]};
        if (!eq(actual,expected)) {
          const fields=Object.keys(actual).filter(k=>!eq(actual[k],expected[k]));
          throw Error(`mismatch deadline=${test.deadline} data=${test.data} refusal=${test.refusal} chunks=${chunks.length} trace=${tracing} fields=${fields}: actual=${JSON.stringify(actual)} expected=${JSON.stringify(expected)}`);
        }
        comparisons++;
      }
    }
  }
  const killed=[];
  for (const name of ['reverse-ties','late-deadline','omit-load-use','refusal-reverse-ties']) {
    const tests=cases.filter(t=>t.refusal===name.startsWith('refusal-'));
    let mismatch=false;
    for (const test of tests) {
      const m=modules[name]; reset(m.memory,test); m.run(test.count||20,1);
      if (!eq(snapshot(m.memory,true),test.expected)) { mismatch=true; break; }
    }
    check(mismatch,`surviving mutant ${name}`); killed.push(name);
  }
  // Wrong tie order must change guest state in the unpriced-store case, not just a log.
  const stopCase=cases.find(t=>t.refusal), wrong=modules['refusal-reverse-ties'];
  reset(wrong.memory,stopCase); wrong.run(20,1);
  check(!eq(snapshot(wrong.memory,true).registers,stopCase.expected.registers),'tie mutant failed to alter guest state');
  return {passed:true,comparisons,negativeMutantsKilled:killed,moduleBytes:modules.timed.bytes,cases:cases.length};
}
export async function measure(load, {events=1_000_000_000,samples=5}={}) {
  const oracleCases=JSON.parse(new TextDecoder().decode(await load('cases.json')));
  const oracle=oracleCases.find(t=>t.count===1000).expected;
  const started=performance.now();
  const m=await instance(load,'timed');
  const compileMilliseconds=performance.now()-started;
  reset(m.memory,{deadline:5,data:41}); m.run(1_000_000,0);
  const results=[];
  for (let sample=0;sample<samples;sample++) {
    reset(m.memory,{deadline:5,data:41});
    const start=performance.now();
    // Fixed chunking includes the host/WASM call, generated scheduler, deadline handling,
    // guards, PC/register/memory updates, counters and per-instruction timing.
    let left=events;
    while(left) { const n=Math.min(left,1000); check(m.run(n,0)===0,'unexpected benchmark refusal'); left-=n; }
    const wallMilliseconds=performance.now()-start;
    const state=snapshot(m.memory,false);
    check(state.events===events && state.insns[0]+state.insns[1]===events,'wrong benchmark instruction count');
    check(state.ccounts.every((v,i)=>v===state.ready[i]),'CCOUNT/ready mismatch');
    check(state.now===Math.min(...state.ready)&&state.flag===1,'wrong shared clock or deadline');
    check(eq(state.sram,oracle.sram),'benchmark changed shared SRAM');
    for(let core=0;core<2;core++){
      const row=oracle.trace.findLast(r=>r[1]===core && r[21]===state.pcs[core]);
      check(row && eq(state.registers[core],row.slice(5,21)),'benchmark register/PC phase differs from native oracle');
      const prefix=core===0?[0,1,3,4,5,6]:[0,1,2,3,4,5];
      const n=state.insns[core];
      check(state.ready[core]===Math.floor(n/6)*(core===0?9:8)+prefix[n%6],'benchmark cycle ledger differs from native pattern');
    }
    const guestSeconds=state.now/240_000_000;
    results.push({wallMilliseconds,guestSeconds,realtimeRatio:guestSeconds/(wallMilliseconds/1000),mips:events/(wallMilliseconds*1000),state});
  }
  return {compileMilliseconds,events,samples,chunkEvents:1000,traceEnabled:false,results};
}
if (typeof process!=='undefined' && process.versions?.node && process.argv[1]?.endsWith('runner.mjs')) {
  const fs=await import('node:fs/promises'), path=await import('node:path');
  const root=process.argv[2]||'work/timed-pair';
  const load=name=>fs.readFile(path.join(root,name));
  const result={runtime:process.version,validation:await verify(load)};
  if(process.argv.includes('--measure')) result.measurements=await measure(load);
  console.log(JSON.stringify(result,null,2));
}
