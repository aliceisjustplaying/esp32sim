import {runBattery} from './battery.mjs';
onmessage=({data})=>runBattery(async name=>new Uint8Array(await(await fetch('/asset/'+name)).arrayBuffer()),e=>postMessage(e),data.jit!==false,data.chain===true).catch(e=>postMessage({type:'error',line:e.stack||String(e)}));
