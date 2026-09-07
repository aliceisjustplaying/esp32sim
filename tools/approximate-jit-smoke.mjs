// Functional TinyDraw probe. Node/V8 execution is not a browser performance receipt.
import fs from 'node:fs/promises';
import {createJitHost} from '../web/wasm/jit.mjs';
const [assetPath,cpiText='2',quantumText='64',secondsText='10'] = process.argv.slice(2);
const assets=JSON.parse(await fs.readFile(assetPath,'utf8'));
const cpi=Number(cpiText), quantum=Number(quantumText), seconds=Number(secondsText);
const enc=new TextEncoder(), dec=new TextDecoder();
let w, serial='', frames=0;
const host=createJitHost(()=>w);
const mem=()=>new Uint8Array(w.memory.buffer);
const logs=[];
w=(await WebAssembly.instantiate(await fs.readFile(new URL('../web/wasm/esp32sim.wasm',import.meta.url)),{env:{...host.imports,host_log:(p,n)=>logs.push(dec.decode(mem().subarray(p,p+n))),host_profile_now:()=>performance.now()}})).instance.exports;
const bytes=(b,f)=>{const p=w.esp32sim_alloc(b.length);mem().set(b,p);try{return f(p,b.length);}finally{w.esp32sim_free(p,b.length);}};
const emu=bytes(enc.encode('waveshare-amoled18-v2'),(p,n)=>w.esp32sim_new(p,n,16,8));
for(const [name,kind] of [['rom',0],['bootloader',1],['ptable',2],['app',3],['elf',4]]){
  const rc=bytes(await fs.readFile(assets[name]),(p,n)=>w.esp32sim_load(emu,kind,p,n));if(rc)throw Error(`${name}: ${rc}`);
}
if(cpi && w.esp32sim_set_approximate_jit_timing(emu,cpi,quantum))throw Error('timing config rejected');
if(w.esp32sim_boot(emu,0))throw Error('boot failed');
w.esp32sim_set_jit(emu,1);
const hz=w.esp32sim_cpu_hz(emu),start=performance.now();let stop=0;
while(w.esp32sim_cycles(emu)<hz*seconds && performance.now()-start<120000){
 stop=w.esp32sim_run(emu,2000000,Date.now());
 const n=w.esp32sim_out_take(emu);
 for(let i=0;i<n;i++){
  if(w.esp32sim_out_kind(emu,i)!==1){frames++;continue;}
  const p=w.esp32sim_out_ptr(emu,i),len=w.esp32sim_out_len(emu,i);
  const msg=JSON.parse(dec.decode(mem().subarray(p,p+len)));
  if(msg.t==='serial' && msg.src==='usb')serial+=msg.data;
 }
 if(stop || /Guru Meditation|stack overflow|task_wdt/.test(serial) || logs.some(l=>/chip reset|panic/i.test(l)))break;
 if(/TINYDRAW_GATE1_AUTOMATED_DONE[^\r\n]*[\r\n]/.test(serial))break;
}
console.log(JSON.stringify({mode:'Node functional smoke, not performance evidence',cpi,quantum,stop,guestSeconds:w.esp32sim_cycles(emu)/hz,wallSeconds:(performance.now()-start)/1000,instructions:w.esp32sim_insns(emu),jitInstructions:w.esp32sim_block_jit_insns(emu),jit:host.stats,frames,logs,serial},null,2));
w.esp32sim_delete(emu);
