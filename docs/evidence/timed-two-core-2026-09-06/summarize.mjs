import fs from 'node:fs';
import path from 'node:path';
const root=path.dirname(new URL(import.meta.url).pathname);
const report=[];
for(const run of ['run-1','run-native']){
  const text=fs.readFileSync(path.join(root,run,'native-code.txt'),'utf8');
  const ranges=[...text.matchAll(/Instructions \(size = (\d+), (0x[0-9a-f]+)-(0x[0-9a-f]+)\)/g)]
    .map(m=>({size:+m[1],start:m[2],end:m[3],nativePcSamples:0}));
  let ticks=0;
  for(const line of text.split('\n')){
    if(!line.startsWith('tick,'))continue;
    const pc=Number(line.split(',')[1]);
    if(!Number.isSafeInteger(pc))throw Error('Malformed native tick');
    ticks++;
    for(const r of ranges)if(pc>=Number(r.start)&&pc<Number(r.end))r.nativePcSamples++;
  }
  const result={run,rawTickCount:ticks,wasmRanges:ranges,
    nativeInstructionsPrinted:(text.match(/^0x[0-9a-f]+\s+[0-9a-f]+\s/gm)||[]).length};
  if(run==='run-1'){
    const p=JSON.parse(fs.readFileSync(path.join(root,run,'profile.cpuprofile')));
    const counts={},times={};
    if(p.samples.length!==p.timeDeltas.length)throw Error('Profile length mismatch');
    p.samples.forEach((id,i)=>{counts[id]=(counts[id]||0)+1;times[id]=(times[id]||0)+p.timeDeltas[i];});
    result.cdp={samples:p.samples.length,durationMicroseconds:p.endTime-p.startTime,
      frames:p.nodes.filter(n=>counts[n.id]).map(n=>({frame:n.callFrame,count:counts[n.id],
        weightedMicroseconds:times[n.id],sourceLines:[...new Set((n.positionTicks||[]).map(x=>x.line))]}))};
  }
  report.push(result);
}
fs.writeFileSync(path.join(root,'summary.json'),JSON.stringify(report,null,2)+'\n');
console.log(report.map(r=>({run:r.run,rawTicks:r.rawTickCount,wasmTicks:r.wasmRanges.reduce((n,r)=>n+r.nativePcSamples,0)})));
