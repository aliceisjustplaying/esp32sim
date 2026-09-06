// Dedicated headless Chrome run; only processes started here are terminated.
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import {spawn} from 'node:child_process';
import os from 'node:os';
import {createHash} from 'node:crypto';
const [rootArg='work/timed-pair', outputArg='work/timed-pair/browser'] = process.argv.slice(2);
const root=path.resolve(rootArg), out=path.resolve(outputArg);
await fs.mkdir(out,{recursive:true});
const runner=await fs.readFile(new URL('./runner.mjs',import.meta.url));
const html=`<!doctype html><meta charset="utf-8"><title>Timed two-core experiment</title><pre id="status">Running</pre><script type="module">
import {verify,measure} from '/runner.mjs';
const load=async name=>new Uint8Array(await (await fetch('/'+name)).arrayBuffer());
window.resultPromise=(async()=>{const result={validation:await verify(load),measurements:await measure(load)};document.querySelector('#status').textContent=JSON.stringify(result);return result;})();
</script>`;
const server=http.createServer(async(req,res)=>{
  try {
    const name=new URL(req.url,'http://local').pathname;
    if(name==='/') {res.setHeader('Content-Type','text/html');res.end(html);return;}
    if(name==='/runner.mjs') {res.setHeader('Content-Type','text/javascript');res.end(runner);return;}
    if(!/^\/[a-z-]+\.(wasm|json)$/.test(name)) {res.writeHead(404);res.end();return;}
    res.setHeader('Content-Type',name.endsWith('.wasm')?'application/wasm':'application/json');
    res.end(await fs.readFile(path.join(root,name.slice(1))));
  } catch(e) {res.writeHead(404);res.end(String(e));}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const url=`http://127.0.0.1:${server.address().port}/`;
const profile=await fs.mkdtemp(path.join(out,'chrome-'));
const args=['--headless=new','--no-first-run','--no-default-browser-check','--disable-background-timer-throttling','--disable-renderer-backgrounding','--remote-debugging-address=127.0.0.1','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'];
const chrome=spawn('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',args,{stdio:['ignore','ignore','pipe']});
let log='';chrome.stderr.on('data',d=>log+=d);
const pause=ms=>new Promise(r=>setTimeout(r,ms));
let ws;
const pending=new Map();let seq=0;
const send=(method,params={},sessionId)=>new Promise((resolve,reject)=>{const id=++seq;pending.set(id,{resolve,reject});ws.send(JSON.stringify({id,method,params,...(sessionId?{sessionId}:{})}));});
const timeout=setTimeout(()=>{chrome.kill();},120000);
try {
  let port;
  for(let i=0;i<200;i++) {
    if(chrome.exitCode!==null) throw Error(`Chrome exited: ${log}`);
    try {port=Number((await fs.readFile(path.join(profile,'DevToolsActivePort'),'utf8')).split('\n')[0]);break;} catch{}
    await pause(50);
  }
  if(!port) throw Error('Chrome did not start');
  const version=await(await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  ws=new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{ws.onopen=resolve;ws.onerror=reject;});
  ws.onmessage=({data})=>{const m=JSON.parse(data);if(!m.id)return;const p=pending.get(m.id);pending.delete(m.id);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);};
  ws.onclose=()=>{for(const p of pending.values())p.reject(Error('Chrome closed'));pending.clear();};
  const {targetId}=await send('Target.createTarget',{url:'about:blank'});
  const {sessionId}=await send('Target.attachToTarget',{targetId,flatten:true});
  const loadsBefore=os.loadavg();
  await send('Page.navigate',{url},sessionId);
  for(let i=0;i<200;i++) {
    const r=await send('Runtime.evaluate',{expression:'!!window.resultPromise',returnByValue:true},sessionId);
    if(r.result.value)break;
    if(i===199)throw Error('Page did not initialize');
    await pause(50);
  }
  const evaluated=await send('Runtime.evaluate',{expression:'window.resultPromise',awaitPromise:true,returnByValue:true},sessionId);
  if(evaluated.exceptionDetails)throw Error(JSON.stringify(evaluated.exceptionDetails));
  const sourceHashes={};
  for(const source of ['wasm-jit/src/scheduled.rs','wasm-jit/src/lib.rs','wasm-jit/examples/timed_pair.rs','wasm-jit/examples/timed_pair/runner.mjs','wasm-jit/examples/timed_pair/browser.mjs','esp32s3/src/timing.rs','esp-soc/src/machine.rs']) {
    sourceHashes[source]=createHash('sha256').update(await fs.readFile(source)).digest('hex');
  }
  for(const file of ['timed.wasm','cases.json'])sourceHashes[file]=createHash('sha256').update(await fs.readFile(path.join(root,file))).digest('hex');
  const report={schemaVersion:1,scope:'bounded two-core experiment, not TinyDraw or production integration',date:new Date().toISOString(),version,node:process.version,host:{arch:os.arch(),release:os.release(),cpu:os.cpus()[0]?.model},loadsBefore,loadsAfter:os.loadavg(),chromeArgs:args,sourceHashes,...evaluated.result.value};
  await fs.writeFile(path.join(out,'result.json'),JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify({validation:report.validation,samples:report.measurements.results.map(({wallMilliseconds,mips,realtimeRatio})=>({wallMilliseconds,mips,realtimeRatio})),output:path.join(out,'result.json')},null,2));
  await send('Target.closeTarget',{targetId});
} finally {
  clearTimeout(timeout);ws?.close();chrome.kill();server.close();
  await fs.writeFile(path.join(out,'chrome.log'),log);
  await new Promise(resolve=>{if(chrome.exitCode!==null)return resolve();chrome.once('exit',resolve);setTimeout(()=>{chrome.kill('SIGKILL');resolve();},5000).unref();});
}
