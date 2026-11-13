// Explicit source states on controlled process rows; no commands reach live targets.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
const template=fixture.snapshot.processes[0];
const ready={valid:true,partial:false,missing:0};
const missing={valid:false,partial:false,missing:2};
const stale={valid:false,partial:false,missing:4};
fixture.snapshot.processNetwork=true;
fixture.snapshot.processes=[
 ['idle.exe','C:/idle.exe',ready,true,0,0],
 ['denied.exe','C:/denied.exe',missing,false,99,99999],
 ['retained.exe','C:/retained.exe',stale,true,12,1200],
 ['partial.exe','C:/partial.exe',ready,true,20,2000],
 ['partial.exe','C:/partial.exe',missing,false,99,99999]
].map(([name,path,state,ioValid,gpu,net],index)=>({...template,pid:44101+index,createTime:1792249465000+index,name,path,description:null,appWindow:true,windowsProcess:false,cpu:0,ioRead:ioValid?0:99999,ioWrite:0,ioOther:0,ioValid,gpu,gpuEngine:null,gpuEngines:[],gpuState:state,netSent:0,netReceived:net,netState:state,netReused:index===2}));
mkdirSync('target',{recursive:true});
writeFileSync('target/live-coverage.json',JSON.stringify(fixture));
writeFileSync('target/live-coverage-steps.json',JSON.stringify({window:[1500,900],steps:[
 {control:{page:'processes',view:'list',pointerOutside:true},wait:900,inspect:'layout',label:'list',expect:{'processTree.visibleRows':5},capture:'list'},
 {control:{view:'apps',pointerOutside:true},wait:900,inspect:'layout',label:'groups',capture:'groups'},
 {control:{contentZoom:1.5,theme:'light',pointerOutside:true},probe:'size 1100 800',wait:700,capture:'large-text'}
]},null,2));
