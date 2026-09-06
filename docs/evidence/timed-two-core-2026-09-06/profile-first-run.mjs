// Measurement-only harness. Uses the existing fixture and verifier unchanged.
import fs from 'node:fs/promises';
import {createWriteStream} from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {spawn} from 'node:child_process';
import os from 'node:os';
import {createHash} from 'node:crypto';

const root=path.resolve('work/timed-pair');
const out=path.resolve(process.argv[2] || 'work/timed-pair-profile/run-1');
await fs.mkdir(out,{recursive:true});
const runner=await fs.readFile('wasm-jit/examples/timed_pair/runner.mjs');
const html=`<!doctype html><meta charset="utf-8"><title>Timed pair profiling</title><script type="module">
import {verify,measure} from '/runner.mjs';
const load=async name=>new Uint8Array(await (await fetch('/'+name)).arrayBuffer());
window.validationPromise=verify(load);
window.doProfile=()=>measure(load,{events:1_000_000_000,samples:3});
</script>`;
const server=http.createServer(async(req,res)=>{
  try {
    const name=new URL(req.url,'http://local').pathname;
    if(name==='/'){res.setHeader('Content-Type','text/html');res.end(html);return;}
    if(name==='/runner.mjs'){res.setHeader('Content-Type','text/javascript');res.end(runner);return;}
    if(!/^\/[a-z-]+\.(wasm|json)$/.test(name)){res.writeHead(404);res.end();return;}
    res.setHeader('Content-Type',name.endsWith('.wasm')?'application/wasm':'application/json');
    res.end(await fs.readFile(path.join(root,name.slice(1))));
  } catch(e){res.writeHead(404);res.end(String(e));}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const url=`http://127.0.0.1:${server.address().port}/`;
const userData=await fs.mkdtemp(path.join(out,'chrome-'));
const args=['--headless=new','--no-first-run','--no-default-browser-check',
  '--disable-background-timer-throttling','--disable-renderer-backgrounding',
  '--remote-debugging-address=127.0.0.1','--remote-debugging-port=0',
  `--user-data-dir=${userData}`,
  `--js-flags=--prof --logfile=${out}/v8.log --print-wasm-code --print-wasm-code-function-index=0`,
  'about:blank'];
const chrome=spawn('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',args,{stdio:['ignore','pipe','pipe']});
const stdout=createWriteStream(path.join(out,'native-code.txt'));
const stderr=createWriteStream(path.join(out,'chrome.log'));
chrome.stdout.pipe(stdout);chrome.stderr.pipe(stderr);
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let ws,seq=0;const pending=new Map();
const send=(method,params={},sessionId)=>new Promise((resolve,reject)=>{
  const id=++seq;pending.set(id,{resolve,reject});
  ws.send(JSON.stringify({id,method,params,...(sessionId?{sessionId}:{})}));
});
const evaluate=async(expression,sessionId)=>{
  const result=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true},sessionId);
  if(result.exceptionDetails)throw Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
};
const timeout=setTimeout(()=>chrome.kill(),90000);
try {
  let port;
  for(let i=0;i<200;i++){
    if(chrome.exitCode!==null)throw Error('Chrome exited before debugging port was available');
    try{port=Number((await fs.readFile(path.join(userData,'DevToolsActivePort'),'utf8')).split('\n')[0]);break;}catch{}
    await pause(50);
  }
  if(!port)throw Error('Chrome did not start');
  const version=await(await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  ws=new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{ws.onopen=resolve;ws.onerror=reject;});
  ws.onmessage=({data})=>{
    const m=JSON.parse(data);if(!m.id)return;
    const p=pending.get(m.id);pending.delete(m.id);
    m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);
  };
  ws.onclose=()=>{for(const p of pending.values())p.reject(Error('Chrome closed'));pending.clear();};
  const {targetId}=await send('Target.createTarget',{url:'about:blank'});
  const {sessionId}=await send('Target.attachToTarget',{targetId,flatten:true});
  await send('Page.navigate',{url},sessionId);
  for(let i=0;i<200;i++){
    if(await evaluate('!!window.validationPromise',sessionId))break;
    if(i===199)throw Error('Page did not initialize');
    await pause(50);
  }
  const validation=await evaluate('window.validationPromise',sessionId);
  await send('Profiler.enable',{},sessionId);
  await send('Profiler.setSamplingInterval',{interval:1000},sessionId);
  const loadsBefore=os.loadavg();
  await send('Profiler.start',{},sessionId);
  const measurements=await evaluate('window.doProfile()',sessionId);
  const {profile}=await send('Profiler.stop',{},sessionId);
  const hashes={};
  for(const [label,file] of [['module',path.join(root,'timed.wasm')],['cases',path.join(root,'cases.json')],['runner','wasm-jit/examples/timed_pair/runner.mjs'],['harness',process.argv[1]]]){
    hashes[label]=createHash('sha256').update(await fs.readFile(file)).digest('hex');
  }
  await fs.writeFile(path.join(out,'profile.cpuprofile'),JSON.stringify(profile)+'\n');
  const result={scope:'instrumented profiling only; elapsed times are not confirmation speed evidence',date:new Date().toISOString(),version,host:{arch:os.arch(),release:os.release(),cpu:os.cpus()[0]?.model},args,hashes,loadsBefore,loadsAfter:os.loadavg(),validation,measurements};
  await fs.writeFile(path.join(out,'result.json'),JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify({out,validation,profileNodes:profile.nodes.length,profileSamples:profile.samples?.length,hashes}));
  await send('Target.closeTarget',{targetId});
} finally {
  clearTimeout(timeout);ws?.close();chrome.kill();server.close();
  await new Promise(resolve=>{
    if(chrome.exitCode!==null)return resolve();
    chrome.once('exit',resolve);
    setTimeout(()=>{chrome.kill('SIGKILL');resolve();},5000).unref();
  });
  await Promise.all([new Promise(resolve=>stdout.end(resolve)),new Promise(resolve=>stderr.end(resolve))]);
}
